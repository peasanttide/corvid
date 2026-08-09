---
name: contributing
description: Corvid's contribution contract and the gate every commit must pass. Use whenever writing or editing Rust code, Cargo manifests, or docs in this repository; before every commit, push, or pull request; when composing commit messages or PR titles; when adding a crate or a dependency; and when asked to audit, review, or check changes against the project rules, even if CONTRIBUTING.md is not named.
---

# Enforcing CONTRIBUTING.md

`CONTRIBUTING.md` at the repo root is the contract; this skill is the
procedure that holds a change to it. Read the contract once per session. It
is short, and this file does not repeat its reasoning.

## While writing

Catch these while the code is still moving; each is rework if it surfaces
at review instead:

- No `unsafe` (the workspace forbids it). FFI alone may override, in its
  own dedicated crate, narrowly, with a stated `reason`, a `// SAFETY:`
  comment per block, and a safe wrapper around the whole surface.
- No output to stdout or stderr; emit `tracing` events.
- Derive instead of hand-implementing. `thiserror` for errors; `serde`,
  behind a feature flag, for serialization.
- Check `core`/`alloc`/`std` before writing a mechanism, and crates.io for
  a crate that does exactly the job and nothing else before writing a
  module. Fold repetition into a macro; move code two crates need into a
  shared crate rather than copying it.
- An external dependency lives in exactly one crate's `[dependencies]`; a
  second taker means one internal crate re-exports it for everyone, with
  the version pinned in `[workspace.dependencies]`.
- New crates start `#![no_std]` with `default = []` and
  `[lints] workspace = true`, and a README of tagline, technical details,
  then scope. Only the facade crate `corvid` re-exports workspace crates,
  and only the facade may have default features.
- `lib.rs` holds module declarations and exports, not implementation.
  Every source file stays under 400 lines and 20 KB.
- Docs and comments: plain typeable ASCII, prose over lists, Mermaid for
  diagrams, and nothing that narrates history, plans, or the line below.

## Before every commit

Run the gate and the mechanical checks in one command from the repo root:

```sh
.claude/skills/contributing/scripts/check.sh [base-ref]
```

The base defaults to `origin/main`. The script runs `cargo fmt --check`,
clippy with warnings denied, the all-features test suite, and the rustdoc
build, then checks the branch diff for file-size violations, non-ASCII
additions, crate-wide `allow` attributes, malformed commit subjects, and
total change size. Every FAIL blocks the commit. A NOTE marks a judgment
call: resolve it deliberately, and say how in the PR description if you
keep it.

Run it before each commit rather than once before pushing; each commit must
pass on its own so the history bisects.

Shape the message before committing, since the script can only check it
after:

```
type(scope): summary in the imperative, lower case, no trailing period
```

Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`,
`chore`, with `!` for a breaking change. Scope is the crate short name
(`fixed`, not `corvid_fixed`), omitted for workspace-wide changes. PR
titles use the same format.

**Example**: adding a reciprocal square root to `corvid_fixed` gives
`feat(fixed): add rsqrt to the fixed-point scalars`.

## What the script cannot check

Read the final diff for these before pushing:

- Comments that narrate history, plans, or the obvious. Delete them.
- A hand-written impl where a derive would do; a mechanism `std` already
  provides; code copied between crates instead of moved to a shared one;
  repetition a macro should fold.
- Tests that restate the implementation instead of pinning behavior, or
  that spend runtime without buying confidence.
- Performance-critical code with no Criterion benchmark in `benches/`.
- A README that has drifted from tagline, technical details, scope.
- A PR that has grown a second concern: split it, or file an issue for the
  part that can wait.

## When a rule is in the way

Do not bend it silently. Keep any override as narrow as the language
allows: `#[allow(..., reason = "...")]` on the item, never the crate
without an extremely good reason. Justify it in the PR description, and
open a follow-up issue when the honest fix is out of scope. Existing code
that predates the rules is not license to match it: what you touch follows
the contract, and what you do not touch becomes an issue instead of a
drive-by.

## Audit mode

When invoked directly (`/contributing`) or asked to review a branch or diff
against the rules: run `scripts/check.sh <base>` with the branch's merge
base, read the diff for the judgment checks above, and report each
violation as `file:line`, the rule it breaks, and the smallest fix, ordered
by severity. Do not fix anything in audit mode unless asked to.
