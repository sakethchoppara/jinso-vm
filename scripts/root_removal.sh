#!/usr/bin/env bash
# Root-removal experiment: applies each deliberate GC bug in scripts/mutants/ to a
# temporary copy of the crate, runs the test suite, and fails if any bug goes unnoticed.
#
# Each mutant is a patch, so when the code it targets changes, the patch stops applying
# and this script fails loudly instead of silently testing nothing. Regenerate the patch
# against the new code in that case. The working tree is never modified.
#
# Usage: ./scripts/root_removal.sh
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -r Cargo.toml Cargo.lock src tests "$work"/
# A separate target dir: builds are reused between mutants and never touch target/release.
export CARGO_TARGET_DIR="$root/target/root-removal"

status=0
for patch in scripts/mutants/*.patch; do
  name=$(basename "$patch" .patch)
  if ! (cd "$work" && git apply "$root/$patch"); then
    echo "STALE    $name: patch no longer applies; regenerate it against the current code"
    status=1
    continue
  fi
  if out=$(cd "$work" && cargo test -q 2>&1); then
    echo "MISSED   $name: every test passed with this bug in place"
    status=1
  elif grep -q "could not compile" <<< "$out"; then
    echo "BROKEN   $name: the mutant doesn't compile, so it proves nothing"
    status=1
  else
    caught=$(grep -oE '^---- [a-z0-9_]+' <<< "$out" | cut -c6- | sort -u | tr '\n' ' ')
    echo "caught   $name: $caught"
  fi
  (cd "$work" && git apply -R "$root/$patch")
done
exit $status
