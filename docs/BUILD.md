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

可以在 Linux 上运行，是因为 `src/main.rs` 用 `#[cfg(any(windows, test))]` 让 `config` 和 `schedule` 模块在测试时也参与编译；而 Windows 相关的 `win` 模块只在 `windows` 下编译。因此这两个纯逻辑模块的单元测试能在 Linux 上跑，但不会覆盖托盘、遮罩和钩子等 Windows 功能。

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

从 WSL 启动时，建议用下面的方式让程序脱离当前终端：

```bash
cmd.exe /c start "" 'D:\Liftly\liftly.exe'
```

不要在前台直接运行 exe，否则终端会一直阻塞到程序退出。

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

PowerShell 运行当前目录下的程序需要 `.\` 前缀。从 WSL 启动可用 `cmd.exe /c start "" 'D:\Liftly\liftly.exe' --work 1 --break 1 --warn 10`。

参数范围：

- `--work`：`1..=600`，工作时长（分钟）
- `--break`：`1..=120`，休息时长（分钟）
- `--warn`：`0..=600`，锁定前提醒秒数

当 `warn` 大于等于工作时长时，会被截断为工作时长减 1 秒。所以 `--work 1` 且不加 `--warn` 时，默认提醒 60 秒会被截断为 59 秒，启动约 1 秒就会弹出提醒（工作 1 分钟后才进入锁定）。

休息期间同时按住 Ctrl、Alt、Shift（左右均可）再按 F12 可以紧急解锁。此功能需在 `liftly.ini` 中设置 `emergency_unlock = true`（默认开启），命令行参数无法修改此项。Ctrl+Alt+Del 由系统直接处理，是最终兜底。

## 9. 常见问题

- **找不到 `cargo-zigbuild`**：确认虚拟环境的 `bin` 目录或 `~/.local/bin` 已在 `PATH` 中；虚拟环境需要先激活。
- **提示缺少 target**：执行 `rustup target add x86_64-pc-windows-gnu`。
- **第二个实例提示“站会儿已经在运行”后退出**：程序使用命名互斥量保证单实例，这是正常行为。
- **焦点在管理员权限窗口时钩子拦不到输入**：这是 Windows 的 UIPI 限制，普通权限进程的低级钩子无法拦截更高完整性级别窗口的输入。
- **复制 exe 失败**：目标 exe 正在运行，文件被占用；先从托盘退出程序再复制。

## 10. 开机自启

开机自启见 README 的「开机自启」一节。
