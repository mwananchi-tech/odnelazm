# odnelazm-mcp

MCP server for accessing Kenyan Parliament hansard data. Provides tools to list parliamentary sittings, fetch transcripts, and look up speaker details.

## Tools

**list_sittings** - List available parliamentary sittings with optional filtering and pagination.

**get_sitting** - Fetch the full transcript of a sitting including sections, contributions and procedural notes.

**get_person** - Fetch speaker details from person profile pages.

## Usage

### Stdio transport

```bash
cargo run --bin odnelazm-mcp
```

### HTTP transport

```bash
cargo run --bin odnelazm-mcp-web
```

The server listens on `127.0.0.1:8055` by default. Override with `BIND_ADDRESS`:

```bash
BIND_ADDRESS=0.0.0.0:8080 cargo run --bin odnelazm-mcp-web
```

The SSE endpoint is available at `/sse`.

## Configuration

| Variable       | Default          | Description                  |
| -------------- | ---------------- | ---------------------------- |
| `BIND_ADDRESS` | `127.0.0.1:8055` | Bind address for HTTP server |
| `RUST_LOG`     | `debug`          | Log level                    |

## Docker

```bash
docker build -t odnelazm-mcp .
docker run -p 8055:8055 -e BIND_ADDRESS=0.0.0.0:8055 odnelazm-mcp
```
