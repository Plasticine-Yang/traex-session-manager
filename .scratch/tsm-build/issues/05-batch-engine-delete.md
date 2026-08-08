# 05 — 批处理引擎 + 删除(单删 + 批删)

**What to build:** 项目的核心痛点。多选一批会话,`d` → 确认模态(大写 `D` 确认)→ 并发删除、阻塞进度模态显示进度 → 全成功自动关闭、有失败转结果面可 `d` 重试。单删(无选中、只删光标行)走**同一模态**(只列 1 条)。删除通过 shell 调 `traex delete <uuid> --force`。建立可被归档复用的统一 `BatchJob` 引擎。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §3(变更契约:`--force`、退出码、输出)、§6.1–6.8(执行形态/引擎/确认/并发/进度/失败重试/刷新/取消)、§5.5(`ConfirmDelete`/`Running`/`Result` 模式)、§5.7(键位)。

引擎形状(来自 spec §6.2,编码了决策):

```rust
struct BatchJob { op: Op, ids: Vec<SessionId> }
enum Op { Delete, Archive, Unarchive }
```

**Blocked by:** 03

**Status:** in-progress

- [ ] 统一 `BatchJob` 引擎:`std::thread` 定长池(4)+ `mpsc`,每 worker `std::process::Command` 跑 `traex <op> <uuid>`,收集退出码 + stderr 行;**不用 tokio**。
- [ ] `d`:有选中删全部选中、否则删光标行 → 确认模态。
- [ ] 确认模态:列将删标题(>10 条则「前 10 条 + 还有 M 条」)+ 数量 + 「不可逆、同时删除 rollout 文件」警告;大写 **`D`** 确认、`Esc`/`n` 取消;单删走同一模态(非 `y`)。
- [ ] delete worker **一律带 `--force`**(无 TTY 否则中止)。
- [ ] 进度模态阻塞:顶部 `Deleting… N/总数` + 进度条,下一行聚合 `✓ x  ✗ y  ⟳ z`;**不逐行渲染**;失败项累积到底部小列表(带 `Error:` 行)。
- [ ] 按退出码判成败:0=成功、非0=失败取 stderr 行;**spawn 失败(如 traex 不在 PATH)也算失败项**。
- [ ] 部分失败 → 转结果面列出失败项 + stderr,失败项**保持选中**、成功项移除;提示「`d` 重试 / `Esc` 关闭」;重试 = 拿仍选中的失败集再走同一流水线。全成功 → 自动关闭(可选 toast `Deleted N.`)。
- [ ] `Esc` 取消:停派发新任务、**不 kill 在飞的 ≤4 个**;随后进结果面,已成功去选、失败+未发起(cancelled)保持选中可重试。
- [ ] 收尾**全量重查库一次**(短只读事务),`selected` 按 id 过滤掉消失/变动行。
