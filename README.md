# odnelazm

A [mzalendo.com](https://info.mzalendo.com) hansard scraper and parser, to help structure the data from national assembly and senate sittings from the Parliament of Kenya over the years.

## library

- parsing logic for hansard list and detail pages
- type definitions for entities:
  - sittings
  - members
  - bills
  - topics
- create associations between these entities in a noSQL fashion (single document)

## command line interface

- run the parser for all hansard documents, specicic or within a range
- determine output format:
  - json
  - text
  - markdown
- support streaming of content to enable async operations for each parsed session e.g for storage into a database
