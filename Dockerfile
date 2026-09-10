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

# Stage 2: Build frontend assets and compile Rust backend
FROM base AS build

WORKDIR /build

COPY . .

RUN npm ci && cd frontend && npm ci

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
