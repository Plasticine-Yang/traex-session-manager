# 02 — Scope + Lifecycle 切换 + 预览面板

**What to build:** 在骨架列表上加两个正交维度的切换和底部预览。`p` 在「当前项目 ↔ 全部项目」间切(全部项目视图追加 `cwd` 列),`Tab` 在「活跃 ↔ 归档」间切(严格两态)。底部预览面板显示当前光标行的完整元数据 + `first_user_message` 首段;`Enter` 手动切预览显隐,终端宽度 < 100 时自动隐藏。顶部状态栏显示当前 `scope · lifecycle`。列表为空时按成因显示引导文案。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §4.1–4.3(三维过滤、seam、Scope/Lifecycle 语义)、§4.6(过滤空状态)、§5.1(范式 B、窄屏降级)、§5.2(全部项目列)、§5.4(状态栏)、§8(项目外空列表+提示、不自动回退)。

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] `p` 切 scope:Project ↔ All(两态开关);切换按 §2.5 重查 `all_rows`(scope+lifecycle 决定查库语句)。
- [ ] All 视图在 `updated` 与 `session` 之间追加 `cwd` 列(相对 `~`、中段截断),可省略 `tokens` 列。
- [ ] `Tab` 切 lifecycle:Active(`archived=0`)↔ Archived(`archived=1`),**严格两态、无第三态**;Archived 视图把 `tokens` 位置换成 `archived_at`。
- [ ] 底部横向预览面板:显示光标行完整 `title` / `id`(中段截断两端可辨) / `git_branch` / `model` / `tokens` / `cwd` / `first_user_message` 首段。
- [ ] `Enter`(Normal)切预览显隐;终端列宽 `< 100` 自动隐藏预览(退化纯列表)。
- [ ] 顶部标题栏右侧显示 `scope · lifecycle`(如 `project · active` / `all · archived`)。
- [ ] 空状态按成因分文案:本项目为空 → `no sessions in this project · switch to all projects`(**不自动回退**);归档为空 → `no archived sessions in this scope · switch lifecycle back to active`。
- [ ] scope/lifecycle 切换时光标/滚动状态处理合理(不越界)。
