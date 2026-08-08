# 09 — CLI 帮助参数

**What to build:** 为进程级 CLI 增加 `-h` 与 `--help` 参数。两个参数都应在打开 Store 或进入 TUI 前输出同一份简洁用法说明并成功退出；说明覆盖默认 TUI、`--db`、`--version`、`self-update` 与 `self-update --check`。

**Blocked by:** none

**Status:** done (15ef34c)

- [x] `tsm -h` 输出帮助并以状态码 0 退出，不打开 Store 或进入 TUI。
- [x] `tsm --help` 与 `-h` 输出相同帮助。
- [x] 帮助列出当前支持的命令和参数。
- [x] README 的 Usage 说明 `-h` / `--help`。

## Comments

- 2026-08-08: 实现提交 `15ef34c`;验证 `cargo test`(104 通过)、`cargo fmt --check`、`git diff --check`,并真实运行 `cargo run --quiet -- -h` 与 `cargo run --quiet -- --help`,确认输出一致且成功退出。
