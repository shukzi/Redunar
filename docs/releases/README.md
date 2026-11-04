# Release notes

Before tagging a release, add `<tag>.md` containing the reviewed notes
for that GitHub release. The title is exactly `<tag>`. Include changes, meaningful
limitations, and migration/recovery information. The publish job requires the file
at the qualified commit. Do not fabricate acceptance evidence or copy old results.
See [the release procedure](../RELEASING.md).

Use these sections for the release changes, in this order:

```md
## New features

- Added ...

## Improvements

- Improved ...

## Maintenance

- Updated ...
```

Use **New features** for new user-facing capabilities. Use **Improvements** for
user-facing bug fixes, performance, reliability, capture, overlay, metrics, and
interface changes. Use **Maintenance** for dependency, packaging, tooling, and
internal changes that are relevant to the release. Omit empty sections. Add
release-specific limitations or migration and recovery information when they
apply.
