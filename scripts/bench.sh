#!/usr/bin/env bash
# Times jinso against CPython on bench/*.jn and prints a Markdown table.
# Usage: ./scripts/bench.sh [runs]    (reports the best of N runs, default 5)
#
# Wall-clock time moves by about ±5% between builds from code layout alone. When `perf`
# can count events, the table adds jinso's instructions executed, which barely changes
# between runs and rebuilds: compare that column to judge a code change.
set -euo pipefail
cd "$(dirname "$0")/.."
runs=${1:-5}

cargo build --release --quiet
jinso=target/release/jinso
py=$(command -v python3 || true)

# Best wall-clock seconds over $runs runs of the given command.
best() {
  local best=999999 t
  for _ in $(seq "$runs"); do
    t=$( { TIMEFORMAT=%R; time "$@" > /dev/null; } 2>&1 )
    best=$(awk -v a="$t" -v b="$best" 'BEGIN { print (a < b ? a : b) }')
  done
  echo "$best"
}

# Millions of instructions jinso executes on a program, or empty if perf can't count.
instructions() {
  perf stat -e instructions -x, "$jinso" run "$1" 2>&1 > /dev/null | awk -F, '$1 ~ /^[0-9]+$/ { printf "%d", $1 / 1e6 }'
}

has_perf=""
if command -v perf > /dev/null && [[ -n "$(instructions bench/fib.jn 2> /dev/null || true)" ]]; then
  has_perf=1
else
  echo "perf can't count instructions here; showing wall-clock time only" >&2
fi

header="| Benchmark | jinso"
rule="|---|---"
[[ -n $py ]] && pyver=$("$py" -c 'import platform; print(platform.python_version())') &&
  header+=" | CPython $pyver | jinso vs CPython" && rule+="|---|---"
[[ -n $has_perf ]] && header+=" | jinso instructions" && rule+="|---"
echo "$header |"
echo "$rule|"

for lm in bench/*.jn; do
  name=$(basename "$lm" .jn)
  t_jinso=$(best "$jinso" run "$lm")
  row="| $name | ${t_jinso}s"
  if [[ -n $py ]]; then
    # Same program, same answer, or the timing means nothing.
    if [[ "$("$jinso" run "$lm")" != "$("$py" "bench/$name.py" | tr 'TF' 'tf')" ]]; then
      echo "output mismatch on $name" >&2
      exit 1
    fi
    t_py=$(best "$py" "bench/$name.py")
    ratio=$(awk -v l="$t_jinso" -v p="$t_py" 'BEGIN {
      if (l <= p) printf "%.2fx faster", p / l; else printf "%.2fx slower", l / p }')
    row="| $name | ${t_jinso}s | ${t_py}s | $ratio"
  fi
  [[ -n $has_perf ]] && row+=" | $(instructions "$lm")M"
  echo "$row |"
done
