# 02 — Scope + Lifecycle 切换 + 预览面板

**What to build:** 在骨架列表上加两个正交维度的切换和底部预览。`p` 在「当前项目 ↔ 全部项目」间切(全部项目视图追加 `cwd` 列),`Tab` 在「活跃 ↔ 归档」间切(严格两态)。底部预览面板显示当前光标行的完整元数据 + `first_user_message` 首段;`Enter` 手动切预览显隐,终端宽度 < 100 时自动隐藏。顶部状态栏显示当前 `scope · lifecycle`。列表为空时按成因显示引导文案。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §4.1–4.3(三维过滤、seam、Scope/Lifecycle 语义)、§4.6(过滤空状态)、§5.1(范式 B、窄屏降级)、§5.2(全部项目列)、§5.4(状态栏)、§8(项目外空列表+提示、不自动回退)。

**Blocked by:** 01

**Status:** done (5f189fb)

- [x] `p` 切 scope:Project ↔ All(两态开关);切换按 §2.5 重查 `all_rows`(scope+lifecycle 决定查库语句)。
- [x] All 视图在 `updated` 与 `session` 之间追加 `cwd` 列(相对 `~`、中段截断),可省略 `tokens` 列。
- [x] `Tab` 切 lifecycle:Active(`archived=0`)↔ Archived(`archived=1`),**严格两态、无第三态**;Archived 视图把 `tokens` 位置换成 `archived_at`。
- [x] 底部横向预览面板:显示光标行完整 `title` / `id`(中段截断两端可辨) / `git_branch` / `model` / `tokens` / `cwd` / `first_user_message` 首段。<sup>预览布局经 UI 单测门(`empty_text_by_reason`)+ 手工阅读渲染代码验证;终端渲染像素级样子未做快照测试(与 spec §13 附录一致:交互样子的 primary source 是原型,不做截图)。</sup>
- [x] `Enter`(Normal)切预览显隐;终端列宽 `< 100` 自动隐藏预览(退化纯列表)。<sup>`toggle_preview` 意图有单测;`preview_visible` 的 `< 100` 阈值门是纯函数但由渲染路径消费,未单独断言。</sup>
- [x] 顶部标题栏右侧显示 `scope · lifecycle`(如 `project · active` / `all · archived`)。
- [x] 空状态按成因分文案:本项目为空 → `no sessions in this project · switch to all projects`(**不自动回退**);归档为空 → `no archived sessions in this scope · switch lifecycle back to active`。<sup>`EmptyReason::NoSessions`(All·Active 全空)spec §4.6 未定文案,取 `no sessions` 兜底。</sup>
- [x] scope/lifecycle 切换时光标/滚动状态处理合理(不越界)。

## Comments

### 2026-08-08 — implemented

照 spec §4.1–4.3/§4.6/§5.1/§5.2/§5.4/§8 实现完成,commit `5f189fb`。

- **store**:把 `query_project_active` 泛化成 `query(scope_cwd, archived)`,四个 scope×lifecycle 组合共用一条绑定参数 SELECT(共享列投影常量 `SELECT_SESSION`),保持每次刷新独立短只读事务(§2.4)。
- **app**:App 收编 Store,新增 `scope`/`lifecycle`/`show_preview`/`home`;`p`/`Tab` 重查并把光标复位到顶(`reset_cursor` 清 offset);`EmptyReason` 按 (scope,lifecycle) 分类空状态成因;busy 重查非致命,保留旧 `all_rows` 并出页脚提示。
- **ui**:All 视图在 `updated`↔`session` 间插入 `~` 相对、中段截断的 `cwd` 列并省 `tokens`;Archived 视图把 `tokens` 位换成 `archived_at`;底部预览面板(`Enter` 切、`<100` 列自动隐);标题栏右显 `scope · lifecycle`。
- **format**:新增 CJK 安全的 `truncate_middle`(两端可辨)与 path-segment 对齐的 `cwd_relative_home`。

**验证**:`cargo test` 40 个单测全绿(store 四组合查询、app 切换/复合/光标复位/空状态成因、format 截断/home 相对),`cargo clippy --all-targets` 无告警。`/code-review` 两轴复查发现一处真问题:页脚 busy 文案原写 "press R to retry" 但 `R`(手动刷新)属 ticket 07 未接线 —— 已改成自包含文案不承诺未接的键;并顺手收敛两处 Duplicated Code(SELECT 列投影、view 重建复用 `rebuild_view`)。TUI 终端逐帧渲染样子未做快照(照 spec §13 附录:交互 primary source 是原型)。
