# Feature Spec: CI Docker Image Build

## User Story

As a maintainer,
I want CI to build the production Docker image,
so container packaging issues are detected before merge and release.

## Scope

- Add a multi-stage `Dockerfile` that builds frontend assets and compiles the Rust backend binary.
- Add a shared Docker toolchain stage so local devcontainer and CI image builds use the same base image definition.
- Configure the `devcontainer` stage Docker daemon defaults for Podman-backed hosts by disabling Docker iptables management.
- Add `.dockerignore` to keep build context small and avoid local sensitive files.
- Extend GitHub Actions CI with a `docker-image` job that builds both the `devcontainer` and runtime Docker targets on pushes and pull requests to `main`, and publishes the runtime image on `main` pushes only.
- Assert the devcontainer toolchain matches `backend/rust-toolchain.toml` on `main` pushes so devcontainer and CI images stay in sync.
- Layer the Dockerfile build stage so dependency compilation is cached and only the backend crate recompiles on source change.
- Document local Docker build/run usage in `README.md`.

## UX Flow

- On every push or pull request to `main`, CI runs quality checks and then builds the `devcontainer` and runtime Docker targets.
- If either target cannot be built, the workflow fails.
- GHCR publishing and the devcontainer toolchain assertion run only on `main` pushes; pull requests still build and validate both Docker targets.
- Builds are incremental: dependency layers are cached, so a source change recompiles only the backend crate instead of forcing a clean rebuild.

## API and Data Impact

- No backend API or frontend contract changes.
- No persistence schema changes.

## Technical Notes

The build stage compiles in dependency layers so cached layers survive source changes:

1. Copy the Node manifests and lockfiles (`package.json`, `frontend/package.json`) and run `npm ci` for both root and frontend.
2. Copy the backend manifest, generate stub sources (an empty `src/main.rs` and `src/bin/openapi.rs`, no `lib.rs`), and run `cargo build --release --bin backend`. This bakes the full dependency graph into a cached `target/` layer. `rust-embed` allows the missing `frontend/dist` folder, so no frontend build is needed in this layer.
3. `COPY . .` brings in real sources. The final build recompiles only the backend crate.

`.dockerignore` excludes `backend/target`, `frontend/dist`, and `node_modules`, so `COPY . .` cannot wipe cached build artifacts.

## Security Considerations

- `.dockerignore` excludes local env files and runtime sample data to reduce accidental context leakage.
- Runtime image runs as a non-root user.
- Disabling Docker iptables in dind trades automatic NAT rule management for compatibility on constrained nested-container hosts.
- No secrets are required for image build in CI.

## Definition of Done

- `Dockerfile` targets `devcontainer` and runtime build successfully in CI.
- `.github/workflows/ci.yml` includes a Docker build job.
- `README.md` includes Docker usage instructions.
- CI checks remain green after changes.
- The devcontainer toolchain assertion passes on `main` pushes.
- Dependency layers are cached: a source change recompiles only the backend crate.
