# geo-clustering-server

Thin HTTP API adapter for `geo-clustering`.

Run:

```bash
cargo run -p geo-clustering-server -- --addr 127.0.0.1:3000
```

Endpoints:

- `GET /health`
- `GET /api/package`
- `GET /api/schema`
- `GET /api/operations`
- `POST /api/run`
- `POST /api/<operation-id>`
