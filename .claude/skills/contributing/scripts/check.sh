#!/usr/bin/env bash
# The mechanical half of CONTRIBUTING.md: the commit gate, then the checks
# no workspace lint covers. Usage: check.sh [--fix] [base-ref], base
# defaulting to origin/main. The diff is the working tree against the merge
# base, so uncommitted and untracked work counts. Exits nonzero if any FAIL
# printed; a NOTE needs a decision, not necessarily a change.
set -u
cd "$(git rev-parse --show-toplevel)" || exit 2

FIX=0
BASE=""
for arg in "$@"; do
    case "$arg" in
        --fix) FIX=1 ;;
        -*) echo "usage: check.sh [--fix] [base-ref]" >&2; exit 2 ;;
        *) BASE="$arg" ;;
    esac
done

for candidate in "$BASE" origin/main main; do
    [ -n "$candidate" ] || continue
    if git rev-parse -q --verify "$candidate" >/dev/null 2>&1; then
        BASE="$candidate"
        break
    fi
done
MERGE_BASE=$(git merge-base "$BASE" HEAD 2>/dev/null) || {
    echo "no usable base ref; pass one: check.sh [--fix] <base-ref>" >&2
    exit 2
}

status=0
say()  { printf '\n== %s\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1"; status=1; }
note() { printf 'NOTE: %s\n' "$1"; }

# Everything this change is answerable for: tracked files it touches, plus
# files not yet added, which are the ones a pre-commit run most needs to see.
touched() {
    {
        git diff --name-only --diff-filter=ACMR "$MERGE_BASE" -- "$@"
        git ls-files --others --exclude-standard -- "$@"
    } | sort -u
}

if [ "$FIX" -eq 1 ]; then
    say "fix: format"
    cargo fmt --all
    say "fix: clippy, machine-applicable suggestions"
    # No `-D warnings` here: the point is to apply what it can, and the
    # check below is what decides whether the result passes.
    cargo clippy --fix --workspace --all-targets --all-features \
        --allow-dirty --allow-staged
    say "fix: typeable characters"
    # The punctuation an editor substitutes on its own, mapped back to what a
    # keyboard types. Anything else non-ASCII is a decision somebody made, and
    # the check below reports it rather than guessing at it.
    while IFS= read -r f; do
        [ -n "$f" ] && [ -f "$f" ] || continue
        before=$(cksum < "$f")
        perl -CSD -pi -e '
            s/[\x{2018}\x{2019}\x{201B}]/\x{27}/g;
            s/[\x{201C}\x{201D}]/"/g;
            s/[\x{2014}\x{2015}]/--/g;
            s/[\x{2013}\x{2212}]/-/g;
            s/\x{2026}/.../g;
            s/[\x{00A0}\x{2007}\x{202F}]/ /g;
            s/\x{2192}/->/g;
            s/\x{2190}/<-/g;
            s/\x{00D7}/x/g;
            s/\x{2264}/<=/g;
            s/\x{2265}/>=/g;
            s/\x{2260}/!=/g;
        ' "$f"
        [ "$before" = "$(cksum < "$f")" ] || echo "  rewrote $f"
    done < <(touched '*.rs' '*.md' '*.toml')
fi

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
    [ -n "$f" ] && [ -f "$f" ] || continue
    lines=$(wc -l < "$f")
    bytes=$(wc -c < "$f")
    if [ "$lines" -gt 400 ] || [ "$bytes" -gt 20480 ]; then
        fail "$f is $lines lines, $bytes bytes (limits 400, 20480): split it"
    fi
done < <(touched '*.rs')

say "typeable characters"
found=0
if git diff --unified=0 "$MERGE_BASE" -- '*.rs' '*.md' '*.toml' \
    | LC_ALL=C grep -n $'^+[^+].*[^\t -~]'; then
    found=1
fi
while IFS= read -r f; do
    [ -n "$f" ] && [ -f "$f" ] || continue
    LC_ALL=C grep -Hn $'[^\t -~]' "$f" && found=1
done < <(git ls-files --others --exclude-standard -- '*.rs' '*.md' '*.toml')
[ "$found" -eq 1 ] && fail "the lines above are not typeable ASCII; --fix handles the common cases"

say "crate-wide allow attributes"
hits=$(touched '*.rs' | tr '\n' '\0' | xargs -0 -r grep -Hn '^#!\[allow' || true)
if [ -n "$hits" ]; then
    printf '%s\n' "$hits"
    note "a crate- or module-wide allow needs an extremely good reason, stated in the attribute and the pull request"
fi

say "commit subjects: Conventional Commits"
RE='^(feat|fix|docs|test|refactor|perf|build|ci|chore)(\([a-z0-9_]+\))?!?: .+'
if git log --no-merges --format=%s "$MERGE_BASE"..HEAD | grep -Ev "$RE"; then
    fail "the subjects above do not parse as type(scope): summary"
fi

say "change size"
tracked=$(git diff --numstat "$MERGE_BASE" | awk '{ n += $1 + $2 } END { print n + 0 }')
untracked=$(git ls-files --others --exclude-standard -z \
    | xargs -0 -r wc -l 2>/dev/null | awk 'END { print $1 + 0 }')
changed=$((tracked + untracked))
echo "$changed lines changed against $BASE"
if [ "$changed" -ge 10000 ]; then
    fail "a pull request stays under 10,000 changed lines: split it"
fi

exit "$status"
