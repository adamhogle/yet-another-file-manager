# ADR 0001: File-Based Runtime Configuration

## Status

Accepted

## Context

Yet Another File Manager is intended to ship as a Linux Docker container. The first feature requires a trusted shared root path and related runtime settings, but the product direction is to avoid configuring infrastructure details through the browser UI.

## Decision

Runtime configuration will be loaded on the server from a JSON or YAML file.

The application will:

1. Prefer an explicit config file path passed as the first backend CLI argument.
2. Otherwise look for `config.yaml` and `config.json` in the process current working directory.
3. Reject invalid configuration shapes during startup or request handling.
4. Keep configuration concerns entirely outside the browser UI.

The initial configuration shape is:

- `sharedRoot`: absolute path to the shared directory inside the container or mounted volume
- `showHidden`: optional boolean controlling whether hidden files are listed
- `listenAddress`: optional IP address or hostname the backend listener binds to
- `listenPort`: optional port number the backend listener binds to

Path handling for this configuration follows Linux/POSIX semantics. Windows path behavior is out of scope.

## Alternatives Considered

1. Environment variables only
2. Browser-based configuration UI
3. Hard-coded shared root inside the image

Environment variables alone become cumbersome as configuration grows. A browser-based configuration UI breaks the product direction and expands the trust surface too early. Hard-coded paths are too rigid for container deployment.

## Consequences

The application now has a clear runtime configuration contract suitable for Linux Docker deployment and Linux-based local development.

This adds a server-side configuration loader and validation path that must be tested and documented. Operators must provide a valid configuration file before the app can function.

## Security / Operations Impact

Configuration is treated as trusted operator input, but it is still validated for shape and safe path handling.

The UI must not expose configuration editing. Error messages returned to users should remain safe and should not leak sensitive host path details beyond what is necessary for operators to diagnose deployment problems.
