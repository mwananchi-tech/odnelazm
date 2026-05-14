# odnelazm

Tools for scraping, parsing, and accessing hansard data from [mzalendo.com](https://mzalendo.com) to provide structured transcripts of National Assembly and Senate sittings from the Parliament of Kenya.

## Crates

- [odnelazm](./crates/odnelazm/) - core scraper and parser
- [odnelazm-cli](./crates/odnelazm-cli/) - command line interface
- [odnelazm-mcp](./crates/odnelazm-mcp) - MCP server for LLM client access
- [odnelazm-ingest](./crates/odnelazm-ingest/) - pipeline for ingesting hansard data into PostgreSQL or any other configured database, and generating AI summaries
