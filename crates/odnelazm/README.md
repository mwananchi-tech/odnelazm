# odnelazm

The core [mzalendo.com](https://mzalendo.com) hansard scraper and parser.

## Usage

### Archive (`info.mzalendo.com`)

```rust
use odnelazm::archive::WebScraper;

let scraper = WebScraper::new()?;

// list all available sittings
let listings = scraper.fetch_hansard_list().await?;

// fetch a sitting transcript
let sitting = scraper.fetch_hansard_sitting("https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00", false).await?;

// fetch a person's profile
let person = scraper.fetch_person_details("/person/farhiya-ali-haji/").await?;
```

### Current (`mzalendo.com/democracy-tools`)

```rust
use odnelazm::current::WebScraper;
use odnelazm::House;

let scraper = WebScraper::new()?;

// list one page of sittings, optionally filtered by house
let listings = scraper.fetch_hansard_list(1, Some(House::Senate)).await?;

// fetch all sittings across all pages
let all = scraper.fetch_all_sittings(None).await?;

// fetch a sitting transcript
let sitting = scraper.fetch_hansard_sitting("thursday-12th-february-2026-afternoon-sitting-2438").await?;

// list members
let members = scraper.fetch_members(House::NationalAssembly, "13th-parliament", 1).await?;

// fetch a member profile (with all activity and bills pages)
let profile = scraper.fetch_member_profile(
    "https://mzalendo.com/mps-performance/national-assembly/13th-parliament/boss-gladys-jepkosgei/",
    true,  // fetch_all_activity
    true,  // fetch_all_bills
).await?;
```
