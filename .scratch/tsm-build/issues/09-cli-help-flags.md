# 09 — CLI 帮助参数

**What to build:** 为进程级 CLI 增加 `-h` 与 `--help` 参数。两个参数都应在打开 Store 或进入 TUI 前输出同一份简洁用法说明并成功退出；说明覆盖默认 TUI、`--db`、`--version`、`self-update` 与 `self-update --check`。

**Blocked by:** none

**Status:** in-progress

- [ ] `tsm -h` 输出帮助并以状态码 0 退出，不打开 Store 或进入 TUI。
- [ ] `tsm --help` 与 `-h` 输出相同帮助。
- [ ] 帮助列出当前支持的命令和参数。
- [ ] README 的 Usage 说明 `-h` / `--help`。

## Comments
