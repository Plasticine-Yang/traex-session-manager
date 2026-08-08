# 05 — 批处理引擎 + 删除(单删 + 批删)

**What to build:** 项目的核心痛点。多选一批会话,`d` → 确认模态(大写 `D` 确认)→ 并发删除、阻塞进度模态显示进度 → 全成功自动关闭、有失败转结果面可 `d` 重试。单删(无选中、只删光标行)走**同一模态**(只列 1 条)。删除通过 shell 调 `traex delete <uuid> --force`。建立可被归档复用的统一 `BatchJob` 引擎。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §3(变更契约:`--force`、退出码、输出)、§6.1–6.8(执行形态/引擎/确认/并发/进度/失败重试/刷新/取消)、§5.5(`ConfirmDelete`/`Running`/`Result` 模式)、§5.7(键位)。

引擎形状(来自 spec §6.2,编码了决策):

```rust
struct BatchJob { op: Op, ids: Vec<SessionId> }
enum Op { Delete, Archive, Unarchive }
```

**Blocked by:** 03

**Status:** done (5a07e55)

- [x] 统一 `BatchJob` 引擎:`std::thread` 定长池(4)+ `mpsc`,每 worker `std::process::Command` 跑 `traex <op> <uuid>`,收集退出码 + stderr 行;**不用 tokio**。
- [x] `d`:有选中删全部选中、否则删光标行 → 确认模态。
- [x] 确认模态:列将删标题(>10 条则「前 10 条 + 还有 M 条」)+ 数量 + 「不可逆、同时删除 rollout 文件」警告;大写 **`D`** 确认、`Esc`/`n` 取消;单删走同一模态(非 `y`)。
- [x] delete worker **一律带 `--force`**(无 TTY 否则中止)。
- [x] 进度模态阻塞:顶部 `Deleting… N/总数` + 进度条,下一行聚合 `✓ x  ✗ y  ⟳ z`;**不逐行渲染**;失败项累积到底部小列表(带 `Error:` 行)。
- [x] 按退出码判成败:0=成功、非0=失败取 stderr 行;**spawn 失败(如 traex 不在 PATH)也算失败项**。
- [x] 部分失败 → 转结果面列出失败项 + stderr,失败项**保持选中**、成功项移除;提示「`d` 重试 / `Esc` 关闭」;重试 = 拿仍选中的失败集再走同一流水线。全成功 → 自动关闭(可选 toast `Deleted N.`)。
- [x] `Esc` 取消:停派发新任务、**不 kill 在飞的 ≤4 个**;随后进结果面,已成功去选、失败+未发起(cancelled)保持选中可重试。<sup>取消后即便在飞的全部成功也进结果面,未发起的 cancelled 尾巴保持选中且 `d` 与失败集一并重试(app.rs `finish_batch`/`retry_failed`/`cancelled_ids`,测试 `cancel_lands_on_result_even_when_all_dispatched_succeed`)。</sup>
- [x] 收尾**全量重查库一次**(短只读事务),`selected` 按 id 过滤掉消失/变动行。<sup>`refresh_after_mutation` 全量重查 + `prune_selection` 按 id 过滤;`traex_runner` 的真实 spawn 未走自动化测试(测试用注入 runner),按 §13 附注人工验收。</sup>

## Comments

### 2026-08-08

照 §6 实现批处理引擎与删除全流程,TDD 建 `mutate` 引擎(池并发上限、取消停派发、spawn 失败归类、退出码判成败),再驱动 `App` 状态机(ConfirmDelete/Running/Result 三模)与 ui.rs 模态。验证:`cargo test`(65 通过)+ `cargo clippy --all-targets`(零告警)。`/code-review` 双轴:standards 仅判断级异味(`impl Op` 三处 match、`(id,error)` 元组),spec 轴发现取消路径缺口——已修:取消后恒进结果面、未发起的 cancelled ids 保持选中并可 `d` 重试(承 §6.8),补 `cancelled_ids()` 与专项测试。`traex` 真实 spawn 属 §13 人工验收范围,未纳入自动化测试。
