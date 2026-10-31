# Release procedure

## Before creating a candidate

1. Merge the reviewed implementation pull requests into `main`; validate the
   integrated tree.
2. Set version metadata on a release task branch using the package specification
   as authority. Run `tools/sync-release-version.sh MAJOR.MINOR.PATCH` so the
   native Cargo manifest, Tauri JSON config, and latest AppStream release stay
   synchronized, then run `tools/check-release-inputs.sh vMAJOR.MINOR.PATCH`.
   Internal shared Rust crate versions are separate dependency metadata and do
   not identify the packaged application release.
3. Prepare `docs/releases/vMAJOR.MINOR.PATCH.md` with the reviewed release notes.
   The release title will be exactly the tag, for example `v0.1.11`.
4. Review the candidate changes, release notes, acceptance evidence, and recovery
   plan with the release maintainer. Local qualification requires no GitHub upload.
5. Commit the candidate inputs on the release task branch and merge its reviewed
   pull request into `main`. Validate the integrated tree and create the version
   tag at that exact commit. Keep release tags immutable. A correction gets a
   new version.

## Qualify before publication

```sh
tools/prepare-release.sh vMAJOR.MINOR.PATCH
```

This requires a clean source tree with the specified tag at HEAD, runs full
validation and the release gate, and prepares unsigned packages under
`.redunar-build/candidates/<tag>-<commit>/`. Existing candidates are not overwritten.
An interrupted candidate is explicitly incomplete; inspect and preserve evidence
before removing only that incomplete directory for a deliberate retry.

Record the exact commit, source-manifest hash, environment, artifact hashes,
automated results, manual acceptance scope, known limitations, and state-format
compatibility. Preserve the previous known-good packages. Package structure
checks do not establish other-distribution runtime or NVIDIA support.

Plan installation and real-game/hardware acceptance separately from automated
checks, including their effects on the test system.
Installation and restarting the app require separate authorization. Select the
native package for the target distribution: Arch `.pkg.tar.zst`, Debian/Ubuntu
`.deb`, or the matching RPM. Record its hash and extract that exact package into
a private temporary directory. Compare the installed executable, sidecars,
helpers, resources, and permissions with its extracted payload. Compare against
packaged bytes rather than raw build outputs, which packaging may strip.
After the authorized restart, confirm a fresh user process loaded the intended
installed executable and sidecars. Do not mix a candidate's results with a
different local package.

`tools/check-installed-tauri-runtime.sh` is an RPM-only convenience check. It
selects an RPM under `target/packages`; ensure that selection is the exact
qualified package before using it. It does not verify Arch or DEB installations
and does not replace checking the selected package's resources and permissions.

## GitHub workflow

The workflow is manual. `publish=false` qualifies and retains an unsigned
candidate and reports as Actions artifacts without creating a GitHub release.
This uploads source-derived artifacts and logs. `publish=true` first performs
the same qualification, then waits for the configured `release` environment's
required review before signing and publishing.

### Repository configuration

Before enabling publication:

- Configure the `release` environment with required maintainer review and restrict
  it to the intended workflow ref. Naming an environment in YAML alone does not
  configure those protections.
- Store `REDUNAR_RELEASE_SIGNING_KEY` as a secret in that environment. Validation
  jobs must not receive it. Remove a repository-wide copy after confirming the
  environment secret is configured correctly.
- Set the repository variable `REDUNAR_RELEASE_PUBLICATION_ENABLED` to `true`
  only after the environment and workflow configuration have been reviewed.
- Protect `main`: require pull requests, apply the rule to administrators, and
  prevent force pushes and branch deletion. Additional reviewer approvals and
  status checks are not currently mandatory GitHub merge gates. Review the diff
  and validation results before integration as described in
  [CONTRIBUTING.md](../CONTRIBUTING.md).

If required environment review is unavailable for the repository, keep automated
publication disabled and use a reviewed manual release process. Workflow files
do not apply branch or environment protection settings themselves.

### Publish the qualified candidate

The release reviewer checks the candidate commit, retained reports, release title
(the tag), and `docs/releases/<tag>.md`. The publish job checks package hashes,
signs the qualified bytes without rebuilding, verifies the signature, and creates
a release with that title and notes. It refuses an existing release instead of
replacing published assets. Release notes come from the reviewed file.

Investigate a failed upload before taking a recovery action. Do not blindly rerun
publication or replace assets with `--clobber`.

## Recovery

For a code regression, prepare a reviewed revert on a fix branch. Preserve the
failed release and logs. Build a new corrected version through the same gates.
Before suggesting a package downgrade, check whether the older binary can read
state written by the new version. Back up affected non-clip state when a migration
needs it; never remove recordings as cleanup. Describe any unsupported downgrade
in the release notes and provide a forward repair where needed.
