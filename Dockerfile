# Stage 1: Build
FROM rust:slim AS builder

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    llvm-18-dev libpolly-18-dev clang-18 lld \
    pkg-config libstdc++-14-dev libc6-dev \
    zlib1g-dev libzstd-dev && \
    rm -rf /var/lib/apt/lists/*

ENV LLVM_SYS_181_PREFIX=/usr/lib/llvm-18

WORKDIR /build
COPY . .

RUN cargo build --release --workspace

# Stage 2: Runtime
FROM debian:trixie-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    libllvm18 clang-18 lld libc6-dev && \
    rm -rf /var/lib/apt/lists/* && \
    ln -sf /usr/bin/clang-18 /usr/bin/cc

COPY --from=builder /build/target/release/nudl /usr/local/bin/nudl
COPY --from=builder /build/target/release/nudl-lsp /usr/local/bin/nudl-lsp
COPY --from=builder /build/nudl-std /usr/local/lib/nudl-std

ENV NUDL_STD_PATH=/usr/local/lib/nudl-std

ENTRYPOINT ["nudl"]
