# 构建指南

## 1. 概述

站会儿（Liftly）是一个用 Rust 编写、通过 `windows-sys` 调用 Win32 API 的 Windows 托盘程序，最终产物是单个 `liftly.exe`。

推荐在 Linux/WSL 上使用 `cargo-zigbuild` 交叉编译，也可以在 Windows 本机直接用 MSVC 工具链构建。

## 2. 安装 Rust

在 Linux/WSL 上使用 rustup 官方安装命令：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

然后安装 Windows GNU 目标：

```bash
rustup target add x86_64-pc-windows-gnu
```

可以用下面的命令确认安装结果：

```bash
rustc --version
rustup target list --installed
```

`x86_64-pc-windows-gnu` 目标用于从 Linux 交叉编译；在 Windows 本机用 MSVC 工具链构建时不需要它。

## 3. 安装 zig 和 cargo-zigbuild

需要 Python 3。把下面的 `python3` 换成你的 Python 3 解释器。推荐先创建虚拟环境，避免污染系统环境；请把虚拟环境建在仓库目录之外，因为仓库的 `.gitignore` 没有忽略 `.venv`，建在仓库里会污染工作区：

```bash
python3 -m venv ~/.venvs/liftly-build
source ~/.venvs/liftly-build/bin/activate
python -m pip install cargo-zigbuild ziglang
```

如果不使用虚拟环境，也可以用 `python3 -m pip install --user cargo-zigbuild ziglang`，此时命令通常装在 `~/.local/bin`。

`ziglang` 这个 Python 包自带 zig，无需单独安装 zig；`cargo-zigbuild` 会通过它调用 zig。

安装完成后确认命令可用：

```bash
cargo-zigbuild --version
```

装好的 `cargo-zigbuild` 命令所在目录必须在 `PATH` 中：使用虚拟环境时是虚拟环境的 `bin` 目录（如 `~/.venvs/liftly-build/bin`，激活虚拟环境后会加入 `PATH`）；使用 `--user` 安装时通常是 `~/.local/bin`。

> 如果运行时报缓存目录不可写，可以把 `CARGO_ZIGBUILD_CACHE_DIR` 和 `ZIG_GLOBAL_CACHE_DIR` 设置为可写目录。

## 4. 在 Linux 上运行单元测试

```bash
cargo test
```

可以在 Linux 上运行，是因为 `src/main.rs` 用 `#[cfg(any(windows, test))]` 让 `config` 和 `schedule` 模块在测试时也参与编译，并通过测试专用模块运行 `build/rsrc.rs` 的 ICO 解析和 COFF 资源结构测试；而 Windows 相关的 `win` 模块只在 `windows` 下编译。这些测试不会覆盖托盘、遮罩和钩子等 Windows 运行时功能。

## 5. 交叉编译 release

```bash
cargo zigbuild --release --target x86_64-pc-windows-gnu
```

产物为：

```text
target/x86_64-pc-windows-gnu/release/liftly.exe
```

可以用 `file` 命令确认它确实是 Windows 可执行文件：

```bash
file target/x86_64-pc-windows-gnu/release/liftly.exe
```

构建脚本自动把 `assets/liftly.ico` 的 8 帧嵌入 exe：8 个 `RT_ICON`（ID 1–8）和 1 个 `RT_GROUP_ICON`（ID 1），语言 ID 为 0。资源通过纯 Rust 生成的 COFF 目标文件链接，不依赖 `windres`、`llvm-rc` 或 Windows SDK 的资源编译器，支持 x86_64、x86 和 aarch64 目标。

可安装 Rust 的 LLVM 工具，检查最终 exe 的资源目录（以下命令适用于 Linux/WSL）：

```bash
rustup component add llvm-tools
"$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-readobj" \
  --coff-resources target/x86_64-pc-windows-gnu/release/liftly.exe
```

## 6. 从 WSL 复制到 Windows 磁盘

编译完成后，把 exe 复制到 Windows 能访问的目录，例如：

```bash
mkdir -p /mnt/d/Liftly
cp target/x86_64-pc-windows-gnu/release/liftly.exe /mnt/d/Liftly/
```

`D:\Liftly` 只是示例目录，可以换成任意位置；`/mnt/d` 也要按实际挂载的盘符调整。

`liftly.ini` 必须和 `liftly.exe` 放在同一目录。需要自定义配置时，可以从仓库里的示例复制一份：

```bash
cp liftly.ini.example /mnt/d/Liftly/liftly.ini
```

如果目标位置的 `liftly.exe` 正在运行，复制会因为文件被占用而失败，需要先从托盘菜单退出程序再复制。

从 WSL 启动时，用 PowerShell 的 `Start-Process` 让程序脱离当前终端：

```bash
powershell.exe -NoProfile -Command "Start-Process -FilePath 'D:\Liftly\liftly.exe'"
```

不要在前台直接运行 exe，否则终端会一直阻塞到程序退出。也不要用 `cmd.exe /c start`：从 WSL 自动化调用时，`start` 可能一直等不到返回，实测出现过卡住 600 秒的情况。`Start-Process` 会把程序交给 Windows 后立即返回。

## 7. Windows 本机构建

在 Windows 上安装 Rust 时，使用 rustup 默认的 MSVC 工具链即可（需要安装 Visual Studio Build Tools 的 C++ 生成工具作为链接器）。

然后在项目目录执行：

```powershell
cargo build --release
```

产物为：

```text
target\release\liftly.exe
```

## 8. 快速试跑

用下面的参数可以在约 1 分钟内体验一次锁定：

```powershell
.\liftly.exe --work 1 --break 1 --warn 10
```

PowerShell 运行当前目录下的程序需要 `.\` 前缀。从 WSL 启动可用：

```bash
powershell.exe -NoProfile -Command "Start-Process -FilePath 'D:\Liftly\liftly.exe' -ArgumentList '--work','1','--break','1','--warn','10'"
```

参数范围：

- `--work`：`1..=600`，工作时长（分钟）
- `--break`：`1..=120`，休息时长（分钟）
- `--warn`：`0..=600`，锁定前提醒秒数

当 `warn` 大于等于工作时长时，会被截断为工作时长减 1 秒。所以 `--work 1` 且不加 `--warn` 时，默认提醒 60 秒会被截断为 59 秒，启动约 1 秒就会弹出提醒（工作 1 分钟后才进入锁定）。

休息期间同时按住 Ctrl、Alt、Shift（左右均可）再按 F12 可以紧急解锁。此功能需在 `liftly.ini` 中设置 `emergency_unlock = true`（默认开启），命令行参数无法修改此项。Ctrl+Alt+Del 由系统直接处理，是最终兜底。

## 9. 真机验证

`cargo test` 覆盖 `config`、`schedule` 及图标资源生成器，交叉编译和资源检查可以验证 PE 格式、DLL 依赖及嵌入图标结构；托盘、提醒弹窗、遮罩、输入钩子和多显示器行为必须在 Windows 真机上确认。

仓库自带一个自动化冒烟脚本，从 WSL 通过 `powershell.exe` 操作 Windows：

```bash
bash tools/win_smoke.sh
# 指定其他目标目录
bash tools/win_smoke.sh /mnt/d/Liftly-verify
```

默认目标目录是 `D:\Liftly-verify`，脚本只会覆盖该目录下的 `liftly.exe`，不会碰 `D:\Liftly`。运行前请确认：

- 已完成第 5 节的交叉编译；
- WSL 能调用 `powershell.exe`、`tasklist.exe`、`taskkill.exe`；
- 没有正在运行的 `liftly.exe`。脚本发现已有实例时会记录后直接退出，不会结束它。

脚本以 `--work 1 --break 1 --warn 10` 启动，全程约 2 分半，其中约 1 分钟会锁住键鼠、覆盖所有显示器。它按顺序检查：

| 步骤 | 检查内容 |
| --- | --- |
| a | 没有已运行的 `liftly.exe` |
| b | 产物复制到目标目录且 sha256 一致；写入辅助 PowerShell 脚本 |
| b2 | 启动前预检 PowerShell、`Add-Type`/EnumWindows 和显示器枚举 |
| c | 主实例启动 3 秒后仍存活 |
| d | 第二个实例停在“站会儿已经在运行”对话框；按 PID 强制结束后主实例不受影响 |
| e | 约 52 秒时存在可见的 `Liftly.WarnPopup` 窗口 |
| f | 约 65 秒时 `Liftly.Overlay` 窗口数等于显示器数 |
| g | 约 130 秒时遮罩和弹窗消失、主进程存活，并记录 WorkingSet |
| h | 先 `taskkill /PID`，必要时 `/F`，确认主进程退出 |

每一步打印 `PASS` 或 `FAIL`，完整输出同时写入 `.toolchain/winsmoke.log`。脚本在失败或按 Ctrl+C 时会清理自己启动的进程，避免遮罩留在屏幕上。

> 脚本会把辅助 PowerShell 文件写到目标目录的 `.winsmoke-ps/` 下。它不能替代人工检查输入钩子是否真的拦住键鼠。

### 需要人工确认的项目

自动化脚本通过后，再按下面的清单手动过一遍：

- [ ] 托盘和资源管理器显示 Liftly 图标；右键菜单的四项（立即休息、暂停/继续计时、重新开始计时、退出）都能工作。Explorer 可能缓存旧图标，必要时改文件名或清理图标缓存。
- [ ] 重启 Explorer 后托盘图标重新出现，仍显示 Liftly 图标；资源管理器中的 exe 图标正常。
- [ ] 休息中同时按住 Ctrl、Alt、Shift（左右均可）再按 F12 能紧急解锁；把 `emergency_unlock` 设为 `false` 后该组合键无效。
- [ ] 在 100%/125%/150%/200% 缩放和多显示器（不同 DPI、副屏在左/上方）下，提醒弹窗位于主屏右下角，每个显示器都被遮罩完整覆盖。
- [ ] 休息中键鼠输入被拦住；切到管理员权限窗口时受 UIPI 限制属于已知限制。
- [ ] `Ctrl+Alt+Del` 能打开系统安全界面，作为最终兜底。
- [ ] 锁屏后解锁、睡眠唤醒后，计时按 `reset_on_unlock` 的预期重新开始；休息中锁屏不会让遮罩卡死。

## 10. 常见问题

- **找不到 `cargo-zigbuild`**：确认虚拟环境的 `bin` 目录或 `~/.local/bin` 已在 `PATH` 中；虚拟环境需要先激活。
- **提示缺少 target**：执行 `rustup target add x86_64-pc-windows-gnu`。
- **第二个实例提示“站会儿已经在运行”后退出**：程序使用命名互斥量保证单实例，这是正常行为。
- **焦点在管理员权限窗口时钩子拦不到输入**：这是 Windows 的 UIPI 限制，普通权限进程的低级钩子无法拦截更高完整性级别窗口的输入。
- **复制 exe 失败**：目标 exe 正在运行，文件被占用；先从托盘退出程序再复制。

## 11. 开机自启

开机自启见 README 的「开机自启」一节。
