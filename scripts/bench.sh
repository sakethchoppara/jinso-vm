#!/usr/bin/env bash
# Times lumen against CPython on bench/*.lm and prints a Markdown table.
# Usage: ./scripts/bench.sh [runs]    (reports the best of N runs, default 5)
set -euo pipefail
cd "$(dirname "$0")/.."
runs=${1:-5}

cargo build --release --quiet
lumen=target/release/lumen
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

if [[ -n $py ]]; then
  pyver=$("$py" -c 'import platform; print(platform.python_version())')
  echo "| Benchmark | lumen | CPython $pyver | lumen vs CPython |"
  echo "|---|---|---|---|"
else
  echo "python3 not found; timing lumen only" >&2
  echo "| Benchmark | lumen |"
  echo "|---|---|"
fi

for lm in bench/*.lm; do
  name=$(basename "$lm" .lm)
  t_lumen=$(best "$lumen" run "$lm")
  if [[ -z $py ]]; then
    echo "| $name | ${t_lumen}s |"
    continue
  fi
  # Same program, same answer, or the timing means nothing.
  if [[ "$("$lumen" run "$lm")" != "$("$py" "bench/$name.py" | tr 'TF' 'tf')" ]]; then
    echo "output mismatch on $name" >&2
    exit 1
  fi
  t_py=$(best "$py" "bench/$name.py")
  ratio=$(awk -v l="$t_lumen" -v p="$t_py" 'BEGIN {
    if (l <= p) printf "%.2fx faster", p / l; else printf "%.2fx slower", l / p }')
  echo "| $name | ${t_lumen}s | ${t_py}s | $ratio |"
done
