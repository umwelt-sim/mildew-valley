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
| `--per-connection` | farmers per connection. `1` loads the edge with sockets; a large number loads the region with entities |
| `--x`, `--y`, `--spread` | where they mill about, and how tightly. The default is close enough that the client's crowd badges trigger |
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
