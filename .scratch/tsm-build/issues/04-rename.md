# 04 — 改名(第一条写路径)

**What to build:** 在列表里直接给会话改名,免去「进会话 → `/rename`」。`r` 在光标行原地弹出单行行内输入框、预填该行原始 `threads.title` 原文;`Enter` 提交、`Esc` 取消。提交时按序清洗校验,直接 `UPDATE threads.title`(tsm 唯一的写库操作),写完刷新列表并把光标按 id 归位。这是第一条打通的写路径,建立 `rename` 模块与独立读写连接。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §2.6(写库参数)、§7.1–7.6(交互/落地/校验/并发/失败刷新/批量排除)、§5.5(`Mode::Rename{buf}`)。写库安全性结论 = R3 GO。

**Blocked by:** 01 (独立于 02/03,可与之并行)

**Status:** done (9ce0a87)

- [x] `r` 在光标行原地弹单行行内输入框(非弹窗),底部提示 `[Enter] save [Esc] cancel`;预填原始 `title` 原文、光标末尾;支持 ←/→、Home/End、Backspace/Delete。
- [x] `title` 为空的会话:输入框预填**空**(所见即 store 真实 title,不填兜底文案)。
- [x] 写库:独立 RW 连接 `SQLITE_OPEN_READ_WRITE|URI`(**不带 `_CREATE`**)+ `busy_timeout(5000)`;`UPDATE threads SET title = ?1 WHERE id = ?2`;**不 bump 时间戳、不改 journal_mode、不同步 `session_index.jsonl`**。
- [x] 提交校验按序:trim 首尾空白 → 换行/Tab 折叠成单空格 → trim 后为空则**拒绝提交**(保留旧 title、提示「标题不能为空」、停编辑态、不写 DB)→ 长度不设上限。
- [x] 成功:重查库刷新 `all_rows`、光标按 session id 停回原行、行尾短暂「已重命名」提示。
- [x] `rowcount==0`(id 已被别处删):提示「会话已不存在,可能已在别处删除」、刷新列表丢弃该行、不崩。
- [x] 超时后仍 `SQLITE_BUSY`:提示「库忙,请重试」、**保留 buffer 停编辑态**、再按 `Enter` 重试、不崩。<sup>通过注入 `SQLITE_BUSY` 等价结果验证编辑态/buffer/光标原样保留与可重复提交;未让测试真实等待 5 秒锁超时。</sup>
- [x] `r` **恒作用光标行、忽略多选**;v1 不提供批量改名。

## Comments

### 2026-08-08 — implemented

按 spec §2.6/§5.5/§7.1–7.6 实现完成,feature commit `9ce0a87`。`cargo test` 95 项全绿,`cargo check --all-targets` 与 `git diff --check` 通过;覆盖原始/空 title 行内编辑、Unicode 光标编辑、只写 title 且时间戳不变、校验拒绝、成功按 id 归位、rowcount=0 刷新移除、busy 保留 buffer 重试和多选排除。两轴 `/code-review` 复查后修复“写成功但刷新失败时成功提示覆盖刷新错误”的消息优先级,最终 Standards/Spec 均无遗留 finding。
