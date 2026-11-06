# tasq-relay, the sync server. Build from the repository root:
#
#   docker build -f deploy/relay/Dockerfile -t tasq-relay .
#   docker run -p 8080:8080 -v tasq-relay:/data tasq-relay
#
# It speaks plain HTTP on 8080: put it behind HTTPS (Dokploy's Traefik,
# Caddy, nginx). Its data is one SQLite file in /data.

FROM rust:1.95-slim-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked --bin tasq-relay

FROM debian:trixie-slim
RUN useradd --system --uid 10001 relay && mkdir /data && chown relay /data
COPY --from=build /src/target/release/tasq-relay /usr/local/bin/tasq-relay
USER relay
VOLUME /data
EXPOSE 8080
ENV TASQ_RELAY_ADDR=0.0.0.0:8080 \
    TASQ_RELAY_DB=/data/relay.db
CMD ["tasq-relay"]
