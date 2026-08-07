# R1 findings — sqlite 读取侧事实

- Ticket: `.scratch/tsm/issues/01-sqlite-read-facts.md`
- Investigated: 2026-08-07 (Asia/Shanghai)
- Environment: macOS aarch64; `traex` (traecli) `0.200.19`; system `sqlite3` `3.51.0`; live traex session was actively writing to `~/.trae/cli/state_5.sqlite` throughout.
- Method: READ-ONLY only. Every DB read used `sqlite3 -readonly` or `file:...?mode=ro`. No traex mutation (`delete`/`archive`/`unarchive`/`rename`) was run. `traex doctor` runs were pointed at throwaway `$TMPDIR` homes so their writable-probe never touched `~/.trae`.

> Live-mutation proof: during the session `SELECT count(*) FROM threads` went 35 → 41 → 34 across reads, and an `archived=1` row flipped back to `archived=0` between two queries. Read-only connections observed these changes with zero lock errors — direct evidence WAL read concurrency works (see Q4).

---

## Q1 — Path resolution (does traex honor `TRAE_HOME` / `CODEX_HOME`?)

### Commands run

```
which traex                       # /Users/bytedance/.local/bin/traex
printenv TRAE_HOME CODEX_HOME     # both empty on this machine
strings -a "$(which traex)" | grep -iE 'TRAE_HOME|TRAECLI_HOME|codex_home'
```

Empirical precedence test using `traex doctor` (prints resolved `TRAE_HOME` / `TRAECLI_HOME` / `log_dir`), with env vars pointed at temp dirs:

```
# Case: no override (canonical live values)
traex doctor
  TRAE_HOME:      /Users/bytedance/.trae
  TRAECLI_HOME:  /Users/bytedance/.trae/cli
  log_dir:        /Users/bytedance/.trae/cli/log

# Case: TRAE_HOME=$B only
TRAE_HOME=$B traex doctor
  TRAE_HOME:      $B
  TRAECLI_HOME:  $B/cli          # cli dir = $TRAE_HOME/cli

# Case: TRAECLI_HOME=$A only
TRAECLI_HOME=$A traex doctor
  TRAE_HOME:      /Users/bytedance/.trae   # unchanged
  TRAECLI_HOME:  $A                         # cli dir overridden directly

# Case: BOTH TRAE_HOME=$B and TRAECLI_HOME=$A
TRAE_HOME=$B TRAECLI_HOME=$A traex doctor
  TRAE_HOME:      $B
  TRAECLI_HOME:  $A              # TRAECLI_HOME wins for the cli dir

# Case: CODEX_HOME=$C only  ->  IGNORED
CODEX_HOME=$C traex doctor
  TRAE_HOME:      /Users/bytedance/.trae
  TRAECLI_HOME:  /Users/bytedance/.trae/cli

# Case: CODEX_HOME=$C + TRAE_HOME=$B  ->  TRAE_HOME wins, CODEX_HOME ignored
```

### Corroborating binary strings

```
return os.environ.get("TRAE_HOME", os.path.expanduser("~/.trae"))     # skill installer helper
goal_home="${TRAE_HOME:-$HOME/.trae}/cli"; ...                        # goals backup/migration shell
"failed to resolve TRAE_HOME"                                          # runtime error string
doctor labels: TRAE_HOME / TRAECLI_HOME / log dir / sqlite dir
```

`CODEX_HOME` appears in the binary only as a serialized JSON field name (`codexHome`, an app-server compatibility alias for `TRAE_HOME`) — never as an env var consulted for path resolution. The env-probe table traex itself checks lists `TRAE_HOME` and `TRAECLI_HOME` (and proxy/token vars); `CODEX_HOME` is absent.

### Answer

- The runtime dir traex reads/writes its SQLite DBs from is what doctor calls **`TRAECLI_HOME`** (the `~/.trae/cli` directory).
- Resolution order traex uses:
  1. `TRAECLI_HOME` env var, if set → used **directly** as the cli dir.
  2. else `TRAE_HOME` env var, if set → cli dir = `$TRAE_HOME/cli`.
  3. else default: `~/.trae/cli` (i.e. `$HOME/.trae/cli`).
- **`CODEX_HOME` is NOT honored** by this traex build (`0.200.19`). Ignore it.
- Default is always `~/.trae/cli/`. `state_5.sqlite` lives directly in that dir.

### Recommended rule for `tsm`

Resolve the cli dir in this order (mirror traex exactly so tsm always points at the same DB traex uses):

1. explicit tsm flag / config (e.g. `--db` or `--trae-cli-dir`) — highest priority, tsm-local.
2. `$TRAECLI_HOME` if non-empty → use as cli dir.
3. else `$TRAECLI_HOME` unset but `$TRAE_HOME` non-empty → `$TRAE_HOME/cli`.
4. else `$HOME/.trae/cli`.

Do NOT consult `CODEX_HOME`. (Optional nicety: if you want forward-compat, you may treat `CODEX_HOME` as a last-resort fallback below `TRAE_HOME`, but current traex ignores it, so matching traex means leaving it out.)

---

## Q2 — Versioned filename (`state_5`): schema version? glob-max or hardcode?

### Commands run

```
ls ~/.trae/cli | grep -E '^(state|logs|goals)_[0-9]+\.sqlite$'
# goals_1.sqlite  logs_2.sqlite  state_5.sqlite

sqlite3 -readonly ~/.trae/cli/state_5.sqlite \
  "SELECT max(version), count(*) FROM _sqlx_migrations;"   # 34 | 34
sqlite3 -readonly ~/.trae/cli/logs_2.sqlite  "SELECT max(version),count(*) FROM _sqlx_migrations;"  # 2 | 2
sqlite3 -readonly ~/.trae/cli/goals_1.sqlite "SELECT max(version),count(*) FROM _sqlx_migrations;"  # 2 | 2

strings -a "$(which traex)" | grep -E 'state_5\.sqlite|logs_2\.sqlite|goals_1\.sqlite'
```

### Key evidence

- Each DB file carries its OWN independent `_sqlx_migrations` table:
  - `state_5.sqlite` → 34 migrations (versions 1..34), all `success=1`.
  - `logs_2.sqlite` → 2 migrations.
  - `goals_1.sqlite` → 2 migrations.
- The trailing number is therefore **NOT** the sqlx migration count/version (state is at migration 34, not 5).
- The filenames are **hardcoded string literals** in the traex binary, in `codex-rs/state/src/runtime.rs`: the strings `state_5.sqlite`, `logs_2.sqlite`, `goals_1.sqlite` appear verbatim next to each other. There is no `state_%d` / format-string construction and no runtime "pick highest N" logic.

### Interpretation

The `_N` is a **database-generation / file-format epoch** baked into the traex build: a monotonic integer traex bumps when it wants a clean new physical DB file (e.g. an incompatible restructuring or a "start fresh, abandon the old file" migration) rather than migrating in place. Within a generation, ordinary schema evolution happens via `_sqlx_migrations` (state is on gen 5, migration 34). A future traex could ship `state_6.sqlite` and leave the old `state_5.sqlite` on disk.

### Recommendation for `tsm`

- Do **not** hardcode `state_5.sqlite` as the only accepted name (brittle across traex upgrades), and do **not** naively "glob `state_*.sqlite`, take max N" as the sole strategy either — if traex bumps to `state_6`, taking max blindly could point at a brand-new empty/differently-shaped DB before you've validated its schema.
- Recommended resolution for the DB file inside the cli dir:
  1. If a tsm config/flag names the file, use it.
  2. Else glob `state_*.sqlite` (regex `^state_(\d+)\.sqlite$`), and among matches pick the **highest N** — but then **validate** before trusting it: confirm the `threads` table exists and has the columns tsm needs (`id, cwd, title, first_user_message, updated_at, updated_at_ms, archived, rollout_path`). If validation fails, surface a clear "unsupported traex state DB version" error rather than crashing on a missing column.
  3. If exactly one `state_*.sqlite` exists (today's case), use it.
- Treat `state_5` as the *known-good* generation to develop/test against; the glob-max-with-validation keeps tsm working if traex advances to `state_6`, while the schema validation is the real guard against a shape change. `_sqlx_migrations.max(version)` can be logged for diagnostics but is not needed for file selection.

---

## Q3 — `threads` schema / column semantics

### Commands run

```
sqlite3 -readonly ~/.trae/cli/state_5.sqlite ".schema threads"
sqlite3 -readonly ~/.trae/cli/state_5.sqlite "SELECT archived, count(*) FROM threads GROUP BY archived;"
sqlite3 -readonly ~/.trae/cli/state_5.sqlite \
  "SELECT datetime(updated_at,'unixepoch','localtime'),
          datetime(updated_at_ms/1000,'unixepoch','localtime'),
          updated_at_ms - updated_at*1000 FROM threads ORDER BY updated_at_ms DESC LIMIT 5;"
sqlite3 -readonly ~/.trae/cli/state_5.sqlite \
  "SELECT (title='') , (first_user_message='') , count(*) FROM threads GROUP BY 1,2;"
sqlite3 -readonly ~/.trae/cli/state_5.sqlite \
  "SELECT count(*), sum(cwd LIKE '/%'), sum(rollout_path LIKE '/%') FROM threads;"
```

### Full CREATE TABLE (verbatim)

```sql
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    rollout_path TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    source TEXT NOT NULL,
    model_provider TEXT NOT NULL,
    cwd TEXT NOT NULL,
    title TEXT NOT NULL,
    sandbox_policy TEXT NOT NULL,
    approval_mode TEXT NOT NULL,
    tokens_used INTEGER NOT NULL DEFAULT 0,
    has_user_event INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    archived_at INTEGER,
    git_sha TEXT,
    git_branch TEXT,
    git_origin_url TEXT
, cli_version TEXT NOT NULL DEFAULT '', first_user_message TEXT NOT NULL DEFAULT '',
  agent_nickname TEXT, agent_role TEXT, memory_mode TEXT NOT NULL DEFAULT 'enabled',
  model TEXT, reasoning_effort TEXT, agent_path TEXT,
  created_at_ms INTEGER, updated_at_ms INTEGER, thread_source TEXT);
```

Relevant indexes (good news for tsm's queries):

```
idx_threads_updated_at              (updated_at DESC, id DESC)
idx_threads_updated_at_ms           (updated_at_ms DESC, id DESC)
idx_threads_archived                (archived)
idx_threads_archived_cwd_updated_at_ms  (archived, cwd, updated_at_ms DESC, id DESC)
idx_threads_archived_cwd_created_at_ms  (archived, cwd, created_at_ms DESC, id DESC)
```

There are triggers that auto-populate `created_at_ms` / `updated_at_ms` from the seconds columns on insert/update when the `_ms` value is NULL, so the `_ms` columns are kept in sync with the seconds columns by traex itself.

### Column semantics table (verified against real rows)

| Column | Type / range observed | Meaning | Notes for tsm |
|---|---|---|---|
| `id` | TEXT, UUIDv7 (e.g. `019fdba1-b420-7802-b9f2-0cd971c3ad84`) | Session id, PRIMARY KEY | Unique (count = distinct). The 8-char time-prefix repeats across sessions started the same instant — always compare full UUID, never a prefix. This is the arg for `traex delete/archive/unarchive`. |
| `rollout_path` | TEXT NOT NULL | Path to the rollout `.jsonl` | **Always absolute** (41/41 start with `/`), e.g. `/Users/bytedance/.trae/cli/sessions/2026/08/07/rollout-<ts>-<uuid>.jsonl`. Never empty in samples. |
| `cwd` | TEXT NOT NULL | Working dir when session was created ("project") | **Always absolute** (41/41). Never empty in samples. Exact-match this against tsm's launch dir for "current project" filtering. Note macOS `/tmp` shows as `/private/tmp` (symlink-resolved) in some rows — tsm should canonicalize its own cwd the same way before comparing. |
| `title` | TEXT NOT NULL DEFAULT '' | User-facing thread title (usually derived from first user message) | Can be `''`. Observed: 38 rows title+fum both present; 1 row title present, fum empty; 2 rows both empty. Empty-title rows all had `has_user_event=0` (session with no captured user message yet). |
| `first_user_message` | TEXT NOT NULL DEFAULT '' | The first user message text | Can be `''`. In samples, whenever `title` was empty `first_user_message` was empty too (no observed case of fum present but title empty). For search, LIKE over BOTH columns. |
| `updated_at` | INTEGER NOT NULL, **epoch SECONDS** | Last-updated time (seconds) | `datetime(updated_at,'unixepoch')` gives correct wall-clock. Reliable, always set. Has a dedicated index. |
| `updated_at_ms` | INTEGER, **epoch MILLISECONDS** | Same event, millisecond resolution | `updated_at_ms - updated_at*1000` was 0..999 ms in every sampled row → it is just the finer-grained version of `updated_at`. Populated by trigger; nominally nullable but present on all live rows. Also indexed. |
| `created_at` / `created_at_ms` | INTEGER | Creation time, seconds / millis | Same seconds-vs-millis relationship as updated_at. |
| `archived` | INTEGER NOT NULL DEFAULT 0 | Archive flag | Strictly 0 or 1 (GROUP BY showed only those two). `0` = active, `1` = archived. Indexed (both alone and composite with cwd). |
| `archived_at` | INTEGER, nullable | Epoch seconds when archived | NULL when `archived=0`. |
| `git_branch` | TEXT nullable | Branch at session start | e.g. `main`. `git_sha`, `git_origin_url` also present/nullable. |
| `model` | TEXT nullable | Model used | e.g. `openrouter-3o`. |
| `tokens_used` | INTEGER NOT NULL DEFAULT 0 | Cumulative tokens | Present; map.md lists it for display only. |
| `source` / `thread_source` | TEXT | Origin classification | Observed `source='cli'`, `thread_source='user'`. The traex `resume` picker filters to interactive sources by default; tsm may want to mirror that if it wants parity, but it is not required for a metadata list. |

### Answers to the specific sub-questions

- `archived` is a strict `0/1` boolean. `1` = archived.
- `updated_at` = **epoch seconds** (reliable, indexed, always set). `updated_at_ms` = **epoch milliseconds** (same instant, finer resolution, trigger-maintained). Either works for sort; `updated_at_ms` gives a stable tie-break at sub-second granularity and has its own index. Both are safe; prefer `updated_at_ms DESC` (matches traex's own newest-first index) or `updated_at DESC`.
- `title` vs `first_user_message`: both `NOT NULL DEFAULT ''`; both can be empty (e.g. a session with no user turn yet → `has_user_event=0`). They are usually equal/similar (title derived from the first message). For a robust list label use `COALESCE(NULLIF(title,''), NULLIF(first_user_message,''), '(untitled)')`; for search, LIKE across both.
- `cwd`: **always absolute** in observed data (NOT NULL, 41/41 absolute). Canonicalize tsm's own launch dir (resolve symlinks, e.g. `/tmp`→`/private/tmp`) before exact-matching.
- `rollout_path`: **absolute** (NOT NULL, 41/41 absolute, points into `~/.trae/cli/sessions/YYYY/MM/DD/...jsonl`).

---

## Q4 — WAL read concurrency (safe to open read-only while traex writes?)

### Commands run

```
sqlite3 -readonly ~/.trae/cli/state_5.sqlite "PRAGMA journal_mode; PRAGMA query_only;"   # wal | 0

# 5 rapid read-only opens during live writes — timing + any lock error?
for i in 1..5; do /usr/bin/time -p sqlite3 -readonly state_5.sqlite "SELECT count(*) FROM threads"; done
# all 5: real 0.00, no SQLITE_BUSY, no lock error

# Freshness: does mode=ro see new WAL commits?
sqlite3 -readonly ... "SELECT max(updated_at_ms) FROM threads"   # t0: 1786096653984
sleep 4; ... same query ...                                       # t1: 1786096657248  (advanced)

# busy_timeout honored on read-only handle?
sqlite3 -readonly ... "PRAGMA busy_timeout=3000; PRAGMA busy_timeout;"   # 3000

# -readonly does not touch -wal/-shm mtime (before == after a SELECT) — confirmed
```

### SQLite docs (primary source: https://www.sqlite.org/wal.html)

- Concurrency: *"WAL provides more concurrency as readers do not block writers and a writer does not block readers. Reading and writing can proceed concurrently."*
- Snapshot isolation: *"the end mark is unchanged for the duration of the transaction, thus ensuring that a single read transaction only sees the database content as it existed at a single point in time."* → a read txn sees a consistent snapshot; new writes appear on the next read txn (matches the t0→t1 observation).
- Read-only access requirement (this matters): opening a WAL DB requires being able to use the `-shm`/`-wal` files. Since **SQLite 3.22.0 (2018)**: *"a read-only WAL-mode database file can be opened if the -shm and -wal files already exist or those files can be created or the database is immutable."* On this machine `-wal`/`-shm` exist and are readable, and the cli dir is writable, so a normal read-only open works (and system sqlite is 3.51.0).
- All processes must be on the same host (WAL uses shared memory) — fine, tsm is local-only (map.md scopes out remote).
- `SQLITE_BUSY` can still occur in narrow cases: another connection in **exclusive locking mode**; the moment the *last* connection closes and cleans up WAL/shm; or during crash recovery. So tsm must still handle `SQLITE_BUSY` (set a busy_timeout).

### `immutable=1` (primary source: https://www.sqlite.org/c3ref/open.html)

*"The immutable parameter … indicates that the database file is stored on read-only media. When immutable is set, SQLite assumes that the database file cannot be changed … all locking and change detection is disabled. Caution: Setting the immutable property on a database file that does in fact change can result in incorrect query results and/or SQLITE_CORRUPT errors."*

→ **Do NOT use `immutable=1` for tsm.** The DB is being actively written by traex; immutable would disable change detection and risk wrong results / `SQLITE_CORRUPT`. (It "worked" in a one-shot test only because the file didn't happen to change during that single statement — it is unsafe for a running TUI.) `mode=ro` is the correct choice: it reads through the WAL, sees committed writes, and stays safe.

`nolock=1` is also wrong here (docs: corruption risk if any writer uses nolock). Don't use it.

### Recommended rusqlite open (primary source: docs.rs/rusqlite)

Default `OpenFlags` is `SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_URI | SQLITE_OPEN_NO_MUTEX`. For a read-only viewer use `open_with_flags` with READ_ONLY (drop CREATE/READ_WRITE):

```rust
use rusqlite::{Connection, OpenFlags};
use std::time::Duration;

let flags = OpenFlags::SQLITE_OPEN_READ_ONLY   // never create, never write
          | OpenFlags::SQLITE_OPEN_URI          // allow file: URIs if you use them
          | OpenFlags::SQLITE_OPEN_NO_MUTEX;     // single-threaded handle (rusqlite default)
let conn = Connection::open_with_flags(&db_path, flags)?;

// Handle transient SQLITE_BUSY (last-connection cleanup / recovery windows).
conn.busy_timeout(Duration::from_millis(3000))?;   // wraps sqlite3_busy_timeout

// Belt-and-suspenders: forbid any accidental write statement on this handle.
conn.pragma_update(None, "query_only", true)?;     // PRAGMA query_only = ON
```

Notes / tradeoffs:
- `SQLITE_OPEN_READ_ONLY` is the equivalent of URI `mode=ro`; both mean "open read-only". Passing the flag directly is cleaner than encoding `?mode=ro` in a URI. If you prefer the URI form, keep `SQLITE_OPEN_URI` and open `file:/abs/path/state_5.sqlite?mode=ro`.
- rusqlite gives new connections a **default 5000 ms busy_timeout** already; setting it explicitly (or lowering to ~3000 ms so the TUI never hangs long) documents intent and survives future default changes. If you'd rather fail fast and show "DB busy, retry", set a small timeout or handle the `SQLITE_BUSY` result code.
- `query_only=ON` is a cheap extra guard; combined with READ_ONLY it makes it impossible for tsm's read module to mutate the DB even by mistake. (tsm's rename feature — the one intentional write per map.md — must use a *separate* read-write connection, not this one.)
- Do **not** run `PRAGMA journal_mode` changes or checkpoints from tsm; a read-only handle can't anyway. Long-lived tsm read transactions can starve traex's checkpointer (WAL grows), so keep read transactions short — open, query, finish; don't hold a transaction open across UI idle time. For a TUI that refreshes the list, run each refresh as its own short query rather than one long-lived snapshot.
- WAL is local-host only; fine for tsm (no remote/network-FS support needed).

---

## Q5 — Example SQL (all run READ-ONLY against the live DB; confirmed working)

All executed via `sqlite3 -readonly ~/.trae/cli/state_5.sqlite` (and one via `file:...?mode=ro`); all returned sane results. Use bound parameters (`?1`, `?2`) in tsm — shown here with literals for the verification runs.

### Q5.1 Filter by exact cwd (current project), active only, newest first — CONFIRMED
```sql
SELECT id, title, first_user_message, updated_at, updated_at_ms, model, git_branch, tokens_used
FROM threads
WHERE cwd = ?1            -- e.g. '/Users/bytedance/code/projects/traex-session-manager'
  AND archived = 0
ORDER BY updated_at_ms DESC;
```
Returned the 5 sessions for this project, newest first.

### Q5.2 Filter by archived — CONFIRMED
```sql
-- archived sessions:
SELECT id, title, archived_at, cwd
FROM threads
WHERE archived = 1
ORDER BY updated_at_ms DESC;

-- active (non-archived), used for the default list:
--   WHERE archived = 0
```
(During the run the single archived row got un-archived by the live traex, so this returned 0 rows at that instant — the query itself is correct; it's just live data.)

### Q5.3 LIKE search over title + first_user_message (case-insensitive) — CONFIRMED
```sql
SELECT id, title, first_user_message, updated_at_ms
FROM threads
WHERE (title LIKE ?1 COLLATE NOCASE
    OR first_user_message LIKE ?1 COLLATE NOCASE)
  AND archived = 0
ORDER BY updated_at_ms DESC;
-- bind ?1 = '%research%'  (wrap the user term in % yourself)
```
Returned the matching "…RESEARCH ticket…" sessions. `COLLATE NOCASE` gives ASCII case-insensitivity; escape user-supplied `%`/`_` with an `ESCAPE` clause if you don't want them treated as wildcards.

### Q5.4 Order by updated_at desc (full/all-projects list) — CONFIRMED
```sql
SELECT id, cwd, title, first_user_message, updated_at, updated_at_ms, archived, model, git_branch, tokens_used
FROM threads
WHERE archived = 0
ORDER BY updated_at_ms DESC          -- or: ORDER BY updated_at DESC
LIMIT ?1 OFFSET ?2;                  -- optional paging
```
Also verified via URI form `file:/Users/bytedance/.trae/cli/state_5.sqlite?mode=ro`. Newest-first, spanning multiple `cwd`s (this is the `--all` view; add a `cwd` column since projects differ).

Notes:
- Every filter/sort column above is backed by an index (`idx_threads_archived_cwd_updated_at_ms`, `idx_threads_updated_at_ms`, etc.), so these stay fast.
- Prefer `updated_at_ms DESC` for ordering to match traex's own newest-first index and get sub-second tie-breaking; `updated_at DESC` is equally valid.
- Always use bound parameters (rusqlite `?1`, `?2`) — never string-format user input into SQL.
