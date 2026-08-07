# R1 — sqlite 读取侧事实

Type: research
Status: resolved
Blocked by: —

## Question

`tsm` 要只读 traex 的 `state_5.sqlite` 里 `threads` 表来列会话。charting 前已知路径为 `~/.trae/cli/state_5.sqlite`,schema 已 dump。需要查清让读取稳健的事实:

1. **路径解析**:traex 如何决定这个目录?是否遵守 `TRAE_HOME` / `CODEX_HOME` 等环境变量?默认是否总是 `~/.trae/cli/`?tsm 应按什么顺序解析(env → 默认)?
2. **版本化文件名**:文件叫 `state_5.sqlite`(带 `_5`),`logs_2.sqlite`、`goals_1.sqlite` 也带数字。这个数字是 schema 版本吗?会随 traex 升级变成 `state_6.sqlite` 吗?tsm 应该 glob `state_*.sqlite` 取最大号,还是硬编码?有没有 `_sqlx_migrations` 表可佐证当前版本?
3. **schema 稳定性 / 关键列**:确认 `threads` 表列语义 —— `archived`(0/1)、`updated_at_ms` vs `updated_at`(哪个是毫秒、哪个可靠)、`title` vs `first_user_message` 的区别与何时为空、`cwd` 是否总是绝对路径、`rollout_path` 是绝对还是相对。
4. **WAL 只读并发安全**:库处于 WAL 模式(有 `-wal`/`-shm`)。tsm 只读打开(`mode=ro` 或 `immutable`?)在 traex 正在写时是否安全、会不会读到脏数据或被锁阻塞?推荐的 rusqlite 打开参数是什么?
5. **过滤所需**:确认按 `cwd` 精确过滤、按 `archived` 过滤、按 `title/first_user_message` 做 LIKE 搜索、按 `updated_at` 排序都可行,并给出示例 SQL。

## 交付

在本文件 `## Answer` 下记录:路径解析规则、版本化文件名策略、关键列语义表、只读打开的确切 rusqlite 参数、以及列表/过滤/搜索/排序的示例 SQL。用真实库验证读取(只读,勿写)。

## Answer

只读实测 live 库(traex `0.200.19`,sqlite `3.51.0`)。完整命令实录见 `../research/R1-findings.md`。**live 变动证据**:读取期间行数 35→41→34、一条 `archived=1` 翻成 `0`,全程零锁错误 —— WAL 只读并发确认可用。

**Q1 — 路径解析**(经 `traex doctor` 指向临时 home 实测 + 二进制字符串佐证):
- **`CODEX_HOME` 不被本 build 认**(doctor 完全忽略)。
- traex 顺序:**`TRAECLI_HOME`**(直接当 cli 目录)→ 否则 **`TRAE_HOME`**(→ `$TRAE_HOME/cli`)→ 否则 **`~/.trae/cli`**。内嵌字符串 `goal_home="${TRAE_HOME:-$HOME/.trae}/cli"` 佐证。
- **tsm 规则**:`--db`/config flag → `$TRAECLI_HOME` → `$TRAE_HOME/cli` → `$HOME/.trae/cli`;忽略 `CODEX_HOME`。

**Q2 — 版本化文件名**:`_5` **不是** sqlx 版本:`state_5` 的 `_sqlx_migrations` 已到版本 **34**;`logs_2`→2 迁移、`goals_1`→2 迁移(各库独立迁移表)。文件名是 `codex-rs/state/src/runtime.rs` 里的**硬编码字面量**(无 `state_%d` 格式化),`_N` 是 traex 另起干净库时 bump 的**数据库世代号**,未来 build 可能出 `state_6`。
- **tsm 策略**:glob `^state_(\d+)\.sqlite$` 取**最大 N**,再**校验** `threads` 含所需列后才信任(既不盲目硬编码,也不盲目取 max);遇未知世代明确报错。

**Q3 — 列语义(实采真实行)**:

| 列 | 含义 | 备注 |
|---|---|---|
| `id` | UUIDv7,主键,唯一 | delete/archive/unarchive 的入参;比对用完整 UUID(8 字符前缀会重复) |
| `cwd` | 项目目录 | **总为绝对路径**(NOT NULL);精确匹配前需 canonicalize tsm 的 cwd(`/tmp`→`/private/tmp`) |
| `title` | 标题 | `NOT NULL DEFAULT ''`;可空(空 ⇔ `has_user_event=0`) |
| `first_user_message` | 首条用户消息 | `NOT NULL DEFAULT ''`;title 空时它也空;搜索对二者做 LIKE |
| `updated_at` | **epoch 秒** | 可靠,有索引 |
| `updated_at_ms` | **epoch 毫秒** | 同一时刻更细,触发器维护,有索引 |
| `archived` | 严格 `0/1` | 1=归档;`archived_at`(秒)仅归档时置 |
| `rollout_path` | rollout `.jsonl` | **总为绝对路径**(NOT NULL) |

另有 `git_branch`、`model`、`tokens_used`。已存在 `updated_at`/`updated_at_ms`/`archived` 及复合 `(archived, cwd, updated_at_ms DESC)` 索引 —— tsm 查询被覆盖。

**Q4 — WAL 只读安全**:安全。SQLite WAL 文档:读不阻塞写、写不阻塞读;读事务见一致快照(新提交下次读可见,已验证)。**用 `mode=ro`,不要 `immutable=1`**(immutable 关闭变更检测 → 对 live 库读到错误结果/`SQLITE_CORRUPT`),不要 `nolock`。推荐 rusqlite 打开:
```rust
let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
          | OpenFlags::SQLITE_OPEN_URI
          | OpenFlags::SQLITE_OPEN_NO_MUTEX;
let conn = Connection::open_with_flags(&db_path, flags)?;
conn.busy_timeout(Duration::from_millis(3000))?;  // 仍需处理瞬时 SQLITE_BUSY
conn.pragma_update(None, "query_only", true)?;     // 额外写保护
```
读事务保持短(长读会饿死 traex 的 checkpointer → WAL 膨胀);每次刷新一条短查询。**改名功能必须用独立的读写连接**(见 R3)。

**Q5 — 示例 SQL(均只读跑通)**:
```sql
-- 当前项目、活跃、最新在前
SELECT id,title,first_user_message,updated_at_ms,model,git_branch
FROM threads WHERE cwd = ?1 AND archived = 0 ORDER BY updated_at_ms DESC;
-- 归档视图
SELECT id,title,archived_at,cwd FROM threads WHERE archived = 1 ORDER BY updated_at_ms DESC;
-- 搜索 title+first_user_message(不区分大小写),?1 = '%term%'
SELECT id,title,first_user_message FROM threads
WHERE (title LIKE ?1 COLLATE NOCASE OR first_user_message LIKE ?1 COLLATE NOCASE)
  AND archived = 0 ORDER BY updated_at_ms DESC;
-- 全部项目
SELECT id,cwd,title,updated_at_ms,archived FROM threads
WHERE archived = 0 ORDER BY updated_at_ms DESC LIMIT ?1 OFFSET ?2;
```
排序统一用 `updated_at_ms DESC`(合 traex 索引 + 亚秒 tie-break);一律用绑定参数。

**对 tsm 的硬约束(供 spec 落地)**:
1. DB 路径解析:`--db` → `$TRAECLI_HOME` → `$TRAE_HOME/cli` → `~/.trae/cli`;忽略 `CODEX_HOME`。
2. 文件名 glob `state_(\d+).sqlite` 取 max + 列校验,别硬编码 `state_5`。
3. 读用 `mode=ro`+`busy_timeout(3s)`+`query_only`;**绝不用 `immutable`**;读事务短。
4. ~~`cwd` 精确匹配前先 canonicalize(macOS `/tmp`→`/private/tmp`、符号链接)。~~ **[T-proj 更正]** 不 canonicalize;两端都用 `std::env::current_dir()`(getcwd)后逐字节比 —— 见文末 Comments。
5. 排序 `updated_at_ms DESC`;搜索对 title+first_user_message 做 `LIKE COLLATE NOCASE`。

## Comments

- **[T-proj 更正 #4]** 硬约束 #4 的"精确匹配前先 canonicalize tsm 的 cwd"已被 [T-proj](08-current-project-semantics.md) 推翻。查清:traex 建会话 cwd = `std::env::current_dir()`(getcwd,物理路径,符号链接已解析),tsm 也用 `current_dir()` 取自身目录,同一目录两端必吐**相同字节** —— canonicalize 多余,还会搅乱 `traex --cd <symlink>` 这种边角情形(那种会话按设计落到"全部项目",不特殊处理)。**改为:只用 `current_dir()` 取目录后逐字节精确比,不 canonicalize、不小写、不碰符号链接。**
