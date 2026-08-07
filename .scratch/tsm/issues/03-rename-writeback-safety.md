# R3 — 改名写库(UPDATE threads.title)安全性

Type: research
Status: resolved
Blocked by: —

## Question

事实已查证:`traex` 顶层**无** `rename` 命令,改名只能进会话 `/rename`。因此 tsm 改名唯一可行路径是直接写 `state_5.sqlite` 的 `threads.title`。这是本 map 里**唯一的写库操作**,必须查清安全性:

1. **写 `threads.title` 够不够**:标题除了 `threads` 表还存在别处吗?—— rollout `.jsonl` 头(charting 已知 `session_meta` 里**没有** title)、`session_index.jsonl`(其字段叫 `thread_name`)、`logs_2.sqlite`?若 `/rename` 会同时更新多处,tsm 只写 `threads.title` 会不会导致 `resume` 选择器显示不一致?
2. **traex 会不会覆盖/缓存**:traex 运行中是否把 title 缓存在内存,退出时回写从而覆盖 tsm 的修改?tsm 改名时如果该会话正被 resume 打开会怎样?
3. **WAL 下写并发**:tsm 以读写打开 WAL 库、执行一条 `UPDATE` 时,若 traex 同时在写会不会 `SQLITE_BUSY`?需要 `busy_timeout`?建议的 rusqlite 打开与写入参数。
4. **`updated_at` 影响**:改 title 要不要同时更新 `updated_at`/`updated_at_ms`?不更新会不会让排序/索引诡异?traex 自己 `/rename` 时更不更新?
5. **回退方案**:如果直接写库被判定为不安全/不可靠,有没有别的路子(比如 `traex resume <id>` 后自动发 `/rename`?—— 但这违背"不启动 resume"的 OOS)。给出明确的 go/no-go 建议。

## 交付

在 `## Answer` 下记录:title 的所有存储位置、只写 `threads.title` 是否足够或需同步哪些、traex 覆盖风险、WAL 写参数、`updated_at` 处理建议、以及"改名写库"的 go/no-go 结论。**实验在 throwaway 会话上做并先备份 `state_5.sqlite`。**

## Answer

**结论:GO** —— 直接写 `threads.title` 改名是安全的。实测方法:机械写测试只在 copy 上做;覆盖测试只在自造的一次性会话 `019fdbac-…` 的那一行上做,已 `traex delete --force` 清理,`session_index.jsonl` 事后与快照 byte-identical。完整实录见 `../research/R3-findings.md`。

**Q1 — title 存储位置**:
| 位置 | 存 title? | 说明 |
|---|---|---|
| `threads.title`(state_5) | **是,权威** | resume 选择器 `SELECT ... threads.title` 并用 `instr(threads.title,…)` 搜索 |
| rollout `.jsonl` `session_meta` | 否 | 头里没有 title/thread_name(已 grep 全文=0) |
| `session_index.jsonl` | 是,但仅改过名的会话 | 字段叫 `thread_name`;是 `/rename` 的 sidecar,选择器**不读它** |
| `logs_2.sqlite` | 否 | 无 title 列 |

从二进制字符串确认 traex 自己的 `/rename` 执行两步:`UPDATE threads SET title = ? WHERE id = ?`(**只写 title,不动时间戳**)+ 写 `session_index.jsonl`。**只写 `threads.title` 对选择器已足够**(它只读也只搜 `threads.title`)。

**Q2 — traex 会不会覆盖/缓存**:**不会**。实测:直接写的 title `TSM_SENTINEL_RENAME` **熬过了完整 resume+turn**(只 bump 了 `updated_at`)。又造了 sidecar 冲突(`session_index` 写 OLD_INDEX_NAME、DB 写 NEW_TITLE_v2),resume 后 **`threads.title` 胜出**且 sidecar 未被回写。traex 不把 title 缓存在内存 flush-on-exit,也不把 sidecar 读回 title。**残留风险**仅剩:同一会话正被 traex 打开、且用户在 tsm 写之后又跑 `/rename`/auto-name → 普通 last-writer-wins,概率低。

**Q3 — WAL 写并发**:copy 上实证 `busy_timeout=0` 时并发写立即 `database is locked`;设 5s 时阻塞 ~2.93s 后成功。推荐写连接:
```rust
let conn = Connection::open_with_flags(
    &state_db_path,
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI, // 不加 _CREATE,库缺失绝不新建
)?;
conn.busy_timeout(Duration::from_millis(5000))?; // 显式设,rusqlite 默认 5s 文档标注"可能变"
let n = conn.execute("UPDATE threads SET title = ?1 WHERE id = ?2",
                     params![new_title, id])?; // n==1 成功;n==0 = id 已被别处删
```
不要改 `journal_mode`(已是持久 WAL);单条 autocommit UPDATE 是极小事务,无需显式 `BEGIN IMMEDIATE`。超时后仍 `SQLITE_BUSY`(罕见)→ 提示"库忙,请重试"别崩。

**Q4 — updated_at 处理**:trigger 只在 `updated_at`(秒)变化时同步 ms。traex `/rename` **只写 title、不动时间戳**。**建议:改名只写 title,不 bump 时间戳**(与 traex 一致,排序稳定;改名是元数据非活动)。**绝不能单独写 `updated_at_ms`**(会 秒/毫秒 skew)。

**GO 的必备条件(供 spec / T-rename 落地)**:
1. 只写 `title` 列(与 traex `/rename` SQL 逐字节一致);title 是 NOT NULL,空写 `''` 不写 NULL。
2. RW 打开 `SQLITE_OPEN_READ_WRITE`(**不带 `_CREATE`**),写前 `busy_timeout(5000)`;不改 journal_mode。
3. 不 bump 时间戳;若将来要"改名置顶"只 bump `updated_at`(秒)让 trigger 同步 ms,**绝不单写 ms**。
4. rowcount:1=成功、0=id 已被别处删 → 优雅提示;超时后 `SQLITE_BUSY` → 提示重试不崩。
5. `session_index.jsonl` 同步**非必需**(选择器不读它);可选:v1 可跳过,若同步则按 traex schema 逐字段匹配、last-write-wins。
6. 校验(空/超长/换行归 T-rename)—— DB 接受任意 TEXT,title 会显示在选择器,故**去掉换行**。
