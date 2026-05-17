# odnelazm-ingest

Ingestion pipeline for [odnelazm](https://github.com/mwananchi-tech/odnelazm). Scrapes parliamentary sittings, stores them in PostgreSQL, and runs AI enrichment to generate summaries of bills, topics, and sittings.

Used as the data backend for [Bunge Hub](https://github.com/mwananchi-tech/bunge-hub).

## Requirements

- Rust (stable)
- PostgreSQL 14+
- [LM Studio](https://lmstudio.ai) with a loaded model (for the `enrich` subcommand)

## Setup

Start a PostgreSQL instance. The default connection string is `postgres://odnelazm:odnelazm@localhost:5432/odnelazm`. You can override it with `--database-url` or the `DATABASE_URL` environment variable.

Migrations run automatically on first use.

## `odnelazm-pipeline`

A single binary with two subcommands: `ingest` and `enrich`.

```bash
cargo build -p odnelazm-ingest --release
./target/release/odnelazm-pipeline --help
```

### Global flags

| Flag             | Description                                                                | Default                                                |
| ---------------- | -------------------------------------------------------------------------- | ------------------------------------------------------ |
| `--database-url` | PostgreSQL connection string                                               | `postgres://odnelazm:odnelazm@localhost:5432/odnelazm` |
| `--metrics-url`  | Prometheus pushgateway URL. When set, metrics are pushed after each batch. |                                                        |

---

## ingest

Scrapes parliamentary sittings and member profiles from mzalendo.com and stores them in the database.

```bash
odnelazm-pipeline ingest [OPTIONS]
```

| Flag               | Description                                       | Default           |
| ------------------ | ------------------------------------------------- | ----------------- |
| `--start-date`     | Only ingest sittings from this date (YYYY-MM-DD)  |                   |
| `--end-date`       | Only ingest sittings up to this date (YYYY-MM-DD) |                   |
| `--concurrency`    | Number of concurrent scrape requests              | `4`               |
| `--parliament`     | Parliament session to import members from         | `13th-parliament` |
| `--skip-sittings`  | Skip scraping sittings                            |                   |
| `--skip-members`   | Skip importing members                            |                   |
| `--enrich-members` | Fetch and store individual member profile pages   |                   |
| `--enrich-batch`   | Run AI speaker summaries after ingest (0 to skip) | `0`               |

```bash
# Ingest everything
odnelazm-pipeline ingest

# Ingest a specific date range, skip member import
odnelazm-pipeline ingest --start-date 2026-01-01 --end-date 2026-03-31 --skip-members

# Ingest sittings and also fetch member profile pages
odnelazm-pipeline ingest --enrich-members

# Ingest with a custom database
odnelazm-pipeline --database-url postgres://user:pass@host/db ingest
```

---

## enrich

Generates AI summaries using a locally running LM Studio model. Requires a model to be loaded and the server running at the specified URL.

```bash
odnelazm-pipeline enrich <TARGET> [OPTIONS]
```

**Targets**

| Target           | What it summarises                                                                    |
| ---------------- | ------------------------------------------------------------------------------------- |
| `bill-mentions`  | Each bill's appearance in a sitting: what was argued and the outcome                  |
| `bill-journeys`  | A bill's full legislative journey across all sittings                                 |
| `bill-speakers`  | Each speaker's individual contributions to a bill debate                              |
| `topics`         | Each topic's appearance in a sitting: all contributions across speakers, full context |
| `topic-speakers` | Each speaker's individual contributions to a question or statement topic              |
| `sittings`       | Full structured summary of a sitting                                                  |

| Flag            | Description                                 | Default                 |
| --------------- | ------------------------------------------- | ----------------------- |
| `--llm-url`     | LM Studio base URL                          | `http://127.0.0.1:1234` |
| `--model`       | Model identifier as shown in LM Studio      | `google/gemma-4-e4b`    |
| `--temperature` | Sampling temperature                        | `0.3`                   |
| `--batch`       | Number of items to fetch per database query | `10`                    |
| `--concurrency` | Number of concurrent LLM requests           | `4`                     |

```bash
# Summarise all pending bill mentions
odnelazm-pipeline enrich bill-mentions --model qwen/qwen3.5-9b

# Generate bill journey summaries with lower concurrency
odnelazm-pipeline enrich bill-journeys --model qwen/qwen3.5-9b --concurrency 2

# Summarise sittings (large context, concurrency capped at 2 internally)
odnelazm-pipeline enrich sittings --model qwen/qwen3.5-9b --batch 5

# Summarise topics (full transcript context, all speakers combined)
odnelazm-pipeline enrich topics --model qwen/qwen3.5-9b --concurrency 2

# Point at a different LM Studio instance
odnelazm-pipeline enrich topic-speakers --llm-url http://192.168.1.10:1234 --model some/model
```

Each enrichment run is idempotent. Items that already have a summary are skipped.

## Metrics

The pipeline can push metrics to a Prometheus pushgateway after each batch. This is optional — omitting `--metrics-url` disables it with no effect on ingestion.

```bash
# With metrics enabled
odnelazm-pipeline --metrics-url http://localhost:9091 enrich bill-mentions --model qwen/qwen3.5-9b
```

A local monitoring stack (Prometheus, pushgateway, Grafana) is available via Docker Compose from the repo root:

```bash
make metrics-up   # start
make metrics-down # stop
```

The Makefile at the repo root also provides convenience targets for running the pipeline with metrics wired in:

```bash
make enrich-bill-mentions MODEL=qwen/qwen3.5-9b METRICS_URL=http://localhost:9091
make enrich-all           MODEL=qwen/qwen3.5-9b METRICS_URL=http://localhost:9091
```
