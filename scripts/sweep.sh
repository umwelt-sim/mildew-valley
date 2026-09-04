#!/usr/bin/env bash
# Runs the stack at several bot counts and prints one row per level.
#
# Each level runs REPEATS times and the row reports the median with the
# observed range. A single run varies enough that one number reads as more
# precise than it is.
#
# Everything runs on this machine, so the numbers include the load generator
# and the edge competing with the region for CPU. That is a floor, not a
# measurement of what a region can do on hardware of its own.
#
# Read undeliv first. Observations ride QUIC datagrams and the edge drops
# rather than queues when a client has no room, so a load generator that
# cannot drain looks exactly like a region that cannot send. A nonzero value
# means only the sim-side columns on that row can be trusted.
#
#   scripts/sweep.sh                        # 1000 2000 4000 8000
#   scripts/sweep.sh 500 1000               # your own levels
#   REPEATS=5 SECONDS_PER=30 scripts/sweep.sh
#   X=200 Y=200 scripts/sweep.sh 8000       # crowd near the region corner
set -uo pipefail
cd "$(dirname "$0")/.."

LEVELS=("$@")
[ ${#LEVELS[@]} -eq 0 ] && LEVELS=(1000 2000 4000 8000)
SECONDS_PER="${SECONDS_PER:-15}"
REPEATS="${REPEATS:-3}"
SPREAD="${SPREAD:-10}"
# One connection cannot carry many observers' datagrams at tick rate: its send
# buffer stays full and the edge drops rather than queues. At 250 a run reports
# hundreds of thousands undeliverable that a real client, being one observer on
# one connection, would never see. At 50 it is zero.
PER_CONN="${PER_CONN:-50}"
# Inside a cell rather than on its edge. 2048 is exactly 16 x 128 m, so a
# crowd there straddles four cells and splits its own candidate work, which
# makes the region look cheaper than it is.
X="${X:-2100}"
Y="${Y:-2100}"
TICK_HZ=20
OUT="${OUT:-/tmp/mv-sweep}"
mkdir -p "$OUT"

command -v nats-server >/dev/null || { echo "nats-server not on PATH"; exit 1; }
pgrep -x nats-server >/dev/null || { echo "start nats-server first"; exit 1; }
cargo build --release --quiet > "$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 1; }

PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && kill "$pid" 2>/dev/null; done
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && wait "$pid" 2>/dev/null; done
  PIDS=()
}
trap cleanup EXIT

# Middle line of those matching a pattern. The load generator keeps printing
# after the sim exits on its tick count, and those windows report no arrivals.
mid_window() {
  local n
  n=$(grep -hc "$2" "$1" 2>/dev/null || echo 0)
  [ "$n" -lt 1 ] && return
  grep -h "$2" "$1" | sed -n "$(( (n + 1) / 2 ))p"
}

median() { printf '%s\n' "$@" | sort -n | awk '{v[NR]=$1} END{print (NR%2)?v[(NR+1)/2]:(v[NR/2]+v[NR/2+1])/2}'; }
lo()     { printf '%s\n' "$@" | sort -n | head -1; }
hi()     { printf '%s\n' "$@" | sort -n | tail -1; }

echo "$REPEATS runs per level, ${SECONDS_PER}s each, crowd at $X,$Y within ${SPREAD}m"
echo
printf '%7s  %20s  %16s  %8s  %9s  %7s  %10s\n' \
  bots 'tick_p50 (range)' 'late% (range)' 'upd/obs' gap_mean kept% undeliv
printf '%7s  %20s  %16s  %8s  %9s  %7s  %10s\n' \
  ------- -------------------- ---------------- -------- --------- ------- ----------

for n in "${LEVELS[@]}"; do
  p50s=(); lates=(); upos=(); gaps=(); kepts=(); unds=()
  ticks=$(( SECONDS_PER * TICK_HZ ))

  for r in $(seq 1 "$REPEATS"); do
    cleanup; sleep 1
    tag="$n-$r"
    ./target/release/mv-sim --ticks "$ticks" --report "$SECONDS_PER" \
      > "$OUT/sim-$tag.log" 2>&1 &
    sim_pid=$!
    sleep 2
    ./target/release/mv-edge > "$OUT/edge-$tag.log" 2>&1 &
    PIDS+=($!)
    sleep 2
    # Planting off: crops are never removed, so leaving it on would grow the
    # world during the run and every level would measure a different one.
    ./target/release/mildew-load --bots "$n" --per-connection "$PER_CONN" \
      --x "$X" --y "$Y" --spread "$SPREAD" --plant-every 0 \
      > "$OUT/load-$tag.log" 2>&1 &
    PIDS+=($!)

    wait "$sim_pid" 2>/dev/null
    cleanup; sleep 1

    sim=$(grep -h 'ticks/' "$OUT/sim-$tag.log" | tail -1)
    load=$(mid_window "$OUT/load-$tag.log" 'farmers on')
    edge=$(mid_window "$OUT/edge-$tag.log" 'clients |')

    p50s+=("$(sed -n 's/.*tick p50 \([0-9.]*\)ms.*/\1/p' <<<"$sim")")
    lates+=("$(grep -h 'run of' "$OUT/sim-$tag.log" | sed -n 's/.*late [0-9]* (\([0-9.]*\)%).*/\1/p')")
    kepts+=("$(sed -n 's/.*(\([0-9]*\)% kept).*/\1/p' <<<"$sim")")
    upos+=("$(sed -n 's/.*(\([0-9]*\)\/observer).*/\1/p' <<<"$load")")
    gaps+=("$(sed -n 's/.*gap mean \([0-9.]*\)ms.*/\1/p' <<<"$load")")
    unds+=("$(sed -n 's/.*undeliverable \([0-9]*\) .*/\1/p' <<<"$edge")")
  done

  printf '%7s  %20s  %16s  %8s  %9s  %7s  %10s\n' "$n" \
    "$(median "${p50s[@]}") ($(lo "${p50s[@]}")-$(hi "${p50s[@]}"))" \
    "$(median "${lates[@]}") ($(lo "${lates[@]}")-$(hi "${lates[@]}"))" \
    "$(median "${upos[@]}")" \
    "$(median "${gaps[@]}")" \
    "$(median "${kepts[@]}")" \
    "$(median "${unds[@]}")"
done

echo
echo "logs in $OUT"
echo "tick period at ${TICK_HZ}Hz is $(( 1000 / TICK_HZ ))ms"
