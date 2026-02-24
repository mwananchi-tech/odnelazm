# odnelazm-mcp

MCP server for accessing Kenyan Parliament hansard data. Provides tools to list parliamentary sittings, fetch transcripts, and look up speaker details.

## Tools

### Archive (info.mzalendo.com — pre-2013)

**archive_list_sittings** - List archived sittings. Filter by date range, house, limit, and offset.

**archive_get_sitting** - Fetch the full transcript of an archived sitting. Optionally fetch speaker profiles inline.

**archive_get_person** - Fetch a speaker's archived profile including party, constituency, and contact info.

### Current (mzalendo.com — 2013 to present)

**current_list_sittings** - List recent sittings. Filter by house. Use `all: true` to fetch all pages at once.

**current_get_sitting** - Fetch the full transcript of a current sitting.

**current_list_members** - List MPs by house and parliament session. Use `all: true` to fetch all pages at once.

**current_get_member_profile** - Fetch a member's full profile including biography, committees, voting patterns, and sponsored bills.

## Installation

With `cargo`

```bash
cargo install odnelazm-mcp
```

## Usage

### Stdio transport

```bash
odnelazm-mcp
```

### HTTP transport

```bash
odnelazm-mcp-web
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
