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
    cargo new --quiet --lib planar-bridge && \
    cargo new --bin spark

# cache dependencies
COPY ./server/Cargo.toml ./server/Cargo.toml
COPY ./common/Cargo.toml ./common/Cargo.toml
COPY ./spark-protocol/Cargo.toml ./spark-protocol/Cargo.toml

COPY ./common/src ./common/src
COPY ./spark-protocol/src ./spark-protocol/src

COPY ./server/.sqlx ./server/.sqlx
RUN cargo build -p blind-eternities --release --bin blind-eternities
RUN rm -r ./server/src

# copy real source
COPY ./server/src ./server/src

# build for release
RUN rm ./target/release/blind-eternities*
COPY ./server/migrations ./server/migrations
RUN find ./spark/src/ -exec touch '{}' ';'
RUN cargo build -p blind-eternities --release --bin blind-eternities
RUN cargo build -p blind-eternities --release --bin create_token

# executing image
FROM debian:buster-slim

COPY --from=build /blind-eternities/target/release/blind-eternities .
COPY --from=build /blind-eternities/target/release/create_token .

CMD ["./blind-eternities"]
