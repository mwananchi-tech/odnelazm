# odnelazm

The core [mzalendo.com](https://mzalendo.com) hansard scraper and parser.

Source routing is automatic: archive (`info.mzalendo.com`) is used for sittings before 2013-03-28, current (`mzalendo.com`) for those after, and both are merged in parallel for ranges that span the cutoff.

## Usage

```rust
use odnelazm::{HansardScraper, House, SittingListOptions};

let scraper = HansardScraper::new()?;

// List recent sittings (current source, page 1)
let listings = scraper.list_sittings(SittingListOptions::default()).await?;

// List sittings in a date range (auto-routed)
let listings = scraper.list_sittings(SittingListOptions {
    start_date: Some("2023-01-01".parse()?),
    end_date: Some("2023-12-31".parse()?),
    house: Some(House::Senate),
    ..Default::default()
}).await?;

// Fetch a sitting transcript (source detected from URL or slug)
let sitting = scraper.get_sitting("thursday-12th-february-2026-afternoon-sitting-2438").await?;
let sitting = scraper.get_sitting("https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00").await?;

// List members
let members = scraper.list_members(House::NationalAssembly, "13th-parliament", 1).await?;

// Fetch all members from both houses in parallel
let all = scraper.list_all_members_all_houses("13th-parliament").await?;

// Fetch a member profile
let profile = scraper.get_member_profile(
    "https://mzalendo.com/mps-performance/national-assembly/13th-parliament/boss-gladys-jepkosgei/",
    false, // all_activity
    false, // all_bills
).await?;
```
