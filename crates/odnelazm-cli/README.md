# odnelazm-cli

Run the `odnelazm` web scraper and parser from the command line. Source routing is automatic: archive (`info.mzalendo.com`) is used for sittings before 2013-03-28, current (`mzalendo.com`) for those after, and both are merged in parallel for ranges that span the cutoff.

## Installation

```bash
cargo install --git https://github.com/mwananchi-tech/odnelazm odnelazm-cli
```

## Global flags

| Flag              | Description                                                                            |
| ----------------- | -------------------------------------------------------------------------------------- |
| `-l, --log-level` | Set log verbosity: `off`, `error`, `warn`, `info`, `debug`, `trace` (default: `info`) |

---

## sittings

List parliamentary sittings. Routing is determined automatically by date range.

| Scenario                          | Source used                          |
| --------------------------------- | ------------------------------------ |
| No dates                          | Current source, paged                |
| `--end-date` before 2013-03-28    | Archive only                         |
| `--start-date` on/after 2013-03-28 | Current only, paged                 |
| Range spans the cutoff            | Both sources merged in parallel; `--page` and `--all` are ignored, use `--limit` / `--offset` |

| Flag                                    | Description                                                        |
| --------------------------------------- | ------------------------------------------------------------------ |
| `--start-date YYYY-MM-DD`               | Start of date range                                                |
| `--end-date YYYY-MM-DD`                 | End of date range                                                  |
| `--house senate\|national_assembly\|na` | Filter by house                                                    |
| `--page N`                              | Page to fetch from the current source (default: 1)                |
| `--all`                                 | Fetch all pages at once (current source; conflicts with `--page`)  |
| `--limit N`                             | Maximum results to return, applied after merging                   |
| `--offset N`                            | Results to skip, applied after merging                             |
| `-o, --output json\|csv\|parquet`       | Output format (default: `json`)                                    |

```bash
# Recent sittings (current source, page 1)
odnelazm sittings

# Archive sittings from 2010
odnelazm sittings --start-date 2010-01-01 --end-date 2010-12-31

# Cross-era range: archive and current merged
odnelazm sittings --start-date 2012-01-01 --end-date 2014-12-31 --limit 50

# Filter by house, all pages
odnelazm sittings --house senate --all -o json
```

---

## sitting

Fetch the full transcript of a sitting. The source is detected automatically from the URL.

| Flag                              | Description                     |
| --------------------------------- | ------------------------------- |
| `<url_or_slug>`                   | Full URL or slug of the sitting |
| `-o, --output json\|csv\|parquet` | Output format (default: `json`) |

```bash
# Current sitting by slug
odnelazm sitting thursday-12th-february-2026-afternoon-sitting-2438

# Archive sitting by URL
odnelazm sitting https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00 -o json
```

---

## members

List members of parliament (current source only).

| Flag                              | Description                                       |
| --------------------------------- | ------------------------------------------------- |
| `<house>`                         | `senate`, `national_assembly`, or `na`            |
| `<parliament>`                    | Parliament session, e.g. `13th-parliament`        |
| `--page N`                        | Page number (default: 1)                          |
| `--all`                           | Fetch all pages at once (conflicts with `--page`) |
| `-o, --output json\|csv\|parquet` | Output format (default: `json`)                   |

```bash
odnelazm members na 13th-parliament
odnelazm members senate 13th-parliament --all -o json
odnelazm members na 12th-parliament --page 2 -o csv
```

---

## all-members

Fetch all members from both houses in parallel for a given parliament session (current source only).

| Flag                              | Description                                     |
| --------------------------------- | ----------------------------------------------- |
| `[parliament]`                    | Parliament session (default: `13th-parliament`) |
| `-o, --output json\|csv\|parquet` | Output format (default: `json`)                 |

```bash
odnelazm all-members
odnelazm all-members 12th-parliament -o json
```

---

## profile

Fetch a member's full profile including speeches, bills, and voting record (current source only).

| Flag                              | Description                               |
| --------------------------------- | ----------------------------------------- |
| `<url_or_slug>`                   | Full URL or slug of the member profile    |
| `--all-activity`                  | Fetch all pages of parliamentary activity |
| `--all-bills`                     | Fetch all pages of sponsored bills        |
| `-o, --output json\|csv\|parquet` | Output format (default: `json`)           |

```bash
odnelazm profile https://mzalendo.com/mps-performance/national-assembly/13th-parliament/boss-gladys-jepkosgei/
odnelazm profile https://mzalendo.com/mps-performance/national-assembly/13th-parliament/boss-gladys-jepkosgei/ --all-activity --all-bills -o json
```
