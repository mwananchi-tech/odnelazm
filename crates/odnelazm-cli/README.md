# odnelazm-cli

Run the `odnelazm` web scraper and parser from the command line.

## Installation

```bash
cargo install odnelazm-cli
```

## Global flags

| Flag              | Description                                                                           |
| ----------------- | ------------------------------------------------------------------------------------- |
| `-l, --log-level` | Set log verbosity: `off`, `error`, `warn`, `info`, `debug`, `trace` (default: `info`) |

---

## archive

Archival Hansard data from `info.mzalendo.com`.

### archive list

Browse available sittings with optional filtering and pagination.

| Flag                                    | Description                            |
| --------------------------------------- | -------------------------------------- |
| `--start-date YYYY-MM-DD`               | Filter sittings from this date onwards |
| `--end-date YYYY-MM-DD`                 | Filter sittings up to this date        |
| `--house senate\|national_assembly\|na` | Filter by house                        |
| `--limit N`                             | Maximum number of results to return    |
| `--offset N`                            | Number of results to skip              |
| `-o, --output text\|json`               | Output format (default: text)          |

```bash
odnelazm archive list
odnelazm archive list --house senate --start-date 2020-01-01 --end-date 2020-12-31 --limit 10
odnelazm archive list -o json | jq '.[] | select(.house == "senate")'
```

### archive sitting

Fetch the full transcript of a sitting including sections, contributions, and procedural notes.

| Flag                      | Description                                                     |
| ------------------------- | --------------------------------------------------------------- |
| `--fetch-speakers`        | Fetch full profile for each speaker (makes additional requests) |
| `-o, --output text\|json` | Output format (default: text)                                   |

```bash
odnelazm archive sitting https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00
odnelazm archive sitting https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00 --fetch-speakers -o json
```

---

## current

Up to date Hansard data from `mzalendo.com/democracy-tools`.

### current sittings

List available sittings, paged or all at once.

| Flag                                    | Description                                     |
| --------------------------------------- | ----------------------------------------------- |
| `--page N`                              | Page number to fetch (default: 1)               |
| `--all`                                 | Fetch all pages at once (conflicts with --page) |
| `--house senate\|national_assembly\|na` | Filter by house                                 |
| `-o, --output text\|json`               | Output format (default: text)                   |

```bash
odnelazm current sittings
odnelazm current sittings --page 2 --house senate
odnelazm current sittings --all -o json
```

### current sitting

Fetch the full transcript of a sitting.

| Flag                      | Description                   |
| ------------------------- | ----------------------------- |
| `-o, --output text\|json` | Output format (default: text) |

```bash
odnelazm current sitting thursday-12th-february-2026-afternoon-sitting-2438
odnelazm current sitting https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2438/ -o json
```

### current members

List members of parliament.

| Flag                      | Description                                     |
| ------------------------- | ----------------------------------------------- |
| `--page N`                | Page number (default: 1)                        |
| `--all`                   | Fetch all pages at once (conflicts with --page) |
| `-o, --output text\|json` | Output format (default: text)                   |

```bash
odnelazm current members na 13th-parliament
odnelazm current members senate 13th-parliament --page 2 -o json
odnelazm current members na 13th-parliament --all -o json
```

### current profile

Fetch a member's full profile including speeches, bills, and voting record.

| Flag                      | Description                               |
| ------------------------- | ----------------------------------------- |
| `--all-activity`          | Fetch all pages of parliamentary activity |
| `--all-bills`             | Fetch all pages of sponsored bills        |
| `-o, --output text\|json` | Output format (default: text)             |

```bash
odnelazm current profile https://mzalendo.com/mps-performance/national-assembly/13th-parliament/boss-gladys-jepkosgei/
odnelazm current profile https://mzalendo.com/mps-performance/national-assembly/13th-parliament/boss-gladys-jepkosgei/ --all-activity --all-bills -o json
```
