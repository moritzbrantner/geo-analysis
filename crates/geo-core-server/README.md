# geo-core-server

Thin HTTP API adapter for `geo-core`.

Run:

```bash
cargo run -p moenarch-geo-core-server -- --addr 127.0.0.1:3000
```

Endpoints:

- `GET /health`
- `GET /api/package`
- `GET /api/schema`
- `GET /api/operations`
- `POST /api/run`
- `POST /api/<operation-id>`
