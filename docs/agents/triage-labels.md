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

## Completion state (local addition)

The five roles above are **triage** vocabulary — they describe an issue *before* implementation starts. They deliberately have no "finished" state: on a hosted tracker (GitHub, Linear) completion is expressed by **closing the issue or merging the PR**, not by a label.

This repo's tracker is **local markdown** (see `issue-tracker.md`), which has no "close" action, so a completed ticket needs an explicit written signal. We use one extra `Status:` string:

| Status | Meaning                                                                                                |
| ------ | ------------------------------------------------------------------------------------------------------ |
| `done` | Implementation is complete, verified, and committed. All acceptance `- [ ]` boxes are checked `- [x]`. |

`done` is what the frontier rule keys on: a ticket becomes workable once every ticket in its "Blocked by" list is `done`. It is written on ticket close-out — see the "Closing out a ticket after `/implement`" section in `AGENTS.md`.
