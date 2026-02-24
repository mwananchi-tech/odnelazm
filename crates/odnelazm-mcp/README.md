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

**current_get_all_members** - Fetch all members from both houses in parallel for a given parliament session. `parliament` defaults to `"13th-parliament"`.

**current_get_member_profile** - Fetch a member's full profile including biography, committees, voting patterns, and sponsored bills.

## Installation

With `cargo`

```bash
cargo install odnelazm-mcp
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
BIND_ADDRESS=0.0.0.0:8080 cargo run --bin odnelazm-mcp-web
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

Uses the HTTP/SSE transport. Claude Web does not allow localhost, so the server must be publicly hosted. The easiest way is via the provided Docker image — deploy it to any cloud host, then:

1. Go to **Settings > Connectors > Add Custom Connector**
2. Enter your SSE endpoint: `https://your-host/sse`

### Other clients

Most MCP-compatible clients (Cursor, Windsurf, Zed, etc.) support either stdio or SSE — refer to their documentation for setup details. For clients with their own tool/plugin ecosystems (e.g. ChatGPT), check their respective docs as integration steps vary.

## Context window considerations

Hansard transcripts and member profiles are large. Fetching multiple sittings or the full member list in a single conversation can easily exceed the context window of most LLMs.

- Prefer narrow date ranges when listing sittings rather than fetching all at once.
- Fetch one sitting transcript at a time when analysing debate content.
- Querying for specific information (e.g. bill mentions) across a broad date range will likely exceed the context window — narrow the scope to a specific sitting or short range first.
- `all_activity: true` and `all_bills: true` on member profiles can return a large volume of data — only use these when exhaustive detail is necessary.

For broad cross-sitting queries, consider running against a local LLM client with a much larger context window (1 million tokens or more).

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
