use crate::types::{HansardListing, House};
use chrono::{NaiveDate, NaiveTime};
use scraper::{Html, Selector};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Failed to parse URL: {0}")]
    UrlParseError(String),

    #[error("Failed to parse date: {0}")]
    DateParseError(String),

    #[error("Failed to parse time: {0}")]
    TimeParseError(String),

    #[error("Invalid house type: {0}")]
    InvalidHouse(String),

    #[error("Missing required field: {0}")]
    MissingField(String),
}

pub fn parse_hansard_list(html: &str) -> Result<Vec<HansardListing>, ParseError> {
    let document = Html::parse_document(html);
    let list_selector = Selector::parse("ul.listing li a").unwrap();

    let mut listings = Vec::new();

    for element in document.select(&list_selector) {
        let url = element
            .value()
            .attr("href")
            .ok_or_else(|| ParseError::MissingField("href attribute".to_string()))?
            .to_string();

        let display_text = element.text().collect::<String>();

        match parse_hansard_entry(&url, &display_text) {
            Ok(listing) => listings.push(listing),
            Err(e) => {
                eprintln!("Warning: Failed to parse entry '{}': {}", display_text, e);
            }
        }
    }

    Ok(listings)
}

fn parse_hansard_entry(url: &str, display_text: &str) -> Result<HansardListing, ParseError> {
    let parts: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();

    if parts.len() < 4 {
        return Err(ParseError::UrlParseError(format!(
            "URL has insufficient parts: {}",
            url
        )));
    }

    let house_str = parts[parts.len() - 2];
    let house = match house_str {
        "senate" => House::Senate,
        "national_assembly" => House::NationalAssembly,
        _ => return Err(ParseError::InvalidHouse(house_str.to_string())),
    };

    let date_time_str = parts[parts.len() - 1];
    let (date, start_time, end_time) = parse_date_time(date_time_str, display_text)?;

    let full_url = if url.starts_with("http") {
        url.to_string()
    } else {
        format!("https://info.mzalendo.com{}", url)
    };

    Ok(HansardListing::new(
        house,
        date,
        start_time,
        end_time,
        full_url,
        display_text.to_string(),
    ))
}

fn parse_date_time(
    date_time_str: &str,
    display_text: &str,
) -> Result<(NaiveDate, Option<NaiveTime>, Option<NaiveTime>), ParseError> {
    let parts: Vec<&str> = date_time_str.split('-').collect();

    if parts.len() < 3 {
        return Err(ParseError::DateParseError(format!(
            "Invalid date format: {}",
            date_time_str
        )));
    }

    let year = parts[0]
        .parse::<i32>()
        .map_err(|_| ParseError::DateParseError(format!("Invalid year: {}", parts[0])))?;
    let month = parts[1]
        .parse::<u32>()
        .map_err(|_| ParseError::DateParseError(format!("Invalid month: {}", parts[1])))?;
    let day = parts[2]
        .parse::<u32>()
        .map_err(|_| ParseError::DateParseError(format!("Invalid day: {}", parts[2])))?;

    let date = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| ParseError::DateParseError(format!("Invalid date: {}-{}-{}", year, month, day)))?;

    let start_time = if parts.len() >= 6 {
        let hour = parts[3]
            .parse::<u32>()
            .map_err(|_| ParseError::TimeParseError(format!("Invalid hour: {}", parts[3])))?;
        let minute = parts[4]
            .parse::<u32>()
            .map_err(|_| ParseError::TimeParseError(format!("Invalid minute: {}", parts[4])))?;
        let second = parts[5]
            .parse::<u32>()
            .map_err(|_| ParseError::TimeParseError(format!("Invalid second: {}", parts[5])))?;

        Some(NaiveTime::from_hms_opt(hour, minute, second)
            .ok_or_else(|| ParseError::TimeParseError(format!("Invalid time: {}:{}:{}", hour, minute, second)))?)
    } else {
        None
    };

    let end_time = parse_end_time_from_display(display_text)?;

    Ok((date, start_time, end_time))
}

fn parse_end_time_from_display(display_text: &str) -> Result<Option<NaiveTime>, ParseError> {
    if let Some(to_pos) = display_text.find(" to ") {
        let time_str = &display_text[to_pos + 4..];
        let time_parts: Vec<&str> = time_str.split(':').take(2).collect();

        if time_parts.len() == 2 {
            let hour = time_parts[0].trim()
                .parse::<u32>()
                .map_err(|_| ParseError::TimeParseError(format!("Invalid end hour: {}", time_parts[0])))?;
            let minute = time_parts[1].trim()
                .parse::<u32>()
                .map_err(|_| ParseError::TimeParseError(format!("Invalid end minute: {}", time_parts[1])))?;

            return Ok(Some(NaiveTime::from_hms_opt(hour, minute, 0)
                .ok_or_else(|| ParseError::TimeParseError(format!("Invalid end time: {}:{}", hour, minute)))?));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::House;
    use chrono::{NaiveDate, Timelike};
    use std::fs;

    #[test]
    fn test_parse_hansard_list_from_sample() {
        let html = fs::read_to_string("samples/root-page/Hansard __ Mzalendo.html")
            .expect("Failed to read sample HTML file");

        let listings = parse_hansard_list(&html)
            .expect("Failed to parse hansard list");

        assert!(!listings.is_empty(), "Should parse at least one listing");

        println!("Parsed {} hansard listings", listings.len());
        for (i, listing) in listings.iter().take(5).enumerate() {
            println!("Entry {}: {:?}", i + 1, listing);
        }

        let first = &listings[0];
        assert_eq!(first.house, House::Senate);
        assert_eq!(first.date, NaiveDate::from_ymd_opt(2025, 7, 17).unwrap());
        assert_eq!(first.display_text, "Senate 2025-07-17");

        let with_time = listings.iter()
            .find(|l| l.display_text.contains("2025-07-01: 14:30 to 18:42"))
            .expect("Should find entry with time range");

        assert_eq!(with_time.house, House::NationalAssembly);
        assert_eq!(with_time.date, NaiveDate::from_ymd_opt(2025, 7, 1).unwrap());
        assert!(with_time.start_time.is_some(), "Should have start time");
        assert!(with_time.end_time.is_some(), "Should have end time");
    }

    #[test]
    fn test_parse_senate_entry() {
        let html = r#"
            <ul class="listing">
                <li><a href="https://info.mzalendo.com/hansard/sitting/senate/2025-07-17">Senate 2025-07-17</a></li>
            </ul>
        "#;

        let listings = parse_hansard_list(html)
            .expect("Failed to parse");

        assert_eq!(listings.len(), 1);
        let listing = &listings[0];
        assert_eq!(listing.house, House::Senate);
        assert_eq!(listing.date, NaiveDate::from_ymd_opt(2025, 7, 17).unwrap());
        assert!(listing.start_time.is_none());
        assert!(listing.end_time.is_none());
    }

    #[test]
    fn test_parse_national_assembly_with_time() {
        let html = r#"
            <ul class="listing">
                <li><a href="https://info.mzalendo.com/hansard/sitting/national_assembly/2025-07-01-14-30-00">National Assembly 2025-07-01: 14:30 to 18:42</a></li>
            </ul>
        "#;

        let listings = parse_hansard_list(html)
            .expect("Failed to parse");

        assert_eq!(listings.len(), 1);
        let listing = &listings[0];
        assert_eq!(listing.house, House::NationalAssembly);
        assert_eq!(listing.date, NaiveDate::from_ymd_opt(2025, 7, 1).unwrap());

        let start = listing.start_time.expect("Should have start time");
        assert_eq!(start.hour(), 14);
        assert_eq!(start.minute(), 30);

        let end = listing.end_time.expect("Should have end time");
        assert_eq!(end.hour(), 18);
        assert_eq!(end.minute(), 42);
    }

    #[test]
    fn test_parse_multiple_entries() {
        let html = r#"
            <ul class="listing">
                <li><a href="https://info.mzalendo.com/hansard/sitting/senate/2025-07-17">Senate 2025-07-17</a></li>
                <li><a href="https://info.mzalendo.com/hansard/sitting/senate/2025-07-16">Senate 2025-07-16</a></li>
                <li><a href="https://info.mzalendo.com/hansard/sitting/national_assembly/2025-07-01-14-30-00">National Assembly 2025-07-01: 14:30 to 18:42</a></li>
            </ul>
        "#;

        let listings = parse_hansard_list(html)
            .expect("Failed to parse");

        assert_eq!(listings.len(), 3);
        assert_eq!(listings[0].house, House::Senate);
        assert_eq!(listings[1].house, House::Senate);
        assert_eq!(listings[2].house, House::NationalAssembly);
    }
}

