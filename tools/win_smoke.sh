#!/usr/bin/env bash
# 在 Windows 真机上对交叉编译产物做一次自动化冒烟测试。
#
# 用法（必须在 WSL 中执行，且 powershell.exe 可互操作）：
#   bash tools/win_smoke.sh [目标目录]
#
# 默认目标目录是 /mnt/d/Liftly-verify，不会覆盖用户自己的 D:\Liftly。
# 全过程约 2 分半，其中约 1 分钟会锁住键鼠（工作 1 分钟 + 休息 1 分钟）。
# 同时检查桌面倒计时气泡在工作时显示、休息时隐藏、结束后恢复。
# 输出同时写入 .toolchain/winsmoke.log。
#
# 每个 Windows 调用都带 timeout 和 </dev/null，避免 WSL 自动化调用卡死。
# 复杂的 PowerShell 逻辑放在目标目录的 .winsmoke-ps/*.ps1 中，用 -File 调用，
# 避免多行命令经 WSL 互操作传参时被引号或换行破坏。

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXE="${LIFTLY_EXE:-$ROOT/target/x86_64-pc-windows-gnu/release/liftly.exe}"
TARGET_DIR="${1:-/mnt/d/Liftly-verify}"
LOG="${LIFTLY_SMOKE_LOG:-$ROOT/.toolchain/winsmoke.log}"

mkdir -p "$ROOT/.toolchain"
exec > >(tee -a "$LOG") 2>&1

PASSED=0
FAILED=0
MAIN_PID=""
SECOND_PID=""
WORKING_SET_MB=""

say() { printf '%s\n' "$*"; }
info() { printf 'INFO  %s\n' "$*"; }
pass() { PASSED=$((PASSED + 1)); printf 'PASS  %s\n' "$*"; }
fail() { FAILED=$((FAILED + 1)); printf 'FAIL  %s\n' "$*"; }

# 正常结束或中途失败时，确保不把测试进程和全屏遮罩留在用户机器上。
cleanup() {
  local rc=$?
  trap - EXIT INT TERM
  if [ -n "$SECOND_PID" ]; then
    info "清理：结束第二个实例 PID=$SECOND_PID"
    timeout 10 taskkill.exe /PID "$SECOND_PID" /F </dev/null >/dev/null 2>&1 || true
  fi
  if [ -n "$MAIN_PID" ]; then
    info "清理：结束主实例 PID=$MAIN_PID"
    timeout 10 taskkill.exe /PID "$MAIN_PID" /F </dev/null >/dev/null 2>&1 || true
    sleep 1
  fi
  exit "$rc"
}
trap cleanup EXIT INT TERM

say "=================================================================="
say "Liftly Windows 真机冒烟测试"
say "时间：$(date -Is)"
say "产物：$EXE"
say "目标：$TARGET_DIR"
say "=================================================================="

if ! command -v powershell.exe >/dev/null 2>&1; then
  say "无法找到 powershell.exe：请在支持 Windows 互操作的 WSL 中运行。"
  exit 2
fi
for cmd in tasklist.exe taskkill.exe wslpath; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    say "缺少命令：$cmd"
    exit 2
  fi
done
if [ ! -f "$EXE" ]; then
  say "找不到产物：$EXE"
  say "请先运行：cargo zigbuild --release --target x86_64-pc-windows-gnu"
  exit 2
fi

TARGET_WIN="$(wslpath -w "$TARGET_DIR")" || { say "无法转换目标目录为 Windows 路径"; exit 2; }
WIN_EXE="$(wslpath -w "$TARGET_DIR/liftly.exe")" || { say "无法转换 exe 路径"; exit 2; }
case "$TARGET_WIN" in
  [A-Za-z]:\\*) ;;
  *)
    say "目标目录必须位于 Windows 盘符下（例如 /mnt/d/Liftly-verify），当前为：$TARGET_WIN"
    exit 2
    ;;
esac
# 这些字符会破坏下面拼装的 PowerShell 命令，直接拒绝。
case "$TARGET_WIN$WIN_EXE" in
  *'"'*|*'$'*|*'`'*|*'|'*|*'&'*)
    say "目标路径包含不支持的字符（\" \$ \` | &）：$TARGET_WIN"
    exit 2
    ;;
esac

# 所有 PowerShell 调用统一超时并禁止读取 stdin；脚本文件用 -File 传入。
ps_run() {
  local limit="$1"
  shift
  timeout "$limit" powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$@" </dev/null 2>&1
}

proc_alive() {
  local pid="$1"
  local out
  out="$(ps_run 12 "$PS_PROC_WIN" -ProcessId "$pid" -Action alive)" || return 1
  [ "$(printf '%s' "$out" | tr -d '\r\n ')" = "ALIVE" ]
}

count_class() {
  local pid="$1" class="$2" text="$3"
  # Windows PowerShell 经 WSL 管道输出的是 CRLF，必须先去掉 \r，否则 $3 会是 "1\r"。
  printf '%s\n' "$text" | tr -d '\r' | awk -F'|' -v p="$pid" -v c="$class" \
    '$1 == p && $2 == c && $3 == "1" { n++ } END { print n + 0 }'
}

wait_until() {
  local target="$1"
  if [ "$SECONDS" -ge "$target" ]; then
    return 0
  fi
  sleep $((target - SECONDS))
}

pid_from_output() {
  printf '%s\n' "$1" | tr -d '\r' | sed -n 's/^LIFTLY_PID=\([0-9][0-9]*\)$/\1/p' | tail -n 1
}

# 写辅助 PowerShell 脚本。全部为 ASCII，避免 Windows PowerShell 5.1 的编码问题。
write_ps_scripts() {
  mkdir -p "$PS_DIR"
  cat > "$PS_DIR/start.ps1" <<'PS'
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$WorkDir
)
$ErrorActionPreference = "Stop"
try {
    $p = Start-Process -FilePath $Exe -WorkingDirectory $WorkDir -ArgumentList "--work", "1", "--break", "1", "--warn", "10" -PassThru
    Write-Output ("LIFTLY_PID=" + $p.Id)
} catch {
    Write-Output ("ERROR: " + $_.Exception.Message)
    exit 1
}
PS

  cat > "$PS_DIR/proc.ps1" <<'PS'
param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [ValidateSet("alive", "workingset")][string]$Action = "alive"
)
$p = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
switch ($Action) {
    "alive" {
        if ($null -ne $p) { Write-Output "ALIVE" } else { Write-Output "GONE" }
    }
    "workingset" {
        if ($null -ne $p) { Write-Output ([math]::Round($p.WorkingSet64 / 1MB, 1)) }
    }
}
PS

  cat > "$PS_DIR/count.ps1" <<'PS'
param([string]$ProcessName = "liftly")
Write-Output (@(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue).Count)
PS

  cat > "$PS_DIR/monitors.ps1" <<'PS'
Add-Type -AssemblyName System.Windows.Forms
Write-Output ("MONITORS=" + [System.Windows.Forms.Screen]::AllScreens.Count)
PS

  cat > "$PS_DIR/list_windows.ps1" <<'PS'
$ErrorActionPreference = "Stop"
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class LiftlyWin {
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hWnd, StringBuilder sb, int n);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr hWnd);
    [DllImport("user32.dll")] static extern uint GetDpiForSystem();
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }
    public static string[] List() {
        var list = new List<string>();
        EnumWindows(delegate(IntPtr h, IntPtr l) {
            var sb = new StringBuilder(256);
            GetClassName(h, sb, 256);
            string cn = sb.ToString();
            if (cn.StartsWith("Liftly.") || cn == "#32770") {
                uint pid;
                GetWindowThreadProcessId(h, out pid);
                RECT rect;
                GetWindowRect(h, out rect);
                uint dpi = GetDpiForWindow(h);
                uint systemDpi = GetDpiForSystem();
                list.Add(pid.ToString() + "|" + cn + "|" + (IsWindowVisible(h) ? "1" : "0")
                    + "|" + rect.Left + "|" + rect.Top
                    + "|" + (rect.Right - rect.Left) + "|" + (rect.Bottom - rect.Top)
                    + "|" + dpi + "|" + systemDpi);
            }
            return true;
        }, IntPtr.Zero);
        return list.ToArray();
    }
}
"@
[LiftlyWin]::List()
PS
}

# ---------------------------------------------------------------------------
# a. 已有实例检查：发现任何 liftly.exe 都不杀，直接退出
# ---------------------------------------------------------------------------
say ""
say "== a. 检查是否已有 liftly.exe 在运行 =="
TL_OUT="$(timeout 30 tasklist.exe /FI "IMAGENAME eq liftly.exe" /FO CSV /NH </dev/null 2>&1)"
TL_RC=$?
printf '%s\n' "$TL_OUT"
if [ "$TL_RC" -ne 0 ]; then
  fail "a. tasklist 执行失败（退出码 $TL_RC）"
  exit 1
fi
if printf '%s\n' "$TL_OUT" | grep -qi 'liftly\.exe'; then
  fail "a. 已有 liftly.exe 在运行；按约定不结束它，请先从托盘退出后重试"
  exit 3
fi
pass "a. 当前没有 liftly.exe 在运行"

# ---------------------------------------------------------------------------
# b. 复制产物，并写入辅助 PowerShell 脚本
# ---------------------------------------------------------------------------
say ""
say "== b. 复制 liftly.exe 到目标目录 =="
mkdir -p "$TARGET_DIR" || { fail "b. 无法创建目标目录"; exit 1; }
if ! cp -f "$EXE" "$TARGET_DIR/liftly.exe"; then
  fail "b. 复制失败（目标文件可能被占用）"
  exit 1
fi
SRC_SUM="$(sha256sum "$EXE" | awk '{print $1}')"
DST_SUM="$(sha256sum "$TARGET_DIR/liftly.exe" | awk '{print $1}')"
if [ "$SRC_SUM" = "$DST_SUM" ]; then
  pass "b. 复制完成，sha256=$DST_SUM"
else
  fail "b. 复制后校验和不一致：$SRC_SUM -> $DST_SUM"
  exit 1
fi
if [ -f "$TARGET_DIR/liftly.ini" ]; then
  info "目标目录已有 liftly.ini，将沿用其中配置；命令行参数会覆盖工作/休息/提醒时长"
fi

PS_DIR="$TARGET_DIR/.winsmoke-ps"
write_ps_scripts || { fail "b. 无法写入辅助 PowerShell 脚本"; exit 1; }
PS_START_WIN="$(wslpath -w "$PS_DIR/start.ps1")"
PS_PROC_WIN="$(wslpath -w "$PS_DIR/proc.ps1")"
PS_COUNT_WIN="$(wslpath -w "$PS_DIR/count.ps1")"
PS_MON_WIN="$(wslpath -w "$PS_DIR/monitors.ps1")"
PS_LIST_WIN="$(wslpath -w "$PS_DIR/list_windows.ps1")"
info "辅助脚本目录：$PS_DIR"

# ---------------------------------------------------------------------------
# b2. PowerShell 预检：在启动程序前确认 -File、Add-Type 和显示器枚举可用
# ---------------------------------------------------------------------------
say ""
say "== b2. PowerShell 预检（不启动程序） =="
PRE_OUT="$(ps_run 25 "$PS_LIST_WIN")"
PRE_RC=$?
printf '%s\n' "$PRE_OUT" | tr -d '\r'
if [ "$PRE_RC" -eq 0 ]; then
  pass "b2. PowerShell 可执行，EnumWindows/Add-Type 编译通过"
else
  fail "b2. PowerShell 预检失败（EnumWindows/Add-Type 退出码 $PRE_RC）"
  exit 1
fi
MON_PRE="$(ps_run 20 "$PS_MON_WIN" | tr -d '\r')"
MON_PRE_N="$(printf '%s\n' "$MON_PRE" | sed -n 's/^MONITORS=\([0-9][0-9]*\)$/\1/p' | tail -n 1)"
if [ -n "$MON_PRE_N" ] && [ "$MON_PRE_N" -ge 1 ]; then
  pass "b2. PowerShell 显示器枚举可用，显示器数=$MON_PRE_N"
else
  fail "b2. PowerShell 显示器枚举失败：$MON_PRE"
  exit 1
fi

# ---------------------------------------------------------------------------
# c. 启动主实例
# ---------------------------------------------------------------------------
say ""
say "== c. 启动主实例（--work 1 --break 1 --warn 10） =="
START_OUT="$(ps_run 30 "$PS_START_WIN" -Exe "$WIN_EXE" -WorkDir "$TARGET_WIN")"
START_RC=$?
printf '%s\n' "$START_OUT" | tr -d '\r'
MAIN_PID="$(pid_from_output "$START_OUT")"
if [ -z "$MAIN_PID" ]; then
  # 极端情况下 Start-Process 已拉起进程但输出没解析到：只有一个实例时兜底取 PID。
  FALLBACK="$(timeout 20 tasklist.exe /FI "IMAGENAME eq liftly.exe" /FO CSV /NH </dev/null 2>&1 | tr -d '\r' | awk -F'","' 'NR==1 { gsub(/"/, "", $2); print $2 }')"
  if [ -n "$FALLBACK" ]; then
    MAIN_PID="$FALLBACK"
    info "未从 PowerShell 输出解析到 PID，tasklist 兜底得到 PID=$MAIN_PID"
  fi
fi
if [ "$START_RC" -ne 0 ] || [ -z "$MAIN_PID" ]; then
  fail "c. 启动失败（退出码 $START_RC，PID 为空）"
  exit 1
fi
info "主实例 PID=$MAIN_PID"
sleep 3
if proc_alive "$MAIN_PID"; then
  pass "c. 3 秒后主实例仍在运行"
else
  fail "c. 3 秒后主实例已退出"
  MAIN_PID=""
  exit 1
fi

# ---------------------------------------------------------------------------
# c2. 启动 3 秒后不应残留可见的启动错误对话框（#32770）
# ---------------------------------------------------------------------------
say ""
say "== c2. 检查主实例没有启动错误对话框 =="
WIN_LIST="$(ps_run 25 "$PS_LIST_WIN")"
WIN_RC=$?
printf '%s\n' "$WIN_LIST" | tr -d '\r'
STARTUP_DIALOGS="$(count_class "$MAIN_PID" "#32770" "$WIN_LIST")"
if [ "$WIN_RC" -eq 0 ] && [ "$STARTUP_DIALOGS" -eq 0 ]; then
  pass "c2. 主实例没有可见的启动错误对话框"
else
  fail "c2. 检测到可见的启动错误对话框（数量 $STARTUP_DIALOGS，枚举退出码 $WIN_RC）"
fi

# ---------------------------------------------------------------------------
# c3. 启动约 3 秒：桌面倒计时气泡应已显示
# ---------------------------------------------------------------------------
say ""
say "== c3. 检查桌面倒计时气泡 Liftly.CountdownBubble =="
BUBBLE_COUNT="$(count_class "$MAIN_PID" "Liftly.CountdownBubble" "$WIN_LIST")"
if [ "$WIN_RC" -eq 0 ] && [ "$BUBBLE_COUNT" -ge 1 ]; then
  pass "c3. 检测到可见的 Liftly.CountdownBubble 窗口"
  # 浅绿双行 UI 在 96 DPI、100% 下约 180x42；按窗口或系统 DPI 与 bubble_scale 校验，允许 2px 舍入误差。
  BUBBLE_SCALE=100
  if [ -f "$TARGET_DIR/liftly.ini" ]; then
    BUBBLE_SCALE_RAW="$(awk -F= '
      {
        key=$1
        sub(/^[ \t\r]+/, "", key)
        sub(/[ \t\r]+$/, "", key)
      }
      tolower(key)=="bubble_scale" {
        val=$2
        sub(/[;#].*/, "", val)
        sub(/^[ \t\r]+/, "", val)
        sub(/[ \t\r]+$/, "", val)
        print val
        exit
      }
    ' "$TARGET_DIR/liftly.ini")"
    if [ -n "$BUBBLE_SCALE_RAW" ] && [ "$BUBBLE_SCALE_RAW" -ge 50 ] 2>/dev/null \
      && [ "$BUBBLE_SCALE_RAW" -le 300 ] 2>/dev/null; then
      BUBBLE_SCALE="$BUBBLE_SCALE_RAW"
    fi
  fi
  BUBBLE_LINE="$(printf '%s\n' "$WIN_LIST" | tr -d '\r' | awk -F'|' \
    -v p="$MAIN_PID" '$1 == p && $2 == "Liftly.CountdownBubble" && $3 == "1" { print; exit }')"
  if [ -n "$BUBBLE_LINE" ]; then
    B_WIDTH="$(printf '%s\n' "$BUBBLE_LINE" | cut -d'|' -f6)"
    B_HEIGHT="$(printf '%s\n' "$BUBBLE_LINE" | cut -d'|' -f7)"
    B_DPI="$(printf '%s\n' "$BUBBLE_LINE" | cut -d'|' -f8)"
    B_SYSTEM_DPI="$(printf '%s\n' "$BUBBLE_LINE" | cut -d'|' -f9)"
    size_matches() {
      local width="$1" height="$2" dpi="${3:-0}" scale="${4:-100}"
      if [ -z "$width" ] || [ -z "$height" ] || [ "$dpi" -le 0 ] 2>/dev/null; then
        return 1
      fi
      local effective=$(((dpi * scale + 50) / 100))
      [ "$effective" -lt 1 ] && effective=1
      local expect_width=$(((180 * effective + 48) / 96))
      local expect_height=$(((42 * effective + 48) / 96))
      local diff_width=$((width - expect_width))
      local diff_height=$((height - expect_height))
      [ "$diff_width" -lt 0 ] && diff_width=$((-diff_width))
      [ "$diff_height" -lt 0 ] && diff_height=$((-diff_height))
      [ "$diff_width" -le 2 ] && [ "$diff_height" -le 2 ]
    }
    EXPECT_DPI="$B_DPI"
    if ! size_matches "$B_WIDTH" "$B_HEIGHT" "$B_DPI" "$BUBBLE_SCALE"; then
      EXPECT_DPI="$B_SYSTEM_DPI"
    fi
    if size_matches "$B_WIDTH" "$B_HEIGHT" "$B_DPI" "$BUBBLE_SCALE" \
      || size_matches "$B_WIDTH" "$B_HEIGHT" "$B_SYSTEM_DPI" "$BUBBLE_SCALE"; then
      pass "c3. 气泡尺寸 ${B_WIDTH}x${B_HEIGHT} 符合紧凑基准（DPI=$EXPECT_DPI, scale=${BUBBLE_SCALE}%）"
    else
      fail "c3. 气泡尺寸 ${B_WIDTH}x${B_HEIGHT} 不符合紧凑基准（窗口 DPI=$B_DPI，系统 DPI=$B_SYSTEM_DPI，scale=${BUBBLE_SCALE}%）"
    fi
  else
    info "c3. 未能解析气泡窗口矩形，跳过尺寸校验"
  fi
else
  fail "c3. 未检测到可见的 Liftly.CountdownBubble 窗口（枚举退出码 $WIN_RC，数量 $BUBBLE_COUNT）"
fi

# ---------------------------------------------------------------------------
# d. 第二个实例：应弹“已经在运行”对话框，确认存在后按 PID 结束
# ---------------------------------------------------------------------------
say ""
say "== d. 启动第二个实例并确认单实例提示 =="
SECOND_OUT="$(ps_run 30 "$PS_START_WIN" -Exe "$WIN_EXE" -WorkDir "$TARGET_WIN")"
SECOND_PID="$(pid_from_output "$SECOND_OUT")"
printf '%s\n' "$SECOND_OUT" | tr -d '\r'
if [ -z "$SECOND_PID" ]; then
  fail "d. 第二个实例没有返回 PID"
else
  sleep 3
  PROC_COUNT="$(ps_run 15 "$PS_COUNT_WIN" -ProcessName liftly | tr -d '\r\n ')"
  info "当前 liftly 进程数=$PROC_COUNT"
  if proc_alive "$SECOND_PID" && [ "${PROC_COUNT:-0}" -ge 2 ]; then
    pass "d. 第二个实例存在并阻塞在提示对话框（PID=$SECOND_PID），说明单实例互斥量生效"
  else
    fail "d. 第二个实例未保持运行（PID=$SECOND_PID，进程数=$PROC_COUNT）"
  fi
  timeout 15 taskkill.exe /PID "$SECOND_PID" /F </dev/null >/dev/null 2>&1
  sleep 1
  if proc_alive "$SECOND_PID"; then
    fail "d. 无法结束第二个实例 PID=$SECOND_PID"
  else
    pass "d. 第二个实例已按 PID 强制结束，主实例未受影响"
    SECOND_PID=""
  fi
fi

# ---------------------------------------------------------------------------
# e. 约 52 秒：提醒弹窗应出现
# ---------------------------------------------------------------------------
say ""
say "== e. 约 52 秒检查提醒弹窗 Liftly.WarnPopup =="
wait_until 52
WIN_LIST="$(ps_run 25 "$PS_LIST_WIN")"
WIN_RC=$?
printf '%s\n' "$WIN_LIST" | tr -d '\r'
POPUP_COUNT="$(count_class "$MAIN_PID" "Liftly.WarnPopup" "$WIN_LIST")"
if [ "$WIN_RC" -eq 0 ] && [ "$POPUP_COUNT" -ge 1 ]; then
  pass "e. 检测到可见的 Liftly.WarnPopup 窗口"
else
  fail "e. 未检测到可见的 Liftly.WarnPopup 窗口（枚举退出码 $WIN_RC，数量 $POPUP_COUNT）"
fi
BUBBLE_COUNT="$(count_class "$MAIN_PID" "Liftly.CountdownBubble" "$WIN_LIST")"
if [ "$WIN_RC" -eq 0 ] && [ "$BUBBLE_COUNT" -ge 1 ]; then
  pass "e. 提醒阶段桌面倒计时气泡仍然可见"
else
  fail "e. 提醒阶段桌面倒计时气泡不可见（枚举退出码 $WIN_RC，数量 $BUBBLE_COUNT）"
fi

# ---------------------------------------------------------------------------
# f. 约 65 秒：每个显示器一个遮罩
# ---------------------------------------------------------------------------
say ""
say "== f. 约 65 秒检查遮罩数量与显示器数量 =="
wait_until 65
MON_OUT="$(ps_run 20 "$PS_MON_WIN" | tr -d '\r')"
MONITORS="$(printf '%s\n' "$MON_OUT" | sed -n 's/^MONITORS=\([0-9][0-9]*\)$/\1/p' | tail -n 1)"
WIN_LIST="$(ps_run 25 "$PS_LIST_WIN")"
WIN_RC=$?
printf '%s\n' "$WIN_LIST" | tr -d '\r'
OVERLAYS="$(count_class "$MAIN_PID" "Liftly.Overlay" "$WIN_LIST")"
if [ -n "$MONITORS" ] && [ "$MONITORS" -ge 1 ] && [ "$WIN_RC" -eq 0 ] && [ "$OVERLAYS" -eq "$MONITORS" ]; then
  pass "f. 遮罩窗口数 $OVERLAYS 等于显示器数 $MONITORS"
else
  fail "f. 遮罩窗口数 $OVERLAYS 与显示器数 ${MONITORS:-未知} 不一致（枚举退出码 $WIN_RC）"
fi
BUBBLE_COUNT="$(count_class "$MAIN_PID" "Liftly.CountdownBubble" "$WIN_LIST")"
if [ "$WIN_RC" -eq 0 ] && [ "$BUBBLE_COUNT" -eq 0 ]; then
  pass "f. 休息时桌面倒计时气泡已隐藏"
else
  fail "f. 休息时仍有桌面倒计时气泡（数量 $BUBBLE_COUNT，枚举退出码 $WIN_RC）"
fi

# ---------------------------------------------------------------------------
# g. 约 130 秒：休息结束，遮罩消失，主进程仍存活
# ---------------------------------------------------------------------------
say ""
say "== g. 约 130 秒检查遮罩消失、主进程存活和内存占用 =="
wait_until 130
WIN_LIST="$(ps_run 25 "$PS_LIST_WIN")"
WIN_RC=$?
printf '%s\n' "$WIN_LIST" | tr -d '\r'
OVERLAYS="$(count_class "$MAIN_PID" "Liftly.Overlay" "$WIN_LIST")"
POPUP_COUNT="$(count_class "$MAIN_PID" "Liftly.WarnPopup" "$WIN_LIST")"
BUBBLE_COUNT="$(count_class "$MAIN_PID" "Liftly.CountdownBubble" "$WIN_LIST")"
if [ "$WIN_RC" -eq 0 ] && [ "$OVERLAYS" -eq 0 ] && [ "$POPUP_COUNT" -eq 0 ]; then
  pass "g. 遮罩和提醒弹窗均已消失（Overlay=$OVERLAYS, Popup=$POPUP_COUNT）"
else
  fail "g. 仍有残留窗口（Overlay=$OVERLAYS, Popup=$POPUP_COUNT, 枚举退出码 $WIN_RC）"
fi
if [ "$WIN_RC" -eq 0 ] && [ "$BUBBLE_COUNT" -ge 1 ]; then
  pass "g. 休息结束后桌面倒计时气泡已恢复"
else
  fail "g. 休息结束后桌面倒计时气泡未恢复（数量 $BUBBLE_COUNT，枚举退出码 $WIN_RC）"
fi
if proc_alive "$MAIN_PID"; then
  pass "g. 主实例仍存活"
  WORKING_SET_MB="$(ps_run 15 "$PS_PROC_WIN" -ProcessId "$MAIN_PID" -Action workingset | tr -d '\r\n ')"
  if [ -n "$WORKING_SET_MB" ]; then
    info "主实例 WorkingSet=${WORKING_SET_MB} MB"
  else
    info "未能读取 WorkingSet"
  fi
else
  fail "g. 主实例已退出"
fi

# ---------------------------------------------------------------------------
# h. 结束主实例：先 taskkill /PID，必要时 /F
# ---------------------------------------------------------------------------
say ""
say "== h. 结束主实例 =="
KILL_OUT="$(timeout 20 taskkill.exe /PID "$MAIN_PID" </dev/null 2>&1)"
KILL_RC=$?
printf '%s\n' "$KILL_OUT" | tr -d '\r'
for _ in 1 2 3 4 5; do
  proc_alive "$MAIN_PID" || break
  sleep 1
done
if proc_alive "$MAIN_PID"; then
  info "普通 taskkill 未结束进程，使用 /F"
  timeout 20 taskkill.exe /PID "$MAIN_PID" /F </dev/null >/dev/null 2>&1 || true
  sleep 2
fi
if proc_alive "$MAIN_PID"; then
  fail "h. 主实例 PID=$MAIN_PID 仍在运行（首次 taskkill 退出码 $KILL_RC）"
else
  pass "h. 主实例已退出"
  MAIN_PID=""
fi

# ---------------------------------------------------------------------------
# 汇总
# ---------------------------------------------------------------------------
say ""
say "=================================================================="
say "汇总：PASS=$PASSED FAIL=$FAILED 用时=${SECONDS}s"
say "日志：$LOG"
say "辅助脚本：$PS_DIR"
if [ -n "$WORKING_SET_MB" ]; then
  say "内存：WorkingSet=${WORKING_SET_MB} MB"
fi
say "=================================================================="
if [ "$FAILED" -ne 0 ]; then
  exit 1
fi
exit 0
