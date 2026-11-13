# Liftly Agent 约定

## 构建

- 交叉编译：`cargo zigbuild --release --target x86_64-pc-windows-gnu`，产物是 `target/x86_64-pc-windows-gnu/release/liftly.exe`。环境搭建见 `docs/BUILD.md`。
- 能在 Linux 上完成的检查先在沙箱里做完，再做下面的 Windows 冒烟测试。

## Windows 冒烟测试

沙箱里调不了 Windows 程序，冒烟测试要在沙箱外执行。执行器的白名单里有这条命令，申请后会自动批准、马上执行，不用等用户。

- 改动会影响 `liftly.exe` 的行为时，交叉编译通过后，在回复最后单独一行输出：

  ```
  NEEDS_ELEVATION: {"command": "bash tools/win_smoke.sh", "reason": "在 Windows 上冒烟测试新构建的 liftly.exe"}
  ```

  命令必须原样：不加参数，不拼接其他命令，不加管道或重定向。白名单逐字匹配，改一个字就会变成等用户输入验证码批准。
- 全程约 2 分半，其中约 1 分钟会锁住键盘和鼠标。结果看交回的命令输出，完整日志在 `.toolchain/winsmoke.log`。
- 失败时按输出修复，重新交叉编译后再申请一次。
- 只改文档、README 截图、`tools/gen_icon.py` 这类不影响 `liftly.exe` 的改动，不需要跑。
- 不要为了让测试通过去改 `tools/win_smoke.sh`。测试本身确实需要调整时，在摘要里说明原因，由用户决定。
