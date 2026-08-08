---
name: implement
description: "Implement a piece of work based on a spec or set of tickets."
disable-model-invocation: true
---

Implement the work described by the user in the spec or tickets.

Use /tdd where possible, at pre-agreed seams.

Run typechecking regularly, single test files regularly, and the full test suite once at the end.

Once done, use /code-review to review the work.

Commit your work to the current branch.

## Close out the ticket

If the work came from a ticket on the issue tracker (not a bare spec), record its completion **after committing** — the frontier rule depends on it, and a finished ticket left as `ready-for-agent` will be re-implemented by the next session.

For the local-markdown tracker (see `docs/agents/issue-tracker.md`):

- Set the ticket's `Status:` line to `done` (the completion state defined in `docs/agents/triage-labels.md`).
- Check each acceptance box `- [ ]` → `- [x]`, one at a time, **verifying each against the code you actually wrote**. Do not bulk-check. If a criterion is only partially met or was verified by mechanism but not by test, check it and add a one-line `<sup>…</sup>` note stating the gap honestly, rather than leaving it unchecked or claiming more than you did.
- Append a short dated entry under a `## Comments` heading: the commit hash and how the work was verified.

On a hosted tracker (GitHub, Linear), express completion the platform's way instead — close the issue or merge the PR — and do not invent a `done` label.
