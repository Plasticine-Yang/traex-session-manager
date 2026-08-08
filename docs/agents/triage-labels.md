# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding label string from this table.

Edit the right-hand column to match whatever vocabulary you actually use.

## Lifecycle states (local-markdown)

The five roles above are **triage** vocabulary — they classify an issue *before* work starts. On a hosted tracker (GitHub, Linear) what happens *after* triage is expressed by the platform: you claim an issue by assigning it, and you finish it by closing the issue / merging its PR. This repo's tracker is **local markdown** (see `issue-tracker.md`), which has none of those actions, so the single `Status:` line carries the whole lifecycle:

```
triage (needs-triage / needs-info / ready-for-agent / ready-for-human) → in-progress → done
```

Two states extend the table beyond triage:

| Status         | Set when                                 | Meaning                                                                                                                             |
| -------------- | ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `in-progress`  | before starting work on the ticket       | Local claim — the ticket is being worked on. Prevents a second session from grabbing the same ticket.                               |
| `done (<sha>)` | after the work is complete and committed | Finished. The closing commit SHA follows on the same line, e.g. `Status: done (a1b2c3d)` — the local stand-in for a merged-PR link. |

`wontfix` (from the triage table) also terminates a ticket.

**Frontier rule:** a ticket becomes workable once every ticket in its "Blocked by" list is `done`. A ticket counts as "unfinished" for any Status other than `done` or `wontfix`. Claiming and completion happen at ticket close-out — see the "Closing out a ticket after `/implement`" section in `AGENTS.md`.
