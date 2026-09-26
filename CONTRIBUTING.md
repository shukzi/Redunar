# Developing Redunar

Work from the repository root. Read
[ARCHITECTURE.md](ARCHITECTURE.md) and the affected product guide before editing.
The shared service owns state; UI drafts and native commands retain their existing
boundaries. This workflow changes development practices, not product policy.

## One task, one branch

Keep `main` ready for integration. Start a focused `fix/`, `feat/`, `refactor/`,
`chore/`, or `spike/` branch from current `main`. Use short-lived branches; split
large features into independently reviewable steps. A spike ends with findings
and a separate decision about production work. Do not add a permanent develop
branch. Create a release stabilization branch only when concurrent work needs it.

Before starting, use the [task brief](docs/templates/TASK.md): define the problem,
acceptance criteria, owning components, failure/cleanup cases, and planned
checks. Preserve unrelated changes. Start with one implementation task and one
review; parallel tasks must have independent file ownership and worktrees.

```sh
git status --short
git switch main
git switch -c fix/descriptive-name
```

For concurrent work, from `source/`:

```sh
git worktree add -b fix/descriptive-name ../worktrees/descriptive-name main
```

Run commands from the new worktree. Bootstrap locked dependencies with `npm ci`
in `output/tauri-redunar` and `cargo fetch --locked` for both Cargo manifests
before running the offline checks. These bootstrap commands download dependencies.
Never link another checkout's mutable build outputs into the worktree.

## Build and state isolation

| Output | Checkout-relative location |
| --- | --- |
| Validation targets | `.redunar-build/checks/root` and `checks/tauri` |
| Host native targets | `.redunar-build/native` |
| Debian compatibility targets | `.redunar-build/linux/native` |
| Source snapshot and binary manifests | `.redunar-build/linux` |
| Verification logs | `.redunar-build/reports` |
| Immutable candidate directories | `.redunar-build/candidates/<tag>-<commit>` |

The compatibility builder copies Git-visible working files into a temporary
snapshot, including uncommitted edits and non-ignored new files. It excludes
ignored caches/private files, refuses source symlinks, and never deletes host
Cargo targets. Its source manifest hashes the actual copied files. One
compatibility build and one validation run per checkout are enforced with locks.
Independent worktrees have independent outputs. Each container runs the exact
image ID produced by its own build; cancellation stops its container before
removing the source snapshot. Dependency download caches may be shared; mutable
frontend/build/package outputs must not be shared.

Keep the working tree fixed during acceptance. Do not run validation and native
frontend builds simultaneously in one checkout: both produce `ui/dist`. Use
another worktree for concurrent changes. Existing cache symlinks remain compatible;
do not share their mutable targets between worktrees. Direct ad hoc Cargo commands
are outside the validation wrapper's lock.

Ordinary tests use fake hardware and private fixture state. The local app runner
uses real user state; launch it deliberately as a manual check. Use the isolated
constructors/probes in [TESTING.md](TESTING.md) for runtime investigations. Run only one hardware acceptance session at a time.
Never install a development package as an incidental build step.

## Checks and review

Pull requests are optional. Use them when a visible diff and discussion help;
the maintainer can also integrate a locally reviewed branch directly. GitHub
does not require additional reviewers or passing status checks for `main`.
The validation expectations below still guide acceptance. The branch rule
blocks force pushes and deletion for users subject to it; administrators retain
GitHub's default bypass. It does not restrict normal branch work.

```sh
tools/validate.sh quick   # syntax, formatting, both Cargo checks, frontend build and UI tests
tools/validate.sh full    # also both Rust test suites and strict Clippy
tools/validate.sh release # also compatibility build and package/release gate
```

Dependencies must already be available; Cargo validation is locked and offline.
Quick validation is for iteration. Full validation is required before merge;
release validation is required for a release candidate. Add focused regression
coverage for changed failures, cancellation, stale state, duplicate actions, and
cleanup. UI changes also require visual inspection. Hardware-dependent claims
need the manual acceptance evidence described in TESTING and HARDWARE-SUPPORT.
An existing failing check blocks acceptance until fixed or explicitly documented
and accepted by a maintainer; do not silently weaken the gate.

Review the final diff using [the review template](docs/templates/REVIEW.md).
Check production callers, persistence readers, startup/shutdown, unsupported
capabilities, and permission boundaries. High-risk lifecycle, replay, updater,
and persisted-format changes receive a separate review pass. Record remaining
limitations. Re-run the required checks after conflict resolution or substantive
review changes, including the final integrated tree before release.

Keep commits small and descriptive. Merge an accepted branch into `main`, retaining
its useful history or using one reviewed squash commit. Do not force-push shared
history. Revert a faulty integration with a new commit; persisted data may need a
separate compatible recovery path. Remove a branch/worktree only after checking
for uncommitted work and confirming its result is retained.

## Documentation and releases

Update facts in their owning guide. Keep significant decisions in
[docs/decisions](docs/decisions/README.md), dated evidence in
[docs/verification](docs/verification/README.md), and current priorities in ROADMAP.
A historical pass does not certify a later commit.

Follow the [release procedure](docs/RELEASING.md) to qualify candidates, configure
release controls, and publish reviewed packages.
