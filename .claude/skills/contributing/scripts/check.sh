#!/usr/bin/env bash
# The mechanical half of CONTRIBUTING.md: the commit gate, then the checks
# no workspace lint covers. Usage: check.sh [base-ref], base defaulting to
# origin/main. Exits nonzero if any FAIL printed; a NOTE needs a decision,
# not necessarily a change.
set -u
cd "$(git rev-parse --show-toplevel)" || exit 2

BASE="${1:-origin/main}"
git rev-parse -q --verify "$BASE" >/dev/null 2>&1 || BASE=main
git rev-parse -q --verify "$BASE" >/dev/null 2>&1 || {
    echo "no usable base ref; pass one: check.sh <base-ref>" >&2
    exit 2
}

status=0
say()  { printf '\n== %s\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1"; status=1; }
note() { printf 'NOTE: %s\n' "$1"; }

say "gate: format"
cargo fmt --all --check || fail "cargo fmt"
say "gate: clippy, warnings denied"
cargo clippy --workspace --all-targets --all-features -- -D warnings \
    || fail "clippy"
say "gate: tests, all features"
cargo test --workspace --all-features || fail "tests"
say "gate: docs, warnings denied"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features \
    || fail "docs"

say "file limits: 400 lines, 20 KB"
while IFS= read -r f; do
    [ -f "$f" ] || continue
    lines=$(wc -l < "$f")
    bytes=$(wc -c < "$f")
    if [ "$lines" -gt 400 ] || [ "$bytes" -gt 20480 ]; then
        # A file already over the limit at the base is legacy debt, not this
        # change's violation; the rule for it is only that it must not grow.
        if git cat-file -e "$BASE:$f" 2>/dev/null &&
            { [ "$(git show "$BASE:$f" | wc -l)" -gt 400 ] ||
              [ "$(git show "$BASE:$f" | wc -c)" -gt 20480 ]; }; then
            note "$f predates the limit ($lines lines, $bytes bytes): do not grow it; split it or file an issue"
        else
            fail "$f is $lines lines, $bytes bytes (limits 400 lines, 20480 bytes): split it"
        fi
    fi
done < <(git diff --name-only --diff-filter=ACMR "$BASE"...HEAD -- '*.rs')

say "typeable characters in added lines"
if git diff "$BASE"...HEAD --unified=0 -- '*.rs' '*.md' '*.toml' \
    | LC_ALL=C grep -n $'^+[^+].*[^\t -~]'; then
    fail "the added lines above use non-ASCII; docs and comments stay typeable"
fi

say "crate-wide allow attributes in changed files"
hits=$(git diff --name-only --diff-filter=ACMR "$BASE"...HEAD -- '*.rs' \
    | xargs -r grep -Hn '^#!\[allow' || true)
if [ -n "$hits" ]; then
    printf '%s\n' "$hits"
    note "a crate- or module-wide allow needs an extremely good reason, stated in the attribute and the PR"
fi

say "commit subjects: Conventional Commits"
RE='^(feat|fix|docs|test|refactor|perf|build|ci|chore)(\([a-z0-9_]+\))?!?: .+'
if git log --no-merges --format=%s "$BASE"..HEAD | grep -Ev "$RE"; then
    fail "the subjects above do not parse as type(scope): summary"
fi

say "change size"
changed=$(git diff --numstat "$BASE"...HEAD | awk '{ n += $1 + $2 } END { print n + 0 }')
echo "$changed lines changed against $BASE"
if [ "$changed" -ge 10000 ]; then
    fail "a PR stays under 10,000 changed lines: split it"
fi

exit "$status"
