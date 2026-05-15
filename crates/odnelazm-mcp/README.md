# odnelazm-mcp

MCP server for accessing Kenyan Parliament hansard data. Exposes tools to list sittings, fetch transcripts, and look up member profiles for use with any MCP-compatible LLM client.

Source routing is automatic: archive (`info.mzalendo.com`) is used for sittings before 2013-03-28, current (`mzalendo.com`) for those after, and both are merged in parallel for ranges that span the cutoff.

## Tools

| Tool                 | Description                                                                                                                                                              |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `list_sittings`      | List parliamentary sittings with automatic source routing. Supports date range filtering, house filtering, and pagination. Cross-era ranges merge both sources in parallel. |
| `get_sitting`        | Fetch the full transcript of a sitting including sections, contributions, and procedural notes. Source is detected automatically from the URL.                            |
| `list_members`       | List MPs by house and parliament session. Set `all: true` to fetch all pages at once.                                                                                    |
| `get_all_members`    | Fetch all members from both houses in parallel for a given parliament session. `parliament` defaults to `"13th-parliament"`.                                              |
| `get_member_profile` | Fetch a member's full profile: biography, positions, committees, voting patterns, and sponsored bills. Set `all_activity` or `all_bills` to paginate fully.               |

## Installation

```bash
cargo install --git https://github.com/mwananchi-tech/odnelazm odnelazm-mcp-local
```

For the HTTP/SSE server:

```bash
cargo install --git https://github.com/mwananchi-tech/odnelazm odnelazm-mcp-web
```

## Usage

### Stdio transport

```bash
odnelazm-mcp-local
```

### HTTP transport

```bash
odnelazm-mcp-web
```

The server listens on `127.0.0.1:8055` by default. Override with `BIND_ADDRESS`:

```bash
BIND_ADDRESS=0.0.0.0:8080 odnelazm-mcp-web
```

The SSE endpoint is available at `/sse`.

## Connecting clients

### Claude Desktop

Uses the stdio transport. Add the following to your `claude_desktop_config.json`:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "odnelazm": {
      "command": "/full/path/to/odnelazm-mcp-local"
    }
  }
}
```

Find the full path after installing:

```bash
which odnelazm-mcp-local
```

Restart Claude Desktop after saving.

### Claude Web (claude.ai)

A hosted instance is available at:

```
https://odnelazm.c12i.xyz/sse
```

To connect:

1. Go to **Settings > Connectors > Add Custom Connector**
2. Enter the SSE endpoint: `https://odnelazm.c12i.xyz/sse`

To self-host instead, build the Docker image and deploy it to any cloud host, then point your connector at your own `/sse` endpoint.

### Other clients

Most MCP-compatible clients (Cursor, Windsurf, Zed, etc.) support either stdio or SSE. For stdio, point to the `odnelazm-mcp-local` binary. For SSE, use the hosted endpoint above or your own deployment.

## Context window considerations

Hansard transcripts and member profiles are large. Fetching multiple sittings or the full member list in a single conversation can easily exceed the context window of most models.

- Prefer narrow date ranges when listing sittings rather than fetching all at once.
- Fetch one sitting transcript at a time when analysing debate content.
- `all_activity: true` and `all_bills: true` on member profiles can return a large volume of data. Only use these when exhaustive detail is necessary.

For broad cross-sitting queries, a local model with a large context window (1M+ tokens) handles it significantly better than a standard cloud model.

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
