## Agent skills

### Issue tracker

Issues and specs live as markdown files under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five canonical triage roles, each label string equal to its name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Working a ticket: claim and close-out

The `/implement` skill does not touch the ticket's `Status:` line — it ends at "commit". On this repo's local-markdown tracker (no "assign" or "close" action), the `Status:` line is the only lifecycle signal, so a session working a ticket (not a bare spec) drives it by hand. See `docs/agents/triage-labels.md` for the state strings.

- **Claim before starting:** set the ticket's `Status:` to `in-progress` and save *before* any work, so a second session doesn't grab the same ticket.
- **Close out after committing:** set `Status: done (<sha>)` with the closing commit's short SHA on the same line, e.g. `Status: done (a1b2c3d)`.
  - Check each acceptance box `- [ ]` → `- [x]`, one at a time, verifying each against the code actually written. Do not bulk-check. If a criterion is only partially met or was verified by mechanism but not by test, check it and add a one-line `<sup>…</sup>` note stating the gap, rather than leaving it unchecked or claiming more than was done.
  - Append a short dated entry under a `## Comments` heading recording how the work was verified.

Left as `ready-for-agent` (or any non-`done`/`wontfix` state), a finished ticket looks unbuilt and gets re-implemented, and anything blocked by it stalls. This is a repo-level supplement to `/implement`, kept here rather than in the vendored skill so skill updates don't wipe it.
