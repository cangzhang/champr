FROM rust:1.86-bookworm AS builder
WORKDIR /app

COPY . .
RUN cargo build --release -p server

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server /usr/local/bin/server

ENV SERVER_ADDR=0.0.0.0:3030
ENV DATABASE_URL=sqlite://data/champr.db

EXPOSE 3030

CMD ["server"]
