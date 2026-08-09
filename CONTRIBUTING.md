# Contributing to corvid

Corvid is a deterministic multiplayer game framework: small `corvid_*`
crates under one facade crate, `corvid`. This document is the contract for
changes. It is enforced three ways: the workspace lints in `Cargo.toml`, CI
in `.github/workflows/rust.yml`, and the `.claude/skills/contributing` skill
that Claude Code loads when working here. The skill's `scripts/check.sh`
runs the whole gate below in one command, for humans too.

Parts of the existing code predate these rules. For anything you touch, the
rules win; bringing old code into line is its own scoped PR or an issue, not
a side effect of an unrelated change.

## Every commit passes the gate

Format, lint, test, and build the docs before each commit, not once before
pushing, so that history bisects.

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

CI also runs release-profile tests, a bare-metal target for the `no_std`
crates, a 32-bit target, and the `rust-version` floor; a commit that passes
locally and fails one of those arms is still yours to fix.

## Commits and pull requests

Write commit messages and PR titles as Conventional Commits:

```
type(scope): summary in the imperative, lower case, no trailing period
```

`type` is one of `feat`, `fix`, `docs`, `test`, `refactor`, `perf`,
`build`, `ci` or `chore`, with `!` appended for a breaking change. `scope`
is the crate name minus the `corvid_` prefix, as in `feat(fixed): add
rsqrt`; omit it when a change is workspace-wide. Put the why in the body
when the summary cannot carry it.

Keep a PR to one concern and under 10,000 changed lines. Anything worth
doing that you notice along the way becomes an issue, not a commit.

## Safety

`unsafe_code = "forbid"` is a workspace lint, and FFI is the only
exception. An FFI boundary lives in its own dedicated crate, overrides the
lint there as narrowly as possible with a stated `reason`, carries a
`// SAFETY:` comment on every block, and wraps everything in a safe API
before any other crate touches it.

Lint overrides in general stay that narrow: never `#![allow(...)]` across a
crate without an extremely good reason, stated in the attribute and in the
PR. Prefer an item-level `#[allow(..., reason = "...")]`.

## Code

Write idiomatic, modern Rust. `rust-version` in the workspace manifest is
the toolchain we develop against, not an old floor we preserve; using a new
stable feature is welcome, with the bump in the same PR.

Do not reinvent what `core`, `alloc` or `std` already provides, and derive
what can be derived: a hand-written `Debug` or `PartialEq` is a claim that
the derived one is wrong, and needs to say how. Errors are `thiserror`
enums; serialization is `serde`. Nothing prints to stdout or stderr; emit
`tracing` events and let the binary choose a subscriber. When the same
shape repeats, fold it into a macro, declarative first and `corvid_macros`
when it takes a proc macro.

Comments state what the code cannot: the constraint, the invariant, the
tradeoff. They do not narrate history, restate the next line, or promise
future work; git and the issue tracker hold those.

## Crates and files

`lib.rs` declares modules and exports the public surface; implementation
lives in files named for what they contain. A source file stays under 400
lines and 20 KB. When it gets there, split it along the seams that were
already forming.

Start every crate `#![no_std]` and put `std` behind a feature when an
integration wants it; only a crate whose job is the operating system gets
`std` unconditionally. Code that two crates need lives in a shared crate,
never in two copies. Only the facade crate `corvid` re-exports other
workspace crates; a member that needs a sibling depends on it privately.

## Dependencies and features

If a crate exists that does exactly what you need and nothing else, use it.
If it does ten other things too, it costs more than it saves: build time
and cache hits are a feature of this workspace, and every dependency and
feature flag is weighed against them.

An external dependency is declared by exactly one workspace crate. The
moment a second crate wants it, route it through one internal crate that
re-exports what the workspace uses, so there is a single place to version,
gate, and eventually replace it; the version itself is pinned once in
`[workspace.dependencies]`. Dev-dependencies are exempt from the
single-declaration rule but inherit the same pins.

Ecosystem integrations (`serde`, `bytemuck`, `mint`, `nalgebra`,
`arbitrary`; `ecosystem.md` tracks the roster) sit behind feature flags,
and `default = []` in every crate except the facade, which is the one place
features may be on by default.

## Tests, examples, benchmarks

Code documents itself through its tests and examples: write the test you
would want to read as the explanation of the behavior. Keep the suite fast;
a test earns its runtime, and a slow one has to buy something no fast one
can. Performance-critical code carries Criterion benchmarks in `benches/`,
because a claim about speed without a benchmark is a guess.

## Documentation

A crate README is a tagline saying what the crate does, the basic technical
details, and the scope it will and will not cover, in that order and
concise. Write rustdoc and READMEs as prose rather than lists of things.
Use only plain typeable ASCII in docs and comments, and reach for a Mermaid
diagram when a picture says it better than a paragraph.
