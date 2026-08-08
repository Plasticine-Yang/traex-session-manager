---
name: release
description: Release tsm through its tagged GitHub workflow, then install and verify the published binary.
disable-model-invocation: true
---

# Release

Publish one SemVer release from the current `main` branch, then update the
machine's installed `tsm` from that release.

## 1. Choose the version

Fetch `origin` and tags. Treat the latest published stable release as the
SemVer baseline.

If the user supplied an exact `X.Y.Z`, use it.

If the user supplied a bump, calculate it from baseline `X.Y.Z`:

- `major` → `(X+1).0.0`
- `minor` → `X.(Y+1).0`
- `patch` → `X.Y.(Z+1)`

Use the resulting exact version.

If the user supplied neither:

1. Inspect every commit and diff from the baseline tag through `HEAD`, plus any
   working-tree changes intended for release.
2. Recommend:
   - `major` for an incompatible user-facing or data-contract change.
   - `minor` for backward-compatible user-facing functionality.
   - `patch` for backward-compatible fixes and maintenance.
3. Show the baseline, recommended bump, exact target version, and the evidence
   that determined it.
4. Ask the user to confirm the exact target or choose another bump. Wait for
   the answer before changing versions, committing, pushing, or tagging.

An explicit version or bump is the user's decision: proceed without asking
them to confirm it again. Stop only when it is invalid, already released,
behind the baseline, or incompatible with repository state.

Completion criterion: one unreleased exact `X.Y.Z` is selected, either
explicitly by the user or confirmed after analysis.

## 2. Prove release readiness

Read the repository release workflow and installer before acting; they are the
source of truth for the tag shape, assets, and installation path.

Account for every local commit and working-tree change. Commit intended product
changes in logical commits before the release commit, following existing
message and attribution conventions. Surface unrelated or ambiguous changes
instead of silently releasing them.

Update `Cargo.toml` and `Cargo.lock` to the selected version. Run:

```sh
cargo fmt --check
cargo test --locked
cargo build --locked --release
./target/release/tsm --version
git diff --check
```

Exercise the release's changed CLI behavior directly when a cheap deterministic
check exists.

Completion criterion: the worktree contains only the intended version change,
all checks pass, and the release binary reports the selected version.

## 3. Publish

Commit the version files with:

```text
chore(release): prepare vX.Y.Z
```

Push `main`, create annotated tag `vX.Y.Z` at the release commit, then push the
tag. Use ordinary pushes; preserve published history.

Find the GitHub Actions run triggered by that exact tag and wait for it with an
exit-status check. A successful build is not yet a successful release: require
the final release job to succeed.

Completion criterion: `main` and `vX.Y.Z` are on `origin`, and the exact tag's
release workflow completed successfully.

## 4. Verify the release

Query the exact GitHub Release. Require it to be published, non-draft,
non-prerelease, and to contain `SHA256SUMS` plus every target archive declared
by the current release workflow.

Completion criterion: the release page and complete asset set exist for the
exact tag.

## 5. Install the published binary

Only after step 4, run the repository installer so the machine receives the
published artifact rather than the local build.

Verify:

1. `command -v tsm` resolves to the expected install location.
2. `tsm --version` reports the selected version.
3. The installed binary's SHA-256 equals the binary unpacked from the matching
   GitHub Release archive for this machine.
4. The repository worktree is clean and `main` matches `origin/main`.

Completion criterion: the installed command is byte-for-byte the published
platform binary and reports the selected version.

## Failure handling

On any failed validation, push, workflow, asset, checksum, or install check,
stop at that boundary and report the exact failing command or job. Keep the
previous installed binary until a complete Release exists. Preserve remote
commits and tags for diagnosis; repair forward unless the user explicitly
chooses another recovery.
