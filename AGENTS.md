## Agent skills

### Issue tracker

Issues and specs live as markdown files under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five canonical triage roles, each label string equal to its name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Closing out a ticket after `/implement`

The `/implement` skill ends at "commit" and does not write the ticket's completion state back. On this repo's local-markdown tracker (no "close issue" action), that leaves a finished ticket looking unbuilt, so the next session may re-implement it and any ticket blocked by it stalls.

So when `/implement` finishes work that came from a ticket (not a bare spec), after committing:

- Set the ticket's `Status:` line to `done` (the completion state defined in `docs/agents/triage-labels.md`).
- Check each acceptance box `- [ ]` → `- [x]`, one at a time, verifying each against the code actually written. Do not bulk-check. If a criterion is only partially met or was verified by mechanism but not by test, check it and add a one-line `<sup>…</sup>` note stating the gap, rather than leaving it unchecked or claiming more than was done.
- Append a short dated entry under a `## Comments` heading: the commit hash and how the work was verified.

This is a repo-level supplement to `/implement`, kept here rather than in the vendored skill so skill updates don't wipe it.
