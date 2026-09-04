#!/usr/bin/env bash
# Runs the stack at several bot counts and prints one row per level.
#
# Everything runs on this machine, so the numbers include the load generator
# and the edge competing with the region for CPU. That is a floor, not a
# measurement of what a region can do on hardware of its own. Watch `late`
# against `tick`: a tick that stays cheap while lateness climbs is the box
# being oversubscribed, not the engine running out.
#
#   scripts/sweep.sh                      # 100 250 500 1000
#   scripts/sweep.sh 200 400 800          # your own levels
#   SECONDS_PER=30 scripts/sweep.sh       # longer runs
set -uo pipefail
cd "$(dirname "$0")/.."

LEVELS=("$@")
[ ${#LEVELS[@]} -eq 0 ] && LEVELS=(100 250 500 1000)
SECONDS_PER="${SECONDS_PER:-20}"
SPREAD="${SPREAD:-10}"
PER_CONN="${PER_CONN:-50}"
TICK_HZ=20
OUT="${OUT:-/tmp/mv-sweep}"
mkdir -p "$OUT"

command -v nats-server >/dev/null || { echo "nats-server not on PATH"; exit 1; }
pgrep -x nats-server >/dev/null || { echo "start nats-server first"; exit 1; }

cargo build --release --quiet > "$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 1; }

# Track PIDs and wait on each after killing. Without the wait, bash reports
# "Terminated" for every job as it reaps them, interleaved with the table.
PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null
  done
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] && wait "$pid" 2>/dev/null
  done
  PIDS=()
}
trap cleanup EXIT

printf '%8s  %9s  %9s  %9s  %7s  %10s  %9s  %8s\n' \
  bots tick_p50 tick_p99 tick_max late% updates/obs gap_mean kept%
printf '%8s  %9s  %9s  %9s  %7s  %10s  %9s  %8s\n' \
  -------- --------- --------- --------- ------- ---------- --------- --------

for n in "${LEVELS[@]}"; do
  cleanup; sleep 1
  ticks=$(( SECONDS_PER * TICK_HZ ))

  ./target/release/mv-sim --ticks "$ticks" --report "$SECONDS_PER" \
    > "$OUT/sim-$n.log" 2>&1 &
  sim_pid=$!
  sleep 2
  ./target/release/mv-edge > "$OUT/edge-$n.log" 2>&1 &
  PIDS+=($!)
  sleep 2
  # Planting off: crops are never removed, so leaving it on would grow the
  # world during the run and every level would measure a different one.
  ./target/release/mildew-load --bots "$n" --per-connection "$PER_CONN" \
    --spread "$SPREAD" --plant-every 0 > "$OUT/load-$n.log" 2>&1 &
  PIDS+=($!)

  wait "$sim_pid" 2>/dev/null   # the sim exits on its own after --ticks
  cleanup; sleep 1

  # Last full window from each side.
  sim=$(grep -h 'ticks/' "$OUT/sim-$n.log" | tail -1)
  load=$(grep -h 'farmers on' "$OUT/load-$n.log" | tail -1)

  p50=$(sed -n 's/.*tick p50 \([0-9.]*\)ms.*/\1/p'   <<<"$sim")
  p99=$(sed -n 's/.*p99 \([0-9.]*\)ms.*/\1/p'        <<<"$sim")
  max=$(sed -n 's/.*max \([0-9.]*\)ms |.*/\1/p'      <<<"$sim")
  kept=$(sed -n 's/.*(\([0-9]*\)% kept).*/\1/p'      <<<"$sim")
  latef=$(grep -h 'run of' "$OUT/sim-$n.log" | sed -n 's/.*late [0-9]* (\([0-9.]*\)%).*/\1/p')
  upo=$(sed -n 's/.*(\([0-9]*\)\/observer).*/\1/p'   <<<"$load")
  gap=$(sed -n 's/.*gap mean \([0-9.]*\)ms.*/\1/p'   <<<"$load")

  printf '%8s  %9s  %9s  %9s  %7s  %10s  %9s  %8s\n' \
    "$n" "${p50:--}" "${p99:--}" "${max:--}" "${latef:--}" "${upo:--}" "${gap:--}" "${kept:--}"
done

echo
echo "logs in $OUT"
echo "tick period at ${TICK_HZ}Hz is $(( 1000 / TICK_HZ ))ms; gap_mean should sit near it"
