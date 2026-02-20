# odnelazm-cli

Run the `odnelazm` web scraper and parser from the command line

## Installation

Install with `cargo`

```bash
cargo install odnelazm-cli
```

## CLI Features

### List sittings

Browse available hansard sittings with optional filtering and pagination.

| Flag                      | Description                            |
| ------------------------- | -------------------------------------- |
| `--start-date YYYY-MM-DD` | Filter sittings from this date onwards |
| `--end-date YYYY-MM-DD`   | Filter sittings up to this date        |
| `--limit N`               | Maximum number of results to return    |
| `--offset N`              | Number of results to skip              |
| `--output text\|json`     | Output format (default: text)          |

```bash
odnelazm list
odnelazm list --start-date 2024-01-01 --end-date 2024-12-31 --limit 10
odnelazm list --output json | jq '.[] | select(.house == "senate")'
```

### Sitting detail

Fetch the full transcript of a sitting, including sections, contributions, and procedural notes.

| Flag                  | Description                                                     |
| --------------------- | --------------------------------------------------------------- |
| `--fetch-speakers`    | Fetch full profile for each speaker (makes additional requests) |
| `--output text\|json` | Output format (default: text)                                   |

```bash
odnelazm detail https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00
odnelazm detail --fetch-speakers --output json
```

### Global flags

| Flag              | Description                                                                           |
| ----------------- | ------------------------------------------------------------------------------------- |
| `-l, --log-level` | Set log verbosity: `off`, `error`, `warn`, `info`, `debug`, `trace` (default: `info`) |

```bash
odnelazm --log-level debug list
```
