# 0001 — Isolated development and qualification before release

- Date: September 26, 2026
- Status: Implemented locally; hosted activation pending

## Context

The repository had one local branch, a published-release build trigger, and
compatibility builds that deleted directories shared with host Cargo checks.
TESTING recorded an interrupted check caused by that shared target cleanup.

## Decision

Use short-lived task branches and separate worktrees for concurrent changes.
Give compatibility builds copied source inputs and checkout-owned targets. Use
one local/CI validation entry point covering both Cargo workspaces. Qualify exact
candidate bytes before signing and publication. Require release review before
publishing.

## Alternatives and consequences

A permanent develop branch adds another integration boundary without a current
need. Shared mutable targets reduce disk use but allow one task to invalidate
another's build. Isolated targets use more disk and take longer on a cold build;
Cargo download caches remain shared. Repository protections need separate
configuration. Hardware and installed-runtime acceptance remain separate.
