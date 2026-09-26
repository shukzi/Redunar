# Release procedure

## Before creating a candidate

1. Integrate the reviewed change into `main`; validate the integrated tree.
2. Set version metadata using the package specification as authority. Run
   `tools/check-release-inputs.sh vMAJOR.MINOR.PATCH`.
3. Prepare `docs/releases/vMAJOR.MINOR.PATCH.md` with the reviewed release notes.
   The release title will be exactly the tag, for example `v0.1.11`.
4. Review the candidate changes, release notes, acceptance evidence, and recovery
   plan with the release maintainer. Local qualification requires no GitHub upload.
5. Commit all candidate inputs and create the version tag at that commit.
   Keep release tags immutable. A correction gets a new version.

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
After installation, compare installed payloads with the selected RPM, fully quit
the old process, and confirm a fresh process loaded the intended binary/sidecars.
The existing installed-runtime checker reads `target/packages`; for a candidate
in another directory, explicitly stage its selected RPM there before using it.
Do not mix a candidate's results with a different local package.

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
- Protect `main` with pull-request review, resolved review conversations, and
  the `Validate source` status check. Enable that required context after the
  first hosted run confirms its name. Prevent force pushes and branch deletion;
  configure review requirements for the available maintainers.

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
