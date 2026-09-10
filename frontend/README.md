# Frontend

Vue 3 SPA for Yet Another File Manager, built with Vite.

Consumes the Rust backend API via an auto-generated OpenAPI client at `src/lib/api/generated/client.ts`. The client is TypeScript with zod-validated responses — every generated fetch function parses its response through a zod schema before returning. The client is regenerated from `api/openapi.yaml` — see `scripts/generate-openapi-client.mjs` and the `contract:generate` npm script at the repo root.

Built frontend assets are embedded into the Rust binary at compile time via `rust-embed`.
