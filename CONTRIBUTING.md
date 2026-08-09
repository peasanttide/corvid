# Contributing to corvid

Corvid is a deterministic multiplayer game framework: small `corvid_*`
crates under one facade crate, `corvid`. This document is the contract for
changes to it. The workspace lints in `Cargo.toml` hold the parts of it a
lint can express, the `.claude/skills/contributing` skill holds the rest,
and that skill's `scripts/check.sh` runs both in one command.

## Every commit passes the gate

Format, lint, test, and build the docs before each commit rather than once
before pushing, so that the history bisects.

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

## Commits and pull requests

Write commit messages and pull request titles as Conventional Commits:

```
type(scope): summary in the imperative, lower case, no trailing period
```

`type` is one of `feat`, `fix`, `docs`, `test`, `refactor`, `perf`,
`build`, `ci` or `chore`, with `!` appended for a breaking change. `scope`
is the crate name minus the `corvid_` prefix, as in `feat(fixed): add
rsqrt`; omit it when a change is workspace-wide. Put the why in the body
when the summary cannot carry it.

The workspace is pre-1.0, so `!` is a label rather than a burden. Change
the API, fix every caller in the same commit, and leave behind no
deprecation, no compatibility shim, and no second way to do the thing.

Keep a pull request to one concern and under 10,000 changed lines. Answer
every review comment, and answer it with the hash of the commit that
resolves it, so the thread says where the fix is instead of leaving a
reader to find it in the diff.

## Safety

`unsafe_code = "forbid"` is a workspace lint, and FFI is the only
exception. An FFI boundary lives in a crate of its own, overrides the lint
there as narrowly as it can with a stated `reason`, carries a `// SAFETY:`
comment on every block, and is wrapped in a safe API before any other crate
touches it.

Lint overrides in general stay that narrow: never `#![allow(...)]` across a
crate without an extremely good reason, stated in the attribute and in the
pull request. Prefer an item-level `#[allow(..., reason = "...")]`.

## Code

Write idiomatic, modern Rust. `rust-version` in the workspace manifest is
the toolchain we develop against rather than an old floor we preserve;
reaching for a new stable feature is welcome, with the bump in the same
change.

Do not reinvent what `core`, `alloc` or `std` already provides, and derive
what can be derived: a hand-written `Debug` or `PartialEq` is a claim that
the derived one is wrong, and has to say how. Errors are `thiserror` enums,
serialization is `serde`. Nothing prints to stdout or stderr; emit
`tracing` events and let the binary choose a subscriber. When a shape
repeats, fold it into a macro, declarative first and `corvid_macros` when
it takes a proc macro.

Comments carry what the code cannot: the constraint, the invariant, the
tradeoff. They do not restate the line below them, and they do not record
what the code used to be, why it changed, or what it will become. Git and
the pull request hold the history, and a comment that repeats it is a lie
as soon as anyone edits around it.

## Crates and files

`lib.rs` declares the modules and the public surface; the implementation
lives in files named for what they hold. A source file stays under 400
lines and 20 KB, and splits along the seams that were already forming once
it reaches either.

Every crate starts `#![no_std]`, with `std` behind a feature when an
integration needs it; only a crate whose job is the operating system takes
`std` unconditionally. Code that two crates need lives in a shared crate
rather than in two copies. Only the facade crate `corvid` re-exports
workspace crates; a member that needs a sibling depends on it directly.

## Dependencies and features

`[workspace.dependencies]` holds every crate in this workspace and nothing
else, so a member names a sibling as `corvid_fixed = { workspace = true }`
and the version and path are written once.

An external dependency is named by exactly one crate. The moment a second
crate wants it, it earns a crate of its own: one member takes the
dependency and re-exports the parts the workspace uses, and everything else
depends on that member. Versioning it, gating it, and one day replacing it
then happen in a single file. Test-only dependencies are the exception,
since they reach no downstream; name one wherever a test needs it.

If a crate exists that does exactly what you need and nothing else, use it.
If it does ten other things too, it costs more than it saves: build time
and cache hits are a feature of this workspace, and every dependency and
every feature flag is weighed against them.

An ecosystem integration (`serde`, `bytemuck`, `mint`, `nalgebra`,
`arbitrary`; `ecosystem.md` tracks the roster) sits behind a feature flag
on the crate that offers it, gating a dependency that follows the rule
above. Features are `default = []` everywhere except the facade, which is
the one crate where a default feature belongs.

## Tests, examples, benchmarks

Code documents itself through its tests and examples: write the test you
would want to read as the explanation of the behavior. Keep the suite fast.
A test earns its runtime, and a slow one has to buy something no fast one
can. Performance-critical code carries Criterion benchmarks in `benches/`,
because a claim about speed without a benchmark is a guess.

## Documentation

A crate README is a tagline saying what the crate does, then the basic
technical details, then the scope it will and will not cover, in that order
and concise. Write rustdoc and READMEs as prose rather than as lists of
things. Use only plain typeable ASCII, and reach for a Mermaid diagram when
a picture says it better than a paragraph.

Name code so that the doc build checks the name. An item mentioned in
rustdoc is an intra-doc link, ``[`Angle16::from_degrees`]`` rather than the
same text in prose, so that renaming it fails `cargo doc` instead of
leaving a document that lies. `#![doc = include_str!("../README.md")]`
makes a README the crate's front page and holds it to the same rule: its
links resolve as intra-doc links and its examples compile and run as
doctests, so state a claim about the API as an example rather than as prose
that nothing checks.
