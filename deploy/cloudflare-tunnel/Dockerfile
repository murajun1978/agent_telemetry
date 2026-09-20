FROM rust:1.98-bookworm AS build

WORKDIR /src
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/agent-telemetry /usr/local/bin/atel

USER 65532:65532
ENTRYPOINT ["/usr/local/bin/atel"]
