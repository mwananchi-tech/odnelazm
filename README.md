# odnelazm

Tools for scraping, parsing, and accessing hansard data from [mzalendo.com](https://mzalendo.com) to provide structured transcripts of National Assembly and Senate sittings from the Parliament of Kenya.

## Modules

- [odnelazm](./crates/odnelazm/) - core library: scrapes and parses Hansard sittings, contributions, member profiles, and bill data from mzalendo.com into structured Rust types
- [odnelazm-cli](./crates/odnelazm-cli/) - command line tool for browsing and fetching sittings, transcripts, and member data; supports JSON, CSV, and Parquet output
- [odnelazm-mcp](./crates/odnelazm-mcp) - MCP server that exposes Hansard data as tools for LLM clients; supports stdio (Claude Desktop) and HTTP/SSE transports
- [odnelazm-ingest](./crates/odnelazm-ingest/) - data pipeline that ingests sittings into a configured PostgreSQL database and runs AI enrichment to generate summaries of bills, topics, and sittings
