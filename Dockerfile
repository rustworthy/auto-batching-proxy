FROM docker.io/rust:1-slim-bookworm AS build

WORKDIR /build

COPY . .

RUN --mount=type=cache,target=/build/target \
  --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/usr/local/cargo/git \
  set -eux; \
  cargo build --release; \
  objcopy --compress-debug-sections target/release/auto-batching-proxy ./main

################################################################################
FROM docker.io/debian:bookworm-slim

WORKDIR /app

COPY --from=build /build/main ./

CMD ["./main"]
