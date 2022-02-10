####################################################################################################
## Builder
####################################################################################################
FROM messense/rust-musl-cross:aarch64-musl AS builder

RUN rustup target add aarch64-unknown-linux-musl
RUN apt update && apt install -y musl-tools musl-dev
RUN update-ca-certificates

# Create appuser
ENV USER=mendess
ENV UID=10001

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    "${USER}"


WORKDIR /blind-eternities

COPY ./ .

ENV SQLX_OFFLINE=true
RUN cargo build --target aarch64-unknown-linux-musl --release

####################################################################################################
## Final image
####################################################################################################
FROM alpine:latest

# Import from builder.
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group

WORKDIR /blind-eternities

# Copy our build
COPY --from=builder /blind-eternities/target/aarch64-unknown-linux-musl/release/blind-eternities ./
COPY --from=builder /blind-eternities/target/aarch64-unknown-linux-musl/release/create_token ./

# Use an unprivileged user.
USER mendess:mendess

CMD ["/blind-eternities/blind-eternities"]
