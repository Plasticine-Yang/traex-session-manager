# Issue tracker: Local Markdown

Issues and specs for this repo live as markdown files in `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The spec is `.scratch/<feature-slug>/spec.md`
- Implementation issues are one file per ticket at `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01` — never a single combined tickets file
- A `Status:` line near the top of each issue file carries its whole lifecycle: `triage → in-progress → done` (see `triage-labels.md` for the strings). Because local markdown has no "assign" or "close/merge" action, `Status:` stands in for both:
  - **Claim before work:** set `Status: in-progress` and save *before* starting, so a second session doesn't grab the same ticket.
  - **Complete after commit:** set `Status: done (<sha>)` with the closing commit SHA on the same line, e.g. `Status: done (a1b2c3d)` — the local stand-in for a merged-PR link.
- Comments and conversation history append to the bottom of the file under a `## Comments` heading

## Scanning for unfinished issues

To see the lifecycle state of every ticket at a glance:

```
rg -n '^\*{0,2}Status:' .scratch/**/issues/*.md
```

The `\*{0,2}` matches both the bare `Status:` line (wayfinding tickets) and the `**Status:**` bold form the `/to-tickets` template emits. Anything whose Status is not `done` or `wontfix` is unfinished (i.e. `needs-triage` / `needs-info` / `ready-for-agent` / `ready-for-human` / `in-progress`).

## When a skill says "publish to the issue tracker"

Create a new file under `.scratch/<feature-slug>/` (creating the directory if needed).

## When a skill says "fetch the relevant ticket"

Read the file at the referenced path. The user will normally pass the path or the issue number directly.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a file with one **child** file per ticket.

- **Map**: `.scratch/<effort>/map.md` — the Notes / Decisions-so-far / Fog body.
- **Child ticket**: `.scratch/<effort>/issues/NN-<slug>.md`, numbered from `01`, with the question in the body. A `Type:` line records the ticket type (`research`/`prototype`/`grilling`/`task`); a `Status:` line records `claimed`/`resolved`.
- **Blocking**: a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every file it lists is `resolved`.
- **Frontier**: scan `.scratch/<effort>/issues/` for files that are open, unblocked, and unclaimed; first by number wins.
- **Claim**: set `Status: claimed` and save before any work.
- **Resolve**: append the answer under an `## Answer` heading, set `Status: resolved`, then append a context pointer (gist + link) to the map's Decisions-so-far in `map.md`.
