---
name: contributing
description: Corvid's contribution contract and the gate every commit must pass. Use whenever writing or editing Rust code, Cargo manifests, or docs in this repository; before every commit, push, or pull request; when composing commit messages or PR titles; when adding a crate or a dependency; when answering review comments; and when asked to audit, review, fix, or check changes against the project rules, even if CONTRIBUTING.md is not named.
---

# Enforcing CONTRIBUTING.md

`CONTRIBUTING.md` at the repo root is the contract; this skill is the
procedure that holds a change to it. Read the contract once per session. It
is short, and this file does not repeat its reasoning.

## While writing

Catch these while the code is still moving; each is rework if it surfaces
at review instead:

- No `unsafe` (the workspace forbids it). FFI alone may override, in a
  crate of its own, narrowly, with a stated `reason`, a `// SAFETY:`
  comment per block, and a safe wrapper around the whole surface.
- No output to stdout or stderr; emit `tracing` events.
- Derive instead of hand-implementing. `thiserror` for errors; `serde`,
  behind a feature flag, for serialization.
- Check `core`/`alloc`/`std` before writing a mechanism, and crates.io for
  a crate that does exactly the job and nothing else before writing a
  module. Fold repetition into a macro; move code two crates need into a
  shared crate rather than copying it.
- `[workspace.dependencies]` lists the workspace's own crates and nothing
  else; a member names a sibling as `corvid_x = { workspace = true }`. An
  external dependency is named by exactly one crate, and the second crate
  that wants it gets one instead: a member that takes the dependency and
  re-exports what the workspace uses. Test-only dependencies are exempt.
- New crates start `#![no_std]` with `default = []` and
  `[lints] workspace = true`, and a README of tagline, technical details,
  then scope. Only the facade crate `corvid` re-exports workspace crates,
  and only the facade may have default features.
- `lib.rs` holds the module declarations and the exports, not the
  implementation. Every source file stays under 400 lines and 20 KB.
- Docs and comments: plain typeable ASCII, prose over lists, Mermaid for
  diagrams. Name an item as an intra-doc link so a rename fails the doc
  build. Never record what the code used to be, why it changed, or what it
  will become, and never restate the line below.
- Pre-1.0: change an API outright, fix its callers in the same commit, and
  leave no deprecation or compatibility shim behind.

## Before every commit

Run the gate and the mechanical checks in one command from the repo root:

```sh
.claude/skills/contributing/scripts/check.sh [--fix] [base-ref]
```

The base defaults to `origin/main`, and the diff it reads is the working
tree, so uncommitted and untracked work is checked too. The script runs
`cargo fmt --check`, clippy with warnings denied, the all-features test
suite, and the rustdoc build, then checks the diff for file-size
violations, non-ASCII additions, crate-wide `allow` attributes, malformed
commit subjects, and total change size. Every FAIL blocks the commit; a
NOTE marks a judgment call to resolve deliberately and explain in the pull
request if you keep it.

`--fix` first applies what can be applied mechanically: `cargo fmt --all`,
`cargo clippy --fix`, and the ASCII transliteration of typography an editor
inserted on its own. Then it runs the full check, so what it prints is what
is left for you. Read the resulting diff before committing it: a clippy fix
is a suggestion rather than a decision, and the transliteration normalizes
every file the change touches rather than only the lines it adds, so a file
that was already untypeable comes back with more than you wrote. What
`--fix` will never do for you is split a file over the size limit, remove a
crate-wide `allow`, rewrite a commit subject, or shrink a pull request,
because each of those is a design choice.

Run the check before each commit rather than once before pushing, so every
commit stands on its own and the history bisects.

Shape the message before committing, since the script can only check it
after:

```
type(scope): summary in the imperative, lower case, no trailing period
```

Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`,
`chore`, with `!` for a breaking change. Scope is the crate short name
(`fixed`, not `corvid_fixed`), omitted for workspace-wide changes. Pull
request titles use the same format.

**Example**: adding a reciprocal square root to `corvid_fixed` gives
`feat(fixed): add rsqrt to the fixed-point scalars`.

## Answering review comments

Fix first, then answer: every review comment gets a reply naming the commit
hash that resolves it, so the thread points at the change instead of
leaving the reader to find it. One comment that asks for two things gets
both hashes. A comment you are not acting on gets a reply saying why, not
silence.

## What the script cannot check

Read the final diff for these before pushing:

- Comments that record history, plans, or the obvious. Delete them.
- A hand-written impl where a derive would do; a mechanism `std` already
  provides; code copied between crates instead of moved to a shared one;
  repetition a macro should fold.
- Tests that restate the implementation instead of pinning behavior, or
  that spend runtime without buying confidence.
- Performance-critical code with no Criterion benchmark in `benches/`.
- A README that has drifted from tagline, technical details, scope.
- A pull request that has grown a second concern. Split it.

## When a rule is in the way

Do not bend it silently. Keep any override as narrow as the language
allows: `#[allow(..., reason = "...")]` on the item, never the crate
without an extremely good reason. Justify it in the pull request
description. Code that does not follow the contract is not a precedent for
writing more of it.

## Audit mode

When invoked directly (`/contributing`) or asked to review a branch or diff
against the rules: run `scripts/check.sh <base>`, read the diff for the
judgment checks above, and report each violation as `file:line`, the rule
it breaks, and the smallest fix, ordered by severity. Audit mode changes
nothing unless asked; `/contributing --fix` is the form that applies the
mechanical fixes and then reports what is left.
