FROM rust:1.88 AS builder

RUN apt-get update && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# The workspace's path dependency points at ../umwelt-rs, so clone it there.
RUN git clone --depth 1 https://github.com/umwelt-sim/umwelt-rs.git umwelt-rs

COPY . mildew-valley

WORKDIR /src/mildew-valley
RUN cargo build --release -p mv-sim -p mv-edge

# --- runtime ---

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/mildew-valley/target/release/mv-sim /usr/local/bin/
COPY --from=builder /src/mildew-valley/target/release/mv-edge /usr/local/bin/
