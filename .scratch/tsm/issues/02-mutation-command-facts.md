# R2 — 变更命令(delete/archive/unarchive)确切行为

Type: research
Status: resolved
Blocked by: —

## Question

`tsm` 的删除/归档/取消归档要 shell 调用 `traex`。需要查清这些命令的确切行为,才能设计批量执行与刷新:

1. **`traex delete <id> [--force]` 到底改了什么**:
   - 删 `threads` 表行?删 rollout `.jsonl` 文件?删 `.artifacts` 目录?清 `session_index.jsonl`?动 `logs_2.sqlite` / `goals_1.sqlite`?
   - `--force` 与不带 `--force` 的区别(交互提示?);tsm 批量时是否必须 `--force`。
   - 退出码:成功 0?找不到 id 时?已删除再删时?
2. **`traex archive <id>` / `traex unarchive <id>`**:
   - 只是把 `threads.archived` 置 1/0,还是也移动/改动文件?
   - 退出码与幂等性(重复归档已归档的?)。
3. **时序 / 并发**:多个 `traex delete` 并发跑会不会争抢 sqlite 锁而失败?需要限并发吗?单条命令大概耗时(决定进度反馈粒度)。
4. **stdout/stderr 形态**:成功/失败分别往哪打、格式如何(供 tsm 解析或忽略)。
5. **id 形态**:必须完整 UUID 还是接受前缀?(charting 已知接受完整 UUID。)

## 交付

在 `## Answer` 下记录:三条命令各自改动的 store(库行/文件/目录清单)、退出码矩阵、并发是否安全与建议上限、`--force` 语义。**实验必须在一次性 throwaway 会话上做,并先备份相关文件/库**;绝不能删用户的真实会话。可用 `traex exec` 之类造一个临时会话再删它来观察。

## Answer

在 traex 0.200.19 上以一手实测得出(在隔离 cwd `/tmp/tsm-r2-scratch` 造一次性 `exec` 会话,只删自己造的;先做只读基线,全程不碰 live 父会话)。完整命令实录见 `../research/R2-findings.md`。

**Q1 — `traex delete <uuid> --force` 改动的 store**:删 `threads` 行 + 删 rollout `.jsonl` 文件 + 删同级 `.artifacts/` 目录 + 删 `session_index.jsonl` 中对应行。不动无关的 `goals_1`/`logs_2` 行(throwaway 恰好 0 条,会话自身的 goal/log 级联未能测到 —— 但 tsm 只读 `threads`,风险低)。日期目录保留。

**`--force` 语义(关键约束)**:无 TTY 且不带 `--force` 时,delete 直接中止:`Error: cannot confirm session deletion without an interactive terminal; rerun with --force`。tsm spawn traex 时无 TTY,故 **tsm 必须传 `--force`**。

**Q2 — archive/unarchive 不只翻标志位**:`archive` 置 `threads.archived=1` + 写 `archived_at` + 改写 `threads.rollout_path`,并把 `.jsonl` + `.artifacts/` **物理移动**到 `~/.trae/cli/archived_sessions/`;`unarchive` 全部反向(把文件移回)。**两者都不幂等** —— 重复归档已归档 / 取消归档未归档的,都是硬错误 exit 1(`no rollout found` / `no archived rollout found`)。**tsm 必须做门控:只对活跃的归档、只对归档的取消归档。**

**Q3 — 并发:安全**。`state_5.sqlite` 是 WAL 模式。6 个并发 `delete --force`(且 live 会话同时在写同一库)全部 exit 0,**零锁/SQLITE_BUSY**。单命令墙上耗时被进程启动主导,不是 DB:**~0.08s 到 ~2.0s**(方差大)。**建议并发上限 = 4**(小池,只为限制进程 spawn,不为锁);进度按每条显示,单条预算 ~2s。

**Q4 — 输出形态**:成功 → exit 0,stdout 一行(`Deleted/Archived/Unarchived session <uuid>.`),stderr 空。失败 → exit 1,stdout 空,stderr 一行 `Error: <msg>`。**tsm 应按退出码分支,失败时把 stderr 那行呈现给用户。**

**Q5 — id 形态**:必须完整规范 UUID。前缀/乱码在查库前就被拒(exit 1:delete → `--force requires a session UUID`;archive → `invalid session id: '<x>' (must be a UUID)`)。直接传 `threads.id` 原值。

**对 tsm 的硬约束(供 spec 落地)**:
1. delete 一律带 `--force`(无 TTY 否则中止)。
2. archive/unarchive 前必须按当前 `archived` 状态门控,否则报错。
3. 批量并发上限 4;按退出码判成败,失败提取 stderr 行。
4. 归档不是"改标志",而是**移动文件**到 `archived_sessions/` —— 影响 R1 的 `rollout_path` 语义与刷新逻辑。
