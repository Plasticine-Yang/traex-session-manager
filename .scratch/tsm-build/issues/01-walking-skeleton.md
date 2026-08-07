# 01 — 走通骨架:启动 → 当前项目活跃列表 → 退出

**What to build:** 在一个项目目录里跑 `tsm`,看到**本项目的活跃会话**列表(最新在前),能用 `j`/`k`/`g`/`G` 上下移动光标,按 `q` 或 `Ctrl-c` 退出。这是贯穿所有层(crate → `store` 读库 → `app` 状态 → `ui` 渲染 → crossterm 事件循环)的最窄完整脊柱;后续 ticket 都挂在它上面。库找不到/结构不认/首查即锁时清晰报错退出,不进 TUI。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §1(目标/非目标)、§2.1–2.5(路径解析、glob、列语义、只读打开、基础 SQL)、§5.1–5.2/5.5/5.6(范式 B、默认列、`App` 结构、主题)、§8(当前项目精确匹配)、§9.1–9.3(crate/模块/依赖/零配置)、§11(启动期致命错误)。

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] crate 建好:包名 `traex-session-manager`、`[[bin]] name = "tsm"`、`version = "0.1.0"`、`edition = "2024"`;模块骨架 `store`/`app`/`ui`(`mutate`/`rename` 可留空占位)。
- [ ] 依赖只含 `ratatui` + `crossterm` + `rusqlite{bundled}` + `unicode-width` + 可选 `anyhow`;**无** serde/tokio/HTTP crate。
- [ ] DB 路径按 `--db → $TRAECLI_HOME → $TRAE_HOME/cli → ~/.trae/cli` 解析,忽略 `CODEX_HOME`;不带任何参数可裸跑。
- [ ] 库文件按 `^state_(\d+)\.sqlite$` 取 max + `threads` 列校验后才信任;`--db` 直指文件同样校验。
- [ ] 只读连接用 `SQLITE_OPEN_READ_ONLY|URI|NO_MUTEX` + `busy_timeout(3000)` + `query_only`,**不用** `immutable`;每次刷新一条独立短查询。
- [ ] 启动目录用 `std::env::current_dir()`;查询 `WHERE cwd = ?1 AND archived = 0 ORDER BY updated_at_ms DESC`,`cwd` **逐字节精确比,不 canonicalize**。
- [ ] 列表渲染默认列 `✓ · updated(MM-DD HH:MM) · session · model · tokens`;`session` = `COALESCE(NULLIF(title,''),NULLIF(first_user_message,''),'(untitled)')`;`tokens` 人类可读(`442k`/`4.3M`);`session` 列 `unicode-width` 感知截断 + `…`。
- [ ] plasticine 暗色主题(bg `#16161e` / fg `#c0caf5` / cyan `#7dcfff` 光标高亮)。
- [ ] `App` 具备 `all_rows: Vec<Session>` + `view: Vec<usize>` 的 seam;`j`/`↓` `k`/`↑` 移光标、`g`/`G` 跳首尾。
- [ ] crossterm raw mode + alternate screen;`q` / `Ctrl-c` 干净退出并还原终端。
- [ ] 启动期致命错误清晰退出、非零码、不进 TUI:找不到 `state_(\d+).sqlite`(或 `--db` 文件不存在)、列校验失败/未知世代、首查 3s 后仍 `SQLITE_BUSY`。
- [ ] traex 正在写库时只读列表无锁错误、无脏数据(WAL 并发)。
