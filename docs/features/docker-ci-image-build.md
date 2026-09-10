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
- Extend GitHub Actions CI with a `docker-image` job that builds both the `devcontainer` and runtime Docker targets on pushes and pull requests to `main`.
- Document local Docker build/run usage in `README.md`.

## UX Flow

- On every push or pull request to `main`, CI runs quality checks and then builds the `devcontainer` and runtime Docker targets.
- If either target cannot be built, the workflow fails.

## API and Data Impact

- No backend API or frontend contract changes.
- No persistence schema changes.

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
