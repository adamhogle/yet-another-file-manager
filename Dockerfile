#syntax=docker/dockerfile:1.7

FROM mcr.microsoft.com/devcontainers/javascript-node:22-bookworm AS toolchain

ENV CARGO_HOME=/usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH=/usr/local/cargo/bin:${PATH}

RUN apt-get update \
	&& apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev ca-certificates curl \
	&& rm -rf /var/lib/apt/lists/* \
	&& curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable

FROM toolchain AS frontend-builder
WORKDIR /workspace/frontend

COPY frontend/package*.json ./
RUN npm ci

COPY frontend/ ./
RUN npm run build

FROM toolchain AS backend-builder
WORKDIR /workspace

COPY backend/Cargo.toml backend/Cargo.lock ./backend/
COPY backend/src ./backend/src
COPY backend/tests ./backend/tests
COPY --from=frontend-builder /workspace/frontend/dist ./frontend/dist

RUN cd backend && cargo build --release --bin backend

FROM toolchain AS devcontainer
RUN apt-get update \
	&& apt-get install -y --no-install-recommends docker.io gh \
	&& rm -rf /var/lib/apt/lists/* \
	&& mkdir -p /etc/docker \
	&& printf '{"iptables": false}\n' > /etc/docker/daemon.json
USER node

FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 --create-home --home-dir /app appuser

WORKDIR /app
COPY --from=backend-builder /workspace/backend/target/release/backend ./backend
COPY config/yafm.config.example.yaml ./config/config.yaml

RUN chown -R appuser:appuser /app
USER appuser

EXPOSE 8080
ENTRYPOINT ["/app/backend"]
CMD ["/app/config/config.yaml"]
