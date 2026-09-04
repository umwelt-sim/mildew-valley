# mildew-valley
Mildew Valley, a ruthlessly casual extreme scale game built to test the Umwelt library

## Running it

Four processes, started in this order. Each needs the one before it.

```sh
nats-server                                              # 127.0.0.1:4222
cargo run --release -p mv-sim                            # region 1
cargo run --release -p mv-edge                           # 127.0.0.1:7777
cargo run --release -p mildew-gameclient --bin mildew    # the game
```

Every default lines up, so on one machine none of them need arguments. Give the
sim a moment to announce its region before starting the edge — the edge finds it
over NATS.

`wasd` walks. `tab` hides the telemetry panel. Where a crowd is too dense to
read it collapses into a badge; click one to page through who is standing there.

Keys send a direction and nothing else. The region owns every position and takes
the steps itself, so what you see of your own farmer is the region's copy coming
back, the same as everyone else's. Pressing a key shows up a round trip later,
and releasing one carries you a round trip further.

### Filling it with people

`mildew-load` runs farmers who walk about and plant, so there is something for
the engine to carry while you walk among them. It drives the same calls the game
client does, at the same pace, so a run measures the path a person exercises.

```sh
cargo run --release -p mv-load                                  # 200 farmers
cargo run --release -p mv-load -- --bots 500 --spread 6         # a real crush
cargo run --release -p mv-load -- --bots 2000 --per-connection 100
```

| | |
|---|---|
| `--bots` | how many farmers, over as many connections as it takes |
| `--per-connection` | farmers per connection. `1` loads the edge with sockets; a large number loads the region with entities. Keep it near the default: one connection cannot carry many observers' datagrams at tick rate, and past about 50 the edge starts dropping them |
| `--x`, `--y`, `--spread` | where they mill about, and how tightly. The default is close enough that the client's crowd badges trigger. A bot steers by the position the region reports back, so a run that drops those is partly blind and its crowd spreads wider than asked |
| `--plant-every` | seconds between plantings, `0` to leave the fields alone. Crops are never removed, so a long run with this on is measuring a growing world |
| `--seed` | the same seed walks the same route twice |

It prints a line a second, with rates against the wall clock so a run that falls
behind says so:

```text
mv-load: 200 farmers on 4 conns | spawned 200 | moves 4000/s |
         in 4000 packets/s 336000 updates/s | planted 130 | refused 0 | dropped 0
```

Those two incoming numbers are the interesting ones. Four thousand packets a
second is 200 farmers each hearing back once a tick. Dividing the updates by the
packets gives how many neighbours each one is being told about — 84 apiece for
the run above, which is what 200 people inside a 14 metre circle costs.

### What a region costs

`scripts/sweep.sh` runs several bot counts and prints a table. Every level runs
three times; the row gives the median and the observed range. All bots stand in
one place, so every observer can see every other entity.

```
   bots      tick_p50 (range)     late% (range)   upd/obs   gap_mean   kept%    undeliv
   1000      3.05 (2.95-3.53)     0.0 (0.0-0.0)        79       49.9      21          0
   2000      6.06 (5.65-6.21)     0.0 (0.0-0.0)        53       50.0      14          0
   4000   14.07 (12.15-14.30)     0.0 (0.0-0.3)        76       50.1      11          0
   8000   21.91 (21.59-22.74)     0.3 (0.3-1.0)        75      102.1      12          0
```

A tick has 50 ms at 20 Hz. Eight thousand entities standing on top of each
other cost 22 ms of it and miss almost no deadlines.

About a fifth of that goes on choosing each viewer's ghost set by distance.
Measured back to back, 8000 bots cost 18.2 ms picking the set by the order a
cell happened to store its occupants and 22.5 ms picking it by distance. The
first is not worth having: it hands every viewer in a crowd the same lowest
entity ids however far off they stand, and never tells a viewer that joined
late where it is.

**Read `undeliv` first.** Observations ride QUIC datagrams and the edge drops
rather than queues when a client has no room, so a load generator that cannot
drain looks exactly like a region that cannot send. Where it is nonzero, only
the sim-side columns — `tick_p50`, `late%`, `kept%` — mean anything.

It is zero at every level above, which took lowering `--per-connection` to 50.
At 250 the same 8000 bots reported 694,678 undeliverable while the region's own
cost barely moved. One connection cannot carry 250 observers' datagrams at tick
rate, and a real client is one observer on one connection.

`gap_mean` is the remaining soft spot. At 8000 it is 102.1 ms against a 50 ms
tick, so the load generator is taking delivery at half rate. The region served
all 8000 viewers on every tick with nothing late, so that number belongs to the
generator rather than to what it is measuring.

Where the crowd stands barely changes the cost. The same 4000 bots inside a
cell, on a cell corner, and near the region's edge:

```
                  tick_p50 (range)   late% (range)   examined/gather
  2100,2100    14.54 (14.06-14.57)   0.0 (0.0-0.0)               712
  2048,2048    13.20 (13.15-13.74)   0.0 (0.0-0.0)               714
   200,200     14.06 (12.33-14.08)   0.0 (0.0-0.0)               691
```

Cells are 128 m, so 2048 falls exactly on a cell corner and a crowd there is
divided among four cells. Each viewer's gather then walks buckets from four
cells instead of one, which is the extra work in that row. Standing near the
region's edge costs nothing measurable.

The sweep's `X` and `Y` default to 2100 for this reason. A crowd sitting on a
cell corner measures the grid's geometry as much as the region's cost.

Time inside `PayloadSink::send` is 2% of the tick at both 4000 and 8000, so
neither NATS nor I/O is the constraint.

These were taken with the sim, edge and load generator on one 8-core machine
over loopback, 15 seconds per level. That is a floor, not a capacity figure.

### Without a window

`mildew-probe` is the same client with no rendering: it connects, plants one
seed, and prints every callback the edge delivers. It is the quickest way to see
whether the stack is talking.

```sh
cargo run --release -p mildew-gameclient --bin mildew-probe
```

```text
mildew: connected to 127.0.0.1:7777, spawned in region 1
sent: PlantLettuce at (200, 200)
  tick 119: entity 1 at (200.000m, 200.000m, 0.000m) tag=1
  tick 159: entity 1 at (200.000m, 200.000m, 0.000m) tag=2
  tick 219: entity 1 at (200.000m, 200.000m, 0.000m) tag=3
```

The tags are lettuce growth stages, and the gaps between them are the durations
in `sim/src/prefab/crops.rs` at the simulation's 20 Hz.

### More than one of each

A region is one `mv-sim` and an edge is one `mv-edge`, and there can be many of
both. The whole arrangement fits on one machine: sims are told apart by region,
edges by the port they listen on.

```sh
nats-server

cargo run --release -p mv-sim -- --region 1
cargo run --release -p mv-sim -- --region 2

cargo run --release -p mv-edge -- --edge 127.0.0.1:7777
cargo run --release -p mv-edge -- --edge 127.0.0.1:7778

cargo run --release -p mildew-gameclient --bin mildew -- --edge 127.0.0.1:7777 --region 1
cargo run --release -p mildew-gameclient --bin mildew -- --edge 127.0.0.1:7778 --region 2
```

Sims need no port of their own — they reach the world through NATS, so only the
edges listen and only they have to differ.

Entity ids are per-region, so both clients above are told about an `entity 0`
and they are not the same entity.

Spreading this across machines changes nothing structural: pass each process the
broker's real address with `--nats`, and point clients at an edge's address
instead of `127.0.0.1`.

### Sprites

The client runs without them, drawing flat stand-in shapes and saying so on
screen. For the real thing see [gameclient/ASSETS.md](gameclient/ASSETS.md) —
the art is licensed and not committed to this repository.

## Layout

| Crate | Binary | What it is |
|---|---|---|
| `sim` | `mv-sim` | Owns a region, runs the tick loop, decides what happens |
| `edge` | `mv-edge` | Relays between clients and regions |
| `gameclient` | `mildew`, `mildew-probe` | The player-facing client, and a headless one |
| `loadgen` | `mildew-load` | Fills a region with farmers. No renderer, so it builds on a headless box |
| `common` | | Commands, tags, pacing, and the network setup they all share |
