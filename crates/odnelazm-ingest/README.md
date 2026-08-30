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

| Flag                | Description                                       | Default           |
| ------------------- | ------------------------------------------------- | ----------------- |
| `--start-date`      | Only ingest sittings from this date (YYYY-MM-DD)  |                   |
| `--end-date`        | Only ingest sittings up to this date (YYYY-MM-DD) |                   |
| `--concurrency`     | Number of concurrent scrape requests              | `4`               |
| `--parliament`      | Parliament session to import members from         | `13th-parliament` |
| `--skip-sittings`   | Skip scraping sittings                            |                   |
| `--skip-members`    | Skip importing members                            |                   |
| `--import-profiles` | Fetch and store individual member profile pages   |                   |
| `--dry-run`         | Resolve and report outcomes without data writes   |                   |

```bash
# Ingest everything
odnelazm-pipeline ingest

# Ingest a specific date range, skip member import
odnelazm-pipeline ingest --start-date 2026-01-01 --end-date 2026-03-31 --skip-members

# Ingest sittings and also fetch member profile pages
odnelazm-pipeline ingest --import-profiles

# Ingest with a custom database
odnelazm-pipeline --database-url postgres://user:pass@host/db ingest
```

## Canonical data migration runbook

This runbook covers the source-identity and reconciliation migration shared with
Bunge Hub.

### Architecture and invariants

- `sittings` and `members` remain canonical. Source-specific identity lives in
  `sitting_sources` and `member_sources`; aliases never create a second canonical
  row when a source external key or normalized URL resolves to an existing row.
- A sitting refresh is transactional. Its active speakers, bill mentions,
  contributors, and topics exactly match the latest successful extraction.
  Missing derived rows are retained with `active = false`, not deleted.
- Existing AI summary text is never cleared by ingestion. Changed summary input
  sets `*_stale_at`; enrichment includes missing or stale summaries, replaces the
  text, records the current input hash, and clears `*_stale_at`. An inactive row
  can retain its summary for audit and recovery but is excluded by compatible
  application queries.
- Every completed `ingestion_runs` row satisfies `processed_count =
succeeded_count + skipped_count + failed_count` and `processed_count <=
discovered_count`.

### Local rehearsal

Start the local database from Bunge Hub and use the explicit local URL for both
repositories:

```bash
docker compose -f ../bunge-hub/compose.yml up -d --wait postgres
export DATABASE_URL='postgres://odnelazm:odnelazm@localhost:5432/odnelazm'
```

Requirements are Docker, a compatible PostgreSQL client (`psql`, `pg_dump`, and
`pg_restore`), enough free space for the dump and restored database, and read
access to the database being backed up. Create and verify a backup before schema
or ingestion work:

```bash
export DATABASE_URL='postgres://odnelazm:odnelazm@localhost:5432/odnelazm'
export BACKUP_FILE="odnelazm-$(date +%Y%m%d-%H%M%S).dump"
pg_dump --format=custom --no-owner --no-acl --file="$BACKUP_FILE" "$DATABASE_URL"
shasum -a 256 "$BACKUP_FILE" > "$BACKUP_FILE.sha256"
shasum -a 256 -c "$BACKUP_FILE.sha256"
pg_restore --list "$BACKUP_FILE" >/dev/null
```

Apply the additive migrations in order. Do not run `0016` or `0017` without
`0015`:

```bash
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f crates/odnelazm-ingest/migrations/0015_source_identity_foundation.sql
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f crates/odnelazm-ingest/migrations/0016_derived_reconciliation.sql
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f crates/odnelazm-ingest/migrations/0017_ingestion_run_accounting.sql
```

The pipeline also applies all migrations at startup. Rehearse a bounded range
first, then perform the same ingest without `--dry-run`:

```bash
export START_DATE='2026-01-01'
export END_DATE='2026-03-31'
cargo run -p odnelazm-ingest --bin odnelazm-pipeline -- \
  --database-url "$DATABASE_URL" ingest \
  --start-date "$START_DATE" --end-date "$END_DATE" --skip-members --dry-run
cargo run -p odnelazm-ingest --bin odnelazm-pipeline -- \
  --database-url "$DATABASE_URL" ingest \
  --start-date "$START_DATE" --end-date "$END_DATE" --skip-members
```

Dry-run fetches, parses, and resolves records but does not write canonical or
derived data. It does apply migrations and write its operational
`ingestion_runs` audit rows. This local rehearsal uses only the explicit local
`DATABASE_URL`; production credentials and production writes are not used.

### Run interpretation

Inspect recent runs with:

```sql
SELECT ir.id, ds.source_key, ir.status,
       ir.discovered_count, ir.processed_count, ir.succeeded_count,
       ir.skipped_count, ir.failed_count, ir.error_message,
       ir.error_metadata, ir.started_at, ir.finished_at
FROM ingestion_runs ir
JOIN data_sources ds ON ds.id = ir.data_source_id
ORDER BY ir.created_at DESC
LIMIT 20;
```

`succeeded` means every discovered item reached a supported terminal outcome;
already-ingested items count as skipped and are reported by
`error_metadata.already_ingested_count`. `partial` means at least one fetch,
parse, store, operation, or unsupported-item outcome prevented completeness.
`failed` means discovery failed or every discovered item failed. A `running` row
with no `finished_at` indicates interruption and needs operator investigation.

An external PDF listing is recorded as skipped with reason
`unsupported_external_pdf`, but makes the run `partial` and the command fail.
The pipeline has no PDF parser that can prove the PDF produced the same complete
canonical extraction as a transcript, so it fails closed instead of silently
claiming a complete ingest.

### Verification SQL

Capture these results before and after the schema change and real ingest. The AI
summary checksums should not change during ingestion; count or checksum changes
require investigation.

```sql
-- Canonical and source-alias counts.
SELECT 'sittings' AS relation, count(*) AS rows FROM sittings
UNION ALL SELECT 'members', count(*) FROM members
UNION ALL SELECT 'bills', count(*) FROM bills
UNION ALL SELECT 'speakers', count(*) FROM speakers
UNION ALL SELECT 'sitting_sources', count(*) FROM sitting_sources
UNION ALL SELECT 'member_sources', count(*) FROM member_sources
ORDER BY relation;

SELECT ds.source_key, 'sitting' AS entity, count(*) AS aliases,
       count(DISTINCT ss.sitting_id) AS canonical_rows
FROM sitting_sources ss
JOIN data_sources ds ON ds.id = ss.data_source_id
GROUP BY ds.source_key
UNION ALL
SELECT ds.source_key, 'member', count(*), count(DISTINCT ms.member_id)
FROM member_sources ms
JOIN data_sources ds ON ds.id = ms.data_source_id
GROUP BY ds.source_key
ORDER BY source_key, entity;

-- Non-null AI summary counts and deterministic content checksums.
SELECT 'sittings.generated_summary' AS summary_type, count(*) AS non_null,
       md5(coalesce(string_agg(id::text || chr(31) || generated_summary,
                               chr(30) ORDER BY id), '')) AS checksum
FROM sittings WHERE generated_summary IS NOT NULL
UNION ALL
SELECT 'bills.summary', count(*),
       md5(coalesce(string_agg(id::text || chr(31) || summary,
                               chr(30) ORDER BY id), ''))
FROM bills WHERE summary IS NOT NULL
UNION ALL
SELECT 'bill_mentions.summary', count(*),
       md5(coalesce(string_agg(id::text || chr(31) || summary,
                               chr(30) ORDER BY id), ''))
FROM bill_mentions WHERE summary IS NOT NULL
UNION ALL
SELECT 'bill_mention_speakers.summary', count(*),
       md5(coalesce(string_agg(bill_mention_id::text || ':' || speaker_id::text ||
                               chr(31) || summary, chr(30)
                               ORDER BY bill_mention_id, speaker_id), ''))
FROM bill_mention_speakers WHERE summary IS NOT NULL
UNION ALL
SELECT 'topics.summary', count(*),
       md5(coalesce(string_agg(id::text || chr(31) || summary,
                               chr(30) ORDER BY id), ''))
FROM topics WHERE summary IS NOT NULL
UNION ALL
SELECT 'topic_speakers.summary', count(*),
       md5(coalesce(string_agg(topic_id::text || ':' || speaker_id::text ||
                               chr(31) || summary, chr(30)
                               ORDER BY topic_id, speaker_id), ''))
FROM topic_speakers WHERE summary IS NOT NULL
ORDER BY summary_type;

-- Active/inactive projections and stale/preserved summaries.
SELECT 'sitting_speakers' AS relation,
       count(*) FILTER (WHERE active) AS active,
       count(*) FILTER (WHERE NOT active) AS inactive,
       0::bigint AS stale
FROM sitting_speakers
UNION ALL
SELECT 'bill_mentions', count(*) FILTER (WHERE active),
       count(*) FILTER (WHERE NOT active),
       count(*) FILTER (WHERE summary_stale_at IS NOT NULL)
FROM bill_mentions
UNION ALL
SELECT 'bill_mention_speakers', count(*) FILTER (WHERE active),
       count(*) FILTER (WHERE NOT active),
       count(*) FILTER (WHERE summary_stale_at IS NOT NULL)
FROM bill_mention_speakers
UNION ALL
SELECT 'topics', count(*) FILTER (WHERE active),
       count(*) FILTER (WHERE NOT active),
       count(*) FILTER (WHERE summary_stale_at IS NOT NULL)
FROM topics
UNION ALL
SELECT 'topic_speakers', count(*) FILTER (WHERE active),
       count(*) FILTER (WHERE NOT active),
       count(*) FILTER (WHERE summary_stale_at IS NOT NULL)
FROM topic_speakers
UNION ALL
SELECT 'bills', count(*), 0,
       count(*) FILTER (WHERE summary_stale_at IS NOT NULL)
FROM bills
UNION ALL
SELECT 'sittings', count(*), 0,
       count(*) FILTER (WHERE generated_summary_stale_at IS NOT NULL)
FROM sittings
ORDER BY relation;
```

### Production rollout and rollback

Roll out in this order:

1. Take a restorable backup, save its SHA-256 checksum, verify both the checksum
   and `pg_restore --list`, and capture the verification SQL baseline.
2. Apply `0015`, `0016`, then `0017` with `ON_ERROR_STOP`.
3. Deploy the compatible Bunge Hub reader and this compatible pipeline writer.
4. Run the bounded dry-run and inspect its `ingestion_runs` rows.
5. Run the real bounded ingest.
6. Re-run all verification SQL, inspect every `partial` or `failed` run, and
   smoke-test canonical sitting URLs and legacy redirects.

For rollback, stop the writer first and route traffic to the last release known
to understand `active` rows and source aliases. Keep migrations `0015`-`0017`
and canonical data in place: they are additive. Do not drop or wholesale restore
`sittings`, `bills`, `bill_mentions`, `bill_mention_speakers`, `topics`, or
`topic_speakers`, because those canonical rows may contain preserved summaries.
Use the verified backup in a separate database for comparison or targeted row
recovery. A reader predating `active` filters is safe only before real
reconciliation; after ingestion it can expose inactive historical projections.

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

The pipeline can push metrics to a Prometheus pushgateway after each batch. This is optional. Omitting `--metrics-url` disables it with no effect on ingestion.

```bash
# With metrics enabled
odnelazm-pipeline --metrics-url http://localhost:9091 enrich bill-mentions --model qwen/qwen3.5-9b
```

### Local monitoring stack

A local stack (Prometheus, pushgateway, Grafana) is available via Docker Compose from the repo root. Grafana comes pre-configured with the Prometheus datasource and the enrichment dashboard, so no manual setup is required.

**Requirements:** Docker (or OrbStack)

**Start the stack:**

```bash
make metrics-up
```

This starts three services:

- Pushgateway at `http://localhost:9091`: receives metric pushes from the pipeline
- Prometheus at `http://localhost:9090`: scrapes pushgateway every 15 seconds
- Grafana at `http://localhost:3001`: dashboards, no login required

Open `http://localhost:3001` and navigate to **Dashboards > odnelazm > odnelazm-ingest** to view the enrichment dashboard.

**Stop the stack:**

```bash
make metrics-down
```

Data is persisted in Docker volumes and restored automatically on the next `make metrics-up`.

The Makefile also provides convenience targets with metrics wired in:

```bash
make enrich-bill-mentions MODEL=qwen/qwen3.5-9b METRICS_URL=http://localhost:9091
make enrich-all           MODEL=qwen/qwen3.5-9b METRICS_URL=http://localhost:9091
```

### Available metrics

| Metric                            | Type    | Description                                               |
| --------------------------------- | ------- | --------------------------------------------------------- |
| `summaries_written`               | counter | Total summaries written, labelled by `target` and `model` |
| `summary_failures`                | counter | Total LLM call failures, labelled by `target`             |
| `llm_tokens_per_second`           | gauge   | Inference throughput of the most recent call              |
| `llm_input_tokens`                | counter | Total input tokens fed to the model                       |
| `llm_output_tokens`               | counter | Total output tokens generated                             |
| `llm_reasoning_tokens`            | counter | Total reasoning (chain-of-thought) tokens generated       |
| `llm_time_to_first_token_seconds` | gauge   | Latency before the model starts generating, in seconds    |
