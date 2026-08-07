# R3 findings — 改名写库(UPDATE threads.title)安全性

- Ticket: `.scratch/tsm/issues/03-rename-writeback-safety.md`
- Investigated: 2026-08-07 (Asia/Shanghai)
- Environment: macOS aarch64; `traex` (traecli) `0.200.19` (Mach-O arm64, `/Users/bytedance/.local/bin/traex`); system `sqlite3` `3.51.0`; rusqlite docs `libsqlite3-sys 0.38.x`. A live traex session was actively writing to `~/.trae/cli/state_5.sqlite` throughout (it went 34→35 during the run from concurrent user sessions).
- Method / safety:
  - MECHANICAL write tests ran ONLY on a copy: `cp ~/.trae/cli/state_5.sqlite{,-wal,-shm} /tmp/tsm-r3-copy.sqlite*`. The live DB was never mutated by the mechanical tests.
  - Q2 overwrite test used ONE throwaway session I created (`traex exec`), UUID `019fdbac-6293-7f62-a1c6-55ee8dded56b`, cwd `/tmp/tsm-r3-scratch`. Only THAT row was ever UPDATEd in the live DB; no pre-existing row was touched, and the DB was never restored/overwritten from a backup.
  - Throwaway session deleted at the end via `traex delete <uuid> --force`; `session_index.jsonl` verified byte-identical to a pre-test snapshot afterward.
  - All inspection of pre-existing data used `sqlite3 -readonly` or read-only file reads.

---

## Q1 — Where does the title live, and is writing `threads.title` alone enough?

### threads schema (from COPY)

```
sqlite3 /tmp/tsm-r3-copy.sqlite ".schema threads"
```

Relevant columns:

```
title TEXT NOT NULL,                 -- the authoritative display title
first_user_message TEXT NOT NULL DEFAULT '',
updated_at INTEGER NOT NULL,         -- unix SECONDS
updated_at_ms INTEGER,               -- unix MILLIS (nullable, kept in sync by triggers)
archived INTEGER NOT NULL DEFAULT 0,
...
```

`title` is `NOT NULL` with no default → an empty title must be written as `''`, never NULL.

### Title storage locations — audited

| Location | Holds the title? | Evidence |
|---|---|---|
| `threads.title` (state_5.sqlite) | YES — authoritative | schema above; picker SELECTs it (below) |
| rollout `.jsonl` line-1 `session_meta` | NO | `head -1 <rollout>` shows keys `id/timestamp/cwd/originator/cli_version/source/thread_source/model_provider/base_instructions` — no `title`/`thread_name`. `grep -cE '"thread_name"|"title"' <rollout>` = `0` across the whole file. |
| `session_index.jsonl` | YES — but only for renamed sessions | field is `thread_name` (not `title`) |
| `logs_2.sqlite` | NO | tables are only `logs` + `_sqlx_migrations`; no title/thread_name column (the single "name" hit was system `sqlite_sequence`). |

### session_index.jsonl is a rename-only sidecar

The live file had 5 lines; the DB had 34 threads. Cross-referencing every indexed id against `threads.title` on the COPY:

```
019fd635… thread_name = threads.title = 增加调整编制 PTableDOM e2e 用例
019fd678… thread_name = threads.title = 调整编制 ptable dom 滚动加载用例 - 接续
019fd7b0… thread_name = threads.title = 编制趋势需求 - case-001 - slice-001
019fd80b… thread_name = threads.title = 生成飞书文档介绍
019fda52… thread_name = threads.title = momo 开发耗时久分析
```

Every `thread_name` exactly equals that row's `threads.title`. Non-indexed sessions have `title == first_user_message` (the auto-title). Conclusion: `session_index.jsonl` records only sessions the user explicitly renamed via `/rename`; struct is `SessionIndexEntry { id, thread_name, updated_at }` (binary string `codex-rs/rollout/src/session_index.rs`).

### What traex's own `/rename` writes (from binary strings)

Source file `codex-rs/thread-store/src/local/update_thread_metadata.rs` emits two ordered steps:

```
failed to set thread name:      # step 1 → UPDATE threads SET title = ? WHERE id = ?
failed to index thread name:    # step 2 → append/replace entry in session_index.jsonl
```

So `/rename` writes BOTH `threads.title` AND the `session_index.jsonl` sidecar. The exact SQL for step 1 is present verbatim in the binary:

```
UPDATE threads SET title = ? WHERE id = ?
```

Note this statement sets **title only** — it does NOT touch `updated_at`/`updated_at_ms` (there is a separate `UPDATE threads SET updated_at = ?, updated_at_ms = ? WHERE id = ?` used elsewhere).

### What the resume picker reads

The picker's list query (binary, `codex-rs/traex-tui/src/resume_picker.rs`) selects from `threads` and displays/searches `threads.title`:

```
SELECT threads.id, threads.rollout_path, threads.created_at_ms AS created_at,
       threads.updated_at_ms AS updated_at, ..., threads.cwd, threads.cli_version,
       threads.title, ..., threads.first_user_message, ...
FROM threads
WHERE 1 = 1 AND threads.archived = 0 [AND threads.cwd IN (...)] ...
  AND instr(threads.title, <query>) > 0
ORDER BY threads.updated_at_ms | threads.created_at_ms ...
```

The picker reads `threads.title` and even runs its text search over `threads.title` (`instr(threads.title, …)`). It does NOT read `session_index.jsonl`. So the picker shows whatever is in `threads.title`.

### Answer to Q1

Writing **`threads.title` alone is sufficient for the resume picker** — it is the single authoritative source the picker reads and searches. `session_index.jsonl` is a rename-only sidecar keyed to `/rename`; the picker never reads it, and (proven in Q2) neither does resume. See Q2 for whether tsm should still sync it for cosmetic consistency (recommended-optional, not required).

---

## Q2 — Does traex overwrite / cache the title? (live throwaway test)

### Setup

```
cd /tmp/tsm-r3-scratch
traex exec --skip-git-repo-check -s read-only "reply with exactly the word DONE"
  → session id: 019fdbac-6293-7f62-a1c6-55ee8dded56b
  → threads.title = "reply with exactly the word DONE and nothing else" (auto-title)
  → NOT in session_index.jsonl (never renamed)
```

### Test A — direct title write survives a resume+turn

```
# single UPDATE of MY row only, RW open + busy_timeout (rusqlite-equivalent via python sqlite3)
UPDATE threads SET title='TSM_SENTINEL_RENAME' WHERE id='019fdbac-…'   # rowcount=1, 0.00s
# resume + run a real turn:
traex exec resume --skip-git-repo-check -c 'sandbox_mode="read-only"' 019fdbac-… "reply with exactly OK"   # exit 0
```

Result (read-only check after the turn):

```
title = TSM_SENTINEL_RENAME    updated_at bumped 1786096944 → 1786097046
```

The externally-written title **survived** a full resume + turn. traex only bumped `updated_at`; it did NOT reset/cache/overwrite `title`.

### Test B — divergent sidecar does NOT win over threads.title

To rule out a `session_index.jsonl` read-back on session start (the `session_init.thread_name_lookup` step seen in the binary), I made the two sources disagree:

```
# inject a stale sidecar entry for MY row:
echo '{"id":"019fdbac-…","thread_name":"OLD_INDEX_NAME","updated_at":"2026-08-07T09:00:00.000000Z"}' >> session_index.jsonl
# set a DIFFERENT DB title:
UPDATE threads SET title='TSM_NEW_TITLE_v2' WHERE id='019fdbac-…'
# resume again:
traex exec resume --skip-git-repo-check -c 'sandbox_mode="read-only"' 019fdbac-… "reply with exactly OK2"   # exit 0
```

Result:

```
threads.title = TSM_NEW_TITLE_v2      # DB title WON
session_index.jsonl my line          = still "OLD_INDEX_NAME"  (traex did NOT rewrite it)
```

Even with a conflicting sidecar, `threads.title` won and was not clobbered; traex did not rewrite the sidecar on resume either. This confirms `thread_name_lookup` is not a write-back-to-title path for an existing thread.

### Risk if the session is open in `resume` while tsm renames it

- The one place a title change could be lost is if a traex process performs its OWN `UPDATE threads SET title=?` (i.e., the user runs `/rename`, or an auto-name/`ThreadNameGenerated` event fires) AFTER tsm's write. That is a genuine last-writer-wins race, but it requires traex to actively rename in that window — normal resume/turns do NOT rewrite title (proven above).
- Auto-naming (`ThreadNameGenerated`, config `tui.auto_thread_name_from_mr`) applies to fresh/unnamed threads; it is not observed to overwrite an already-set title on resume in these tests.
- Practical guidance: tsm renaming a session that is NOT currently open is fully safe. Renaming one that IS open in traex carries a small race only if that traex instance itself issues a rename/auto-name afterward; the value is not cached-and-flushed-on-exit in a way that clobbers tsm's write during ordinary use.

### Answer to Q2

No overwrite/cache risk under normal resume. traex reads `threads.title` live from the DB; it does not cache it in memory and flush-on-exit, and it does not read `session_index.jsonl` back into `threads.title`. Residual risk is only the ordinary last-writer-wins case if traex itself renames the same row after tsm.

---

## Q3 — WAL write concurrency (rusqlite open params + busy_timeout)

### The DB is WAL; default busy_timeout on a bare connection is 0

```
sqlite3 /tmp/tsm-r3-copy.sqlite "PRAGMA journal_mode;"   → wal
sqlite3 /tmp/tsm-r3-copy.sqlite "PRAGMA busy_timeout;"   → 0
```

### Demonstrated SQLITE_BUSY vs. busy_timeout (on COPY)

Holder connection took a write lock (`BEGIN IMMEDIATE; UPDATE …`) and slept 4s; a second connection then tried to write:

```
[contender, busy_timeout=0 ]  FAILED after 0.01s: "database is locked"        (SQLITE_BUSY)
[contender, busy_timeout=5s]  SUCCEEDED after waiting 2.93s                    (blocked, then wrote)
```

This is the canonical WAL single-writer contention: with `busy_timeout=0` a concurrent writer errors immediately; with a busy timeout it transparently waits for the other writer to release, then commits.

### Primary sources

- SQLite WAL doc (`https://www.sqlite.org/wal.html`, §2.2 Concurrency): *"Writers merely append new content to the end of the WAL file. Because writers do nothing that would interfere with the actions of readers, writers and readers can run at the same time. However, since there is only one WAL file, there can only be one writer at a time."* §9 also lists rare `SQLITE_BUSY` cases in WAL mode (a connection in exclusive-locking mode, last-connection cleanup, crash recovery) — "applications should be prepared for that happenstance."
- rusqlite `Connection::busy_timeout(&self, timeout: Duration) -> Result<()>` (`https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html#method.busy_timeout`): *"Set a busy handler that sleeps for a specified amount of time when a table is locked. The handler will sleep multiple times until at least 'ms' milliseconds of sleeping have accumulated."* Also: *"Newly created connections currently have a default busy timeout of 5000ms, but this may be subject to change."* Setting `PRAGMA busy_timeout=N` (or `busy_timeout()`) installs/replaces the busy handler.

Important nuance: rusqlite's default 5000ms busy timeout is documented as "subject to change" — tsm should set it explicitly rather than rely on the default.

### Recommended rusqlite open + write params for tsm's single UPDATE

```rust
use rusqlite::{Connection, OpenFlags};
use std::time::Duration;

// Read-WRITE for rename (list/read path uses SQLITE_OPEN_READ_ONLY separately per R1).
let conn = Connection::open_with_flags(
    &state_db_path,
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    // NOTE: do NOT pass SQLITE_OPEN_CREATE — never create the DB if missing.
)?;

// Explicit busy timeout so concurrent traex writers don't cause SQLITE_BUSY.
conn.busy_timeout(Duration::from_millis(5000))?;   // 3000–5000ms is plenty; test needed ~3s max

// Do NOT set PRAGMA journal_mode here — the DB is already WAL and mode is persistent;
// tsm must not attempt to change it. Just write:
let n = conn.execute(
    "UPDATE threads SET title = ?1 WHERE id = ?2",
    rusqlite::params![new_title, id],
)?;   // n == 1 on success; n == 0 means id not found (session deleted elsewhere)
```

Notes:
- WAL requires local filesystem + write access to the dir/`-shm` (tsm only manages the local default traex home, so fine).
- A single autocommit `UPDATE` is a tiny write transaction — ideal for WAL. No explicit `BEGIN IMMEDIATE` needed for one statement.
- On `Err(SQLITE_BUSY)` after the timeout (rare), surface a retry/"库忙,请重试" message rather than crashing (feeds T-rename Q3).

---

## Q4 — updated_at / updated_at_ms handling

### Trigger facts (from schema on COPY)

```
CREATE TRIGGER threads_updated_at_ms_after_update
AFTER UPDATE OF updated_at ON threads
WHEN NEW.updated_at != OLD.updated_at AND NEW.updated_at_ms IS OLD.updated_at_ms
BEGIN
    UPDATE threads SET updated_at_ms = NEW.updated_at * 1000 WHERE id = NEW.id;
END;
```

(plus the symmetric `created_at_ms` trigger and `after_insert` variants.)

### Tested on COPY

| Test | Statement | updated_at | updated_at_ms | Note |
|---|---|---|---|---|
| A: title only | `SET title=?` | unchanged | unchanged | matches traex's own `/rename` SQL — title-only write does NOT touch timestamps |
| B: seconds only | `SET updated_at=1900000000` | 1900000000 | 1900000000000 | trigger auto-syncs ms = s×1000 |
| C: both explicit | `SET updated_at=?, updated_at_ms=?` | 1786100000 | 1786100000123 | explicit ms kept; trigger does NOT override (guard `NEW.updated_at_ms IS OLD.updated_at_ms` fails) |
| D: ms only | `SET updated_at_ms=1786100999000` | old | 1786100999000 | no trigger on `updated_at_ms` column → **seconds/millis SKEW** |

### What traex's own /rename does

traex's rename statement is `UPDATE threads SET title = ? WHERE id = ?` — **title only, no timestamp bump**. So the faithful, traex-consistent behavior is to NOT touch `updated_at`/`updated_at_ms` on rename. (The separate `UPDATE threads SET updated_at=?, updated_at_ms=?` statement is a general "touch" used by other flows, e.g. unarchive, not by rename.)

### What breaks if you don't bump updated_at

- Nothing breaks in the picker/index. The `idx_threads_updated_at_ms` index simply doesn't move the row; its list position (sorted by `updated_at_ms`) stays put, which is arguably correct — a rename is metadata, not activity. This matches traex's own `/rename`, which leaves the row's sort position alone.

### Recommendation for Q4

- Default: **write `title` only; do NOT touch `updated_at`/`updated_at_ms`.** This mirrors traex's own `/rename` and keeps sort order stable.
- If tsm ever DOES want the rename to bubble the row to the top, bump the **seconds** column and let the trigger sync ms: `SET title=?, updated_at=?` (seconds) — but only if `updated_at_ms` is left unset in the same statement so the trigger fires. NEVER write `updated_at_ms` alone (Test D → skew). Simplest safe combo if bumping: set both explicitly and consistently (`updated_at = now_s, updated_at_ms = now_s*1000`).

---

## GO / NO-GO

```
┌───────────────────────────────────────────────────────────────────────────┐
│  VERDICT: GO — rename by writing threads.title directly is SAFE.            │
│                                                                             │
│  Required conditions:                                                       │
│   1. Write column `threads.title` ONLY:                                     │
│        UPDATE threads SET title = ?1 WHERE id = ?2                          │
│      (byte-identical to traex's own /rename SQL). title is NOT NULL →       │
│      write '' for empty, never NULL.                                        │
│   2. Open RW with OpenFlags::SQLITE_OPEN_READ_WRITE (NO _CREATE);           │
│      set conn.busy_timeout(Duration::from_millis(5000)) BEFORE the write.   │
│      Do NOT change journal_mode (already WAL, persistent).                  │
│   3. Do NOT bump updated_at/updated_at_ms (matches traex /rename; keeps     │
│      sort stable). If you ever bump, set updated_at (seconds) and let the   │
│      trigger sync ms — never write updated_at_ms alone (causes skew).       │
│   4. Treat execute() rowcount: 1 = ok, 0 = id gone (deleted elsewhere) →    │
│      report gracefully. On SQLITE_BUSY after timeout, show retry, no crash. │
│                                                                             │
│  session_index.jsonl sync: NOT required for correctness (resume picker      │
│  reads threads.title only, and resume does not read the sidecar back).      │
│  OPTIONAL nicety: also upsert the {id,thread_name,updated_at} line so a     │
│  future traex feature that trusts the sidecar stays consistent. If synced,  │
│  match traex's schema exactly and keep last-write-wins semantics. v1 may    │
│  safely skip it.                                                            │
│                                                                             │
│  Residual risks to verify during implementation:                           │
│   • Last-writer-wins if the SAME session is open in traex and the user      │
│     runs /rename (or auto-name fires) AFTER tsm writes. Not cached-flush;   │
│     only an active traex rename in that window can clobber. Low risk.       │
│   • rusqlite default busy_timeout (5000ms) is documented "subject to        │
│     change" → set it explicitly.                                            │
│   • Rare WAL SQLITE_BUSY (exclusive-lock / cleanup / recovery, per SQLite   │
│     wal.html §9) → busy_timeout + graceful retry covers it.                 │
│   • Validation (empty/too-long/newlines) is a T-rename concern; DB accepts  │
│     any TEXT, and title is shown in the traex picker, so strip newlines.    │
└───────────────────────────────────────────────────────────────────────────┘
```

## Appendix — cleanup / non-mutation confirmation

- Throwaway session `019fdbac-6293-7f62-a1c6-55ee8dded56b` created by me, then deleted via `traex delete … --force` (exit 0, "Deleted session …"). Its `threads` row is gone (`COUNT WHERE id=…` = 0) and its rollout `.jsonl` was removed. `traex delete` also removed the sidecar `session_index.jsonl` entry (corroborates R2: delete cleans the name index).
- `session_index.jsonl` verified byte-identical to the pre-test snapshot (`diff` = IDENTICAL); my injected `OLD_INDEX_NAME` line is gone.
- The live `~/.trae/cli/state_5.sqlite` was NEVER restored/overwritten from any backup, and the ONLY live-DB writes I performed were single-row UPDATEs of my own throwaway row. No pre-existing row was ever mutated.
