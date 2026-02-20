# odnelazm

The core [mzalendo.com](https://mzalendo.com) hansard scraper and parser.

## Usage

```rust
use odnelazm::scraper::WebScraper;

let scraper = WebScraper::new()?;

// list all available hansard sittings
let listings = scraper.fetch_hansard_list().await?;

// fetch a specific sitting
let detail = scraper.fetch_hansard_detail("https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00").await?;

// fetch a person's profile
let person = scraper.fetch_person_details("/person/farhiya-ali-haji/").await?;
```
