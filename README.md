# geo-analysis

Rust geo analysis package family extracted from `rust-packages`.

The first vertical surface is `moenarch-geo-core`. It provides:

- Rust library crate `moenarch-geo-core` with lib target `geo_core`
- CLI adapter `moenarch-geo-core-cli`
- HTTP adapter `moenarch-geo-core-server`
- WASM adapter crate `moenarch-geo-core-wasm`
- npm wrapper `@moenarch/geo-core-wasm`
- Vite app `@moenarch/geo-core-app`

## Checks

```bash
cargo test --workspace
bun install
bun run build
bun run test
cargo package -p moenarch-geo-core
```

Release workflow setup is documented in [docs/release.md](docs/release.md).
