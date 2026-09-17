# Stage 1: Shared toolchain (Node + Rust)
FROM node:22.23.2-bookworm-slim AS base

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    git \
    openssh-client \
    build-essential \
    pkg-config \
    libssl-dev \
    tar \
    && rm -rf /var/lib/apt/lists/*

# Install rustup under /usr/local so the non-root devcontainer user can run cargo
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH="/usr/local/cargo/bin:${PATH}"
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path

# Bake the toolchain pinned in backend/rust-toolchain.toml (the single source of
# truth) so the devcontainer user never has to download a toolchain into the
# root-owned RUSTUP_HOME. Parsing the pin at build time keeps CI images and
# local devcontainer rebuilds in sync; the docker-image CI job additionally
# asserts the match (drift guard).
COPY backend/rust-toolchain.toml /tmp/rust-toolchain.toml
RUN TOOLCHAIN_CHANNEL="$(grep -oP 'channel = "\K[^"]+' /tmp/rust-toolchain.toml)" \
    && rustup toolchain install "$TOOLCHAIN_CHANNEL" \
    && rustup default "$TOOLCHAIN_CHANNEL"

# Stage 2: Build frontend assets and compile Rust backend
FROM base AS build

WORKDIR /build

# Dependency layers come first (manifests only) so Docker layer caching keeps
# them valid across source edits. A commit that touches only backend/src/* then
# reuses the cached npm ci and cargo dependency layers, so the final cargo build
# is incremental instead of a full clean rebuild.
COPY package.json package-lock.json ./
COPY frontend/package.json frontend/package-lock.json ./frontend/
RUN npm ci && cd frontend && npm ci

COPY backend/Cargo.toml backend/Cargo.lock ./backend/
# Stub sources so cargo compiles the full dependency graph into a cached layer;
# the real sources are copied afterwards and only the backend crate recompiles.
RUN mkdir -p backend/src/bin \
    && printf 'fn main() {}\n' > backend/src/main.rs \
    && cp backend/src/main.rs backend/src/bin/openapi.rs
RUN cd backend && cargo build --release --bin backend

# Real sources last: only the backend crate (lib + bins) recompiles after this.
COPY . .

RUN npm run frontend:build

RUN cd backend && cargo build --release --bin backend

# Stage 3: Minimal runtime image with non-root user
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r yafm && useradd -r -g yafm yafm

COPY --from=build /build/backend/target/release/backend /app/backend

EXPOSE 8080

USER yafm

WORKDIR /app

CMD ["./backend"]

# Stage 4: Devcontainer for local development
FROM base AS devcontainer

WORKDIR /workspace

# Headless browser system libraries: the repo's UI verification convention
# (render and screenshot every affected width against real listing data) needs
# a headless Firefox in the container. The browser binary itself is fetched at
# provision time by the devcontainer.json postCreateCommand (kept out of the
# image so CI's devcontainer build stays slim); these are the shared system
# libraries it dlopens. libgtk-3-0 pulls the GTK stack (atk, cairo, pango,
# gdk-pixbuf, the X client libraries, xcb, dbus, fontconfig, freetype, glib);
# libasound2 and libxcb-shm0 are dlopened separately; fonts-liberation keeps
# screenshot text readable. The runtime stage builds from
# debian:bookworm-slim and never sees these.
RUN apt-get update && apt-get install -y --no-install-recommends \
    sudo \
    libgtk-3-0 \
    libasound2 \
    libxcb-shm0 \
    fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

# Belt-and-braces: keep the toolchain dirs writable by the devcontainer user
# (remoteUser: node) so an in-place rustup toolchain install or update works
# even if the baked pin ever drifts. The runtime stage is built from
# debian:bookworm-slim and never sees these directories.
RUN chmod -R a+rwX /usr/local/rustup /usr/local/cargo
