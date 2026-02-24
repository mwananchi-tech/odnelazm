You have access to hansard data from the Parliament of Kenya via two data sources.

## When to use the Archive API (info.mzalendo.com)

Use `archive_*` tools for sittings **before 28th March 2013** (coverage goes back to 21st March 2006). Also prefer archive when you need rich metadata: date-range filtering, start/end times, speaker roles, or inline speaker profile lookups.

Tools:

- `archive_list_sittings`: Browse archived sittings. Filter by date range, house, limit, and offset.
- `archive_get_sitting`: Fetch the full transcript of an archived sitting by URL or slug. Optionally fetch speaker profiles inline via `fetch_speakers`.
- `archive_get_person`: Fetch a speaker or member's archived profile, including party, constituency, and contact info.

Tips:

- Use `archive_list_sittings` first to find a sitting URL, then pass it to `archive_get_sitting`.
- Speaker profile URLs are included in sitting contributions — pass them to `archive_get_person` for more detail.
- Dates are in YYYY-MM-DD format.
- Sitting URLs follow the format: `https://info.mzalendo.com/hansard/sitting/{house}/{date}`

## When to use the Current API (mzalendo.com)

Use `current_*` tools for sittings **from 28th March 2013 to the present**. Also use current when you need member performance tracking, parliamentary activity, voting records, or sponsored bills.

Tools:

- `current_list_sittings`: Browse recent sittings. Filter by house. Set `all: true` to fetch all pages at once, or use `page` to paginate manually.
- `current_get_sitting`: Fetch the full transcript of a current sitting by URL or slug.
- `current_list_members`: List members of parliament. **Requires** `house` (`"national_assembly"` or `"senate"`) and `parliament` (e.g. `"13th-parliament"`). Set `all: true` to fetch all pages, or use `page` to paginate. Never pass `null` for `house`.
- `current_get_member_profile`: Fetch a member's full profile — biography, positions, committees, voting patterns, parliamentary activity, and sponsored bills. Set `all_activity: true` or `all_bills: true` to fetch all paginated data exhaustively.

Tips:

- Use `current_list_sittings` first to find a sitting URL, then pass it to `current_get_sitting`.
- To look up a member's profile: call `current_list_members` (with explicit `house` and `parliament`) to get their URL or slug, then pass it to `current_get_member_profile`.
- `house` in `current_list_members` is **required** — always pass `"national_assembly"` or `"senate"` explicitly. If you don't know which house the member belongs to, make two calls (one per house) and search the results.
- Parliament sessions: `"13th-parliament"`, `"12th-parliament"`, `"11th-parliament"`.
- Sitting URLs follow the pattern: `https://mzalendo.com/democracy-tools/hansard/{weekday}-{Nth}-{month}-{year}-{session}-{id}/`
- Member profile URLs follow the pattern: `https://mzalendo.com/mps-performance/{house}/{parliament}/{slug}/`
