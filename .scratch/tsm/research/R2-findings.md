# R2 findings — 变更命令(delete/archive/unarchive)确切行为

Research method: /research discipline — primary-source observation of real `traex` behavior on
**throwaway sessions I created in an isolated temp cwd** (`/tmp/tsm-r2-scratch`). No pre-existing
session was mutated. traex version `traecli 0.200.19 (internal edition)`, binary
`/Users/bytedance/.local/bin/traex`, on macOS. Date 2026-08-07.

## Safety baseline (read-only, before any experiment)

```
$ sqlite3 -readonly ~/.trae/cli/state_5.sqlite "SELECT count(*) FROM threads;"
35
$ sqlite3 -readonly ~/.trae/cli/state_5.sqlite "SELECT archived, count(*) FROM threads GROUP BY archived;"
0|35                       # all 35 pre-existing sessions were archived=0
$ cp ~/.trae/cli/session_index.jsonl /tmp/tsm-R2-session_index.bak    # 5 lines
$ sqlite3 -readonly ~/.trae/cli/goals_1.sqlite "SELECT count(*) FROM thread_goals;"
3
$ sqlite3 -readonly ~/.trae/cli/logs_2.sqlite "SELECT count(*) FROM logs;"
341868
$ find ~/.trae/cli/sessions -name 'rollout-*.jsonl' | wc -l
35
# saved all 35 pre-existing ids to /tmp/tsm-R2-protected-ids.txt for a post-run integrity check
```

Stores of interest under `~/.trae/cli/`:
- `state_5.sqlite` — `threads` table (list source of truth). WAL mode.
- `sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` + optional sibling `...<uuid>.artifacts/` dir.
- `session_index.jsonl` — a **separate, small** recent-exec index (only 5 lines vs 35 threads;
  it is NOT the session list and does not track every session).
- `archived_sessions/` — destination dir that `archive` moves rollout files into.
- `goals_1.sqlite` (`thread_goals`), `logs_2.sqlite` (`logs`, has a `thread_id` column).

Command help (all three take exactly one `<SESSION_ID>` positional, described as "Session id (UUID)";
only `delete` has a `--force` flag, help text "Delete without prompting"):
```
Usage: traecli delete    [OPTIONS] <SESSION_ID>   (has --force)
Usage: traecli archive   [OPTIONS] <SESSION_ID>
Usage: traecli unarchive [OPTIONS] <SESSION_ID>
```

Throwaway sessions were created non-interactively with:
`(cd /tmp/tsm-r2-scratch/sN && traex exec --skip-git-repo-check --sandbox read-only "reply ok")`.
Note: `exec --session-id <x>` does **NOT** set the session id — it sets the session *title*; the id is
always an auto-generated UUIDv7. Real ids were read back from `threads` filtered by the scratch cwd.

---

## Q1 — `traex delete <id> [--force]`: what it mutates + exit codes

### Success (existing session, with `--force`)
```
$ traex delete 019fdba3-ca1b-7550-bc96-73e25229c254 --force ; echo $?
Deleted session 019fdba3-ca1b-7550-bc96-73e25229c254.      # <-- STDOUT, one line
0                                                          # stderr empty
```
Before/after diff for that session (id `019fdba3-...c254`):

| Store | Before | After | Changed by delete? |
|---|---|---|---|
| `threads` row | present (total 36) | gone (total 35) | **YES — row DELETEd** |
| rollout `.jsonl` (`sessions/2026/08/07/rollout-...c254.jsonl`) | exists | gone | **YES — file removed** |
| `.artifacts` dir (`...c254.artifacts/`) | exists (empty) | gone | **YES — dir removed** |
| `session_index.jsonl` | 6 lines (my line present) | 5 lines (my line gone) | **YES — line removed** |
| `goals_1.thread_goals` | 3 | 3 | no change (my session had 0 goal rows; see caveat) |
| `logs_2.logs` | 341868 | 342861 | **NOT delete** — grew only from the live parent session's ambient logging; my throwaway had 0 rows with its `thread_id` both before and after |

Caveat: my exec throwaway had **0** rows in `thread_goals` and **0** in `logs` for its `thread_id`, so I
could not positively observe whether delete *cascades* goals/log rows for a session that has them. What
is proven: delete does not touch *unrelated* goal/log rows, and the goals total was unchanged.

The `sessions/2026/08/07/` day-dir itself was **left in place** (still holds other sessions); only the
target session's own files were removed. No orphan day-dir cleanup observed (dir wasn't empty anyway).

### Failure — already-deleted id, and random non-existent id (both with `--force`)
```
$ traex delete 019fdba3-ca1b-7550-bc96-73e25229c254 --force ; echo $?   # re-delete
Error: thread 019fdba3-ca1b-7550-bc96-73e25229c254 not found            # <-- STDERR
1
$ traex delete 00000000-0000-0000-0000-000000000000 --force ; echo $?   # never existed
Error: thread 00000000-0000-0000-0000-000000000000 not found            # <-- STDERR
1
```
Exit 1, stdout empty, one `Error: thread <uuid> not found` line on stderr. Same behavior for
already-deleted and never-existed — indistinguishable.

### `--force` semantics (critical for tsm)
Without `--force`, and without an interactive TTY (piped stdin), delete **refuses** and does nothing:
```
$ traex delete <uuid> < /dev/null ; echo $?
Error: cannot confirm session deletion without an interactive terminal; rerun with --force and a session UUID
1
# session verified still present afterwards
```
So: interactively (real TTY) delete prompts for confirmation; non-interactively it aborts unless
`--force`. **tsm runs traex as a child process (no TTY on that child), so tsm MUST pass `--force`.**

---

## Q2 — `traex archive <id>` / `traex unarchive <id>`

`archive` is **more than a flag flip** — it also physically **moves** the rollout files.

### archive success
```
$ traex archive 019fdba5-e439-7d33-a80b-154d2cf021a8 ; echo $?
Archived session 019fdba5-e439-7d33-a80b-154d2cf021a8.     # STDOUT, one line
0
```
Mutations observed:

| Store | Before archive | After archive |
|---|---|---|
| `threads.archived` | 0 | **1** |
| `threads.archived_at` | NULL | **<epoch secs>** (e.g. 1786096554) |
| `threads.rollout_path` | `.../sessions/2026/08/07/rollout-...jsonl` | **`.../archived_sessions/rollout-...jsonl`** (rewritten) |
| rollout `.jsonl` + `.artifacts/` | in `sessions/2026/08/07/` | **moved to `~/.trae/cli/archived_sessions/`** |
| `session_index.jsonl` | (session not in it) | unchanged |

### archive is NOT idempotent (re-archiving an archived session errors)
```
$ traex archive 019fdba5-e439-7d33-a80b-154d2cf021a8 ; echo $?     # already archived
Error: invalid thread-store request: no rollout found for thread id 019fdba5-e439-7d33-a80b-154d2cf021a8
1
# DB row unchanged (archived stays 1) — the error is because the .jsonl already left sessions/
```

### unarchive success (mirror of archive)
```
$ traex unarchive 019fdba5-e439-7d33-a80b-154d2cf021a8 ; echo $?
Unarchived session 019fdba5-e439-7d33-a80b-154d2cf021a8.   # STDOUT, one line
0
```
Effects: `archived` -> 0, `archived_at` -> NULL, `rollout_path` -> back under `sessions/...`, and the
`.jsonl` + `.artifacts/` are **moved back** out of `archived_sessions/`.

### unarchive is NOT idempotent (unarchiving an active session errors)
```
$ traex unarchive 019fdba5-e439-7d33-a80b-154d2cf021a8 ; echo $?    # already active
Error: invalid thread-store request: no archived rollout found for thread id 019fdba5-e439-7d33-a80b-154d2cf021a8
1
# DB unchanged (archived stays 0)
```

Implication for tsm: guard state before calling — only offer "archive" on active sessions and
"unarchive" on archived ones. Re-issuing the same op is a hard error (exit 1), not a no-op.

---

## Q3 — timing / concurrency

`state_5.sqlite` is **WAL** journal mode:
```
$ sqlite3 -readonly ~/.trae/cli/state_5.sqlite "PRAGMA journal_mode;"
wal
```

### 6 deletes in parallel, on 6 distinct throwaway ids, while the live parent session was
### concurrently writing to the same state_5.sqlite:
```
# each launched with:  /usr/bin/time -p traex delete <uuid> --force  &   ... ; wait
PROC1..PROC6 all: "Deleted session <uuid>."   EXIT=0
per-proc real: 0.07 – 0.09 s
NO "database is locked" / SQLITE_BUSY errors on any process.
```
All six succeeded, zero lock contention. WAL + tiny per-delete write transactions make concurrent
deletes safe even alongside an active traex writer.

### single-command wall time is dominated by process startup, not DB work
Observed wall times for one invocation ranged **~0.08 s to ~2.0 s** (high variance):
- fast path (warm, minimal startup): ~0.08–0.10 s
- slow path (config load / hooks / startup network advisory): ~1.8–2.0 s
The actual sqlite mutation is sub-millisecond; the cost is spawning a full `traex` process each time.

**Concurrency verdict: safe. Recommended max concurrency for tsm = 4** (a small pool). Rationale:
DB locking is a non-issue (WAL), so the only reason to cap is to avoid spawning dozens of full traex
processes at once (memory/CPU/fd churn); 4 gives good throughput with low footprint. Going to 6+ also
worked here without errors, so 4 is a conservative default, not a hard ceiling.
Progress-feedback granularity: budget **up to ~2 s per session** worst-case, so show per-item progress
(N/total) rather than assuming instant completion.

---

## Q4 — stdout / stderr shape (for tsm parsing)

| Outcome | exit | STDOUT | STDERR |
|---|---|---|---|
| delete OK | 0 | `Deleted session <uuid>.` | (empty) |
| archive OK | 0 | `Archived session <uuid>.` | (empty) |
| unarchive OK | 0 | `Unarchived session <uuid>.` | (empty) |
| any failure | 1 | (empty) | one line `Error: <message>` |

Guidance: **branch on exit code** (0 = success). On failure, surface the stderr line (already
human-readable, `Error:`-prefixed). Success stdout is a fixed confirmation string; tsm can ignore it and
just refresh from the DB.

---

## Q5 — id form: full UUID required, prefix rejected

```
$ traex delete 019fdba5-e439 --force ; echo $?          # 12-char prefix of a real id
Error: --force requires a session UUID
1                                                       # session NOT deleted
$ traex archive 019fdba5-e439 ; echo $?
Error: invalid session id: '019fdba5-e439' (must be a UUID)
1
$ traex delete not-a-uuid --force ; echo $?
Error: --force requires a session UUID
1
```
**Full canonical UUID is mandatory.** Prefixes and non-UUID strings are rejected before any lookup
(exit 1, nothing mutated). tsm already has the full UUID from the `threads.id` column, so pass it
verbatim — no prefix expansion needed or supported.

---

## Exit-code matrix (summary)

| Command / condition | exit | stream | message |
|---|---|---|---|
| `delete <uuid> --force` (exists) | 0 | stdout | `Deleted session <uuid>.` |
| `delete <uuid> --force` (missing / already-deleted) | 1 | stderr | `Error: thread <uuid> not found` |
| `delete <uuid>` (no `--force`, no TTY) | 1 | stderr | `Error: cannot confirm session deletion without an interactive terminal; rerun with --force and a session UUID` |
| `delete <prefix-or-garbage> --force` | 1 | stderr | `Error: --force requires a session UUID` |
| `archive <uuid>` (active) | 0 | stdout | `Archived session <uuid>.` |
| `archive <uuid>` (already archived) | 1 | stderr | `Error: invalid thread-store request: no rollout found for thread id <uuid>` |
| `archive <prefix>` | 1 | stderr | `Error: invalid session id: '<x>' (must be a UUID)` |
| `unarchive <uuid>` (archived) | 0 | stdout | `Unarchived session <uuid>.` |
| `unarchive <uuid>` (already active) | 1 | stderr | `Error: invalid thread-store request: no archived rollout found for thread id <uuid>` |

## Which store each command mutates (summary)

| | `threads` row | `threads.archived`/`archived_at` | `threads.rollout_path` | rollout `.jsonl` + `.artifacts/` | `session_index.jsonl` | `goals_1` | `logs_2` |
|---|---|---|---|---|---|---|---|
| **delete** | DELETE row | (n/a) | (row gone) | **removed** | line removed (if present) | not touched* | not touched* |
| **archive** | keep | `archived=1`, `archived_at=now` | rewritten to `archived_sessions/…` | **moved** to `~/.trae/cli/archived_sessions/` | unchanged | not touched | not touched |
| **unarchive** | keep | `archived=0`, `archived_at=NULL` | rewritten back to `sessions/…` | **moved back** to `sessions/YYYY/MM/DD/` | unchanged | not touched | not touched |

\* delete: proven not to touch *unrelated* goal/log rows; whether it cascades a session's *own* goal/log
rows was untestable (throwaway had none). Low-risk for tsm since tsm only reads `threads`.

---

## Cleanup / integrity (post-run)

```
$ sqlite3 -readonly ~/.trae/cli/state_5.sqlite "SELECT count(*) FROM threads;"
35                                            # back to baseline
$ sqlite3 -readonly ~/.trae/cli/state_5.sqlite "SELECT id,cwd FROM threads WHERE cwd LIKE '%tsm-r2-scratch%';"
(empty)                                       # all 7 of my throwaway sessions gone
# integrity loop over /tmp/tsm-R2-protected-ids.txt:  PROTECTED present=35 missing=0
$ diff (sorted session_index.jsonl) (sorted /tmp/tsm-R2-session_index.bak)  -> IDENTICAL
$ ls ~/.trae/cli/archived_sessions/           # empty (traex-created dir, left in place)
$ rm -rf /tmp/tsm-r2-scratch                  # my scratch working dir removed
```
Created 7 throwaway sessions total (1 solo + 6 batch); deleted all 7. All 35 pre-existing sessions
intact; `session_index.jsonl` byte-identical to the pre-run backup. The live parent session was never
touched. traex created one new empty dir `~/.trae/cli/archived_sessions/` as a side effect of the
archive test (left in place — it is infra, not a session).
