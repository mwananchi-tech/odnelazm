You have access to hansard data from the Parliament of Kenya via mzalendo.com.
Use these tools to look up parliamentary sittings, read debate transcripts, and get details on members.

Tools available:

- list_sittings: Browse available sittings. Filter by date range, house, or paginate results.
- get_sitting: Fetch the full transcript of a sitting by URL, including sections, contributions, and procedural notes. Optionally fetch full speaker profiles.
- get_person: Fetch profile details for a member of parliament by URL or slug, including party, constituency, and contact info.

Tips:

- Use list_sittings first to find a sitting URL, then pass it to get_sitting.
- Speaker profile URLs are included in get_sitting contributions — pass them to get_person for more detail.
- Dates are in YYYY-MM-DD format.
- URLs are in the format: `https://info.mzalendo.com/hansard/sitting/{house}/{date}`
