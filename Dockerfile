# syntax=docker/dockerfile:1
# CCM (Cognitive Codebase Matrix) çok aşamalı imaj.
# Amaç: Rust/protoc kurmadan tek komutla `ccm-cli` ve `ccm-mcp` çalıştırmak.

# NOT: Sürüm bilinçli olarak pinlendi (reprodüktibilite). 1.91+ gerekli:
# lancedb/lance bağımlılıkları rustc 1.91.0 talep ediyor (Cargo.lock, diğer
# ajan tarafından güncelleniyor olabilir). Güncellerken diğer ajanın
# Cargo.toml/Cargo.lock değişiklikleriyle çakışmadığından emin olun.
FROM rust:1.91-slim-bookworm AS builder

# LanceDB build'i için protoc, C gramerleri ve bazı native bağımlılıklar için
# derleyici araçları gerekir.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        protobuf-compiler \
        libprotobuf-dev \
        cmake \
        clang \
        libclang-dev \
        libssl-dev \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Bağımlılık katmanını ayrı cache'lemek için önce manifest'ler kopyalanır.
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml core/Cargo.toml
COPY cli/Cargo.toml cli/Cargo.toml
COPY mcp/Cargo.toml mcp/Cargo.toml

# Tam kaynak tek seferde derlenir; workspace üyeleri manifest'te sabittir.
COPY core/src core/src
COPY cli/src cli/src
COPY mcp/src mcp/src

# LanceDB dev bellek kullanımı yüksektir; builder aşamasında LTO'yu kapatmak
# derleme belleğini düşürür. Runtime ikilileri ayrıca `strip` kullanır.
ENV CARGO_PROFILE_RELEASE_LTO=false
ENV CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

# reqwest/rustls ile dış embedding sağlayıcısına TLS bağlantısı için CA sertifikaları.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 ccm \
    && useradd --uid 1000 --gid ccm --shell /usr/sbin/nologin --no-create-home ccm

COPY --from=builder /src/target/release/ccm-cli /usr/local/bin/ccm-cli
COPY --from=builder /src/target/release/ccm-mcp /usr/local/bin/ccm-mcp

USER ccm

ENTRYPOINT ["ccm-cli"]
CMD ["--help"]
