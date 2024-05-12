# inspiration: https://dev.to/rogertorres/first-steps-with-docker-rust-30oi

FROM rust:1.77.0-buster as build

# create an empty shell project
RUN USER=root cargo new --bin blind-eternities
WORKDIR /blind-eternities

# copy manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
RUN rm -r ./src && \
    cargo new --quiet --bin server && \
    cargo new --quiet --lib common && \
    cargo new --quiet --lib spark-protocol && \
    cargo new --quiet --bin planar-bridge && \
    cargo new --bin spark

# cache dependencies
COPY ./planar-bridge/Cargo.toml ./planar-bridge/Cargo.toml
COPY ./common/Cargo.toml ./common/Cargo.toml
COPY ./spark-protocol/Cargo.toml ./spark-protocol/Cargo.toml

COPY ./common/src ./common/src
COPY ./spark-protocol/src ./spark-protocol/src

RUN cargo build -p planar-bridge --release --bin planar-bridge
RUN rm -r ./planar-bridge/src

# copy real source
COPY ./planar-bridge/src ./planar-bridge/src
COPY ./planar-bridge/templates ./planar-bridge/templates

# build for release
RUN rm ./target/release/planar-bridge*
RUN find ./planar-bridge -name '*rs' -exec touch '{}' \;
RUN cargo build -p planar-bridge --release --bin planar-bridge

# executing image
FROM debian:buster-slim

COPY --from=build /blind-eternities/target/release/planar-bridge bridge
COPY ./planar-bridge/assets ./planar-bridge/assets
COPY bridgerc.toml .

CMD ["./bridge"]
