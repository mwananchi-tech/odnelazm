use std::str::FromStr;
use std::sync::LazyLock;

use crate::types::{
    Contribution, HansardDetail, HansardListing, HansardSection, House, PersonDetails,
};

use chrono::{NaiveDate, NaiveTime};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

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

static RE_SESSION_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(Special|Morning|Afternoon) Sitting").expect("invalid regex: session type")
});
static RE_NAME_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(Hon\.|Sen\.)\s(Dr\.\s)?").expect("invalid regex: name prefix")
});
static RE_ROLE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(The\s)?(Ayes|Noes|Teller|Temporary Speaker|Speaker|Chairperson|Majority Leader|Minority Leader|Majority Whip|Minority Whip)")
        .expect("invalid regex: role prefix")
});
static RE_CONSTITUENCY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[^,]+,\s*.+").expect("invalid regex: constituency"));
static RE_NAME_IN_PARENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?)\s*\((.+?)\)$").expect("invalid regex: name in parens"));
static RE_END_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bto\s+(\d{1,2}):(\d{2})\b").expect("invalid regex: end time"));

fn elem_text(element: ElementRef) -> String {
    element.text().collect::<String>()
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_parenthesized(text: &str) -> Option<String> {
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    (end > start).then(|| text[start + 1..end].trim().to_string())
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

        let display_text = elem_text(element);

        match parse_hansard_entry(&url, &display_text) {
            Ok(listing) => listings.push(listing),
            Err(e) => log::warn!("Skipping entry '{}': {}", display_text, e),
        }
    }

    Ok(listings)
}

pub fn parse_hansard_detail(html: &str, url: &str) -> Result<HansardDetail, ParseError> {
    let document = Html::parse_document(html);

    let parts: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();

    let house_str = parts
        .get(parts.len().wrapping_sub(2))
        .ok_or_else(|| ParseError::UrlParseError("Could not extract house from URL".to_string()))?;
    let house = House::from_str(house_str)?;

    let date_time_str = parts
        .last()
        .ok_or_else(|| ParseError::UrlParseError("Could not extract date from URL".to_string()))?;
    let (date, start_time, _) = parse_date_time(date_time_str, "")?;

    let h2_selector = Selector::parse("h2").unwrap();

    let parliament_number = document
        .select(&h2_selector)
        .map(|e| normalize_whitespace(&elem_text(e)))
        .find(|t| t.contains("PARLIAMENT"))
        .unwrap_or_else(|| "PARLIAMENT OF KENYA".to_string());

    let session_number = document
        .select(&h2_selector)
        .map(|e| normalize_whitespace(&elem_text(e)))
        .find(|t| t.contains("Session"))
        .unwrap_or_else(|| "Unknown Session".to_string());

    let page_number_selector = Selector::parse("li.page_number").unwrap();
    let session_type = document
        .select(&page_number_selector)
        .next()
        .and_then(|e| {
            RE_SESSION_TYPE
                .find(&elem_text(e))
                .map(|m| m.as_str().to_string())
        })
        .unwrap_or_else(|| "Regular Sitting".to_string());

    let scene_selector = Selector::parse("li.scene").unwrap();
    let speaker_in_chair = document
        .select(&scene_selector)
        .find_map(|e| {
            let t = elem_text(e);
            t.contains("in the Chair").then(|| normalize_whitespace(&t))
        })
        .unwrap_or_else(|| "[Speaker information not found]".to_string());

    let sections = parse_sections(&document)?;

    Ok(HansardDetail {
        house,
        date,
        start_time,
        end_time: None,
        parliament_number,
        session_number,
        session_type,
        speaker_in_chair,
        sections,
    })
}

pub fn parse_person_details(html: &str, url: &str) -> Result<PersonDetails, ParseError> {
    let document = Html::parse_document(html);

    let slug = url
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .ok_or_else(|| ParseError::UrlParseError("Could not extract slug from URL".to_string()))?
        .to_string();

    let h1_selector = Selector::parse("h1").unwrap();
    let name = document
        .select(&h1_selector)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .ok_or_else(|| ParseError::MissingField("name".to_string()))?;

    let p_selector = Selector::parse("p").unwrap();
    let summary = document
        .select(&p_selector)
        .find(|e| {
            let t = elem_text(*e);
            !t.trim().is_empty()
                && !t.contains("Email")
                && !t.contains("Telephone")
                && !t.contains('@')
        })
        .map(|e| normalize_whitespace(&elem_text(e)));

    let party_selector = Selector::parse(".party-membership").unwrap();
    let (party, party_url) = document
        .select(&party_selector)
        .next()
        .map(|e| {
            (
                Some(normalize_whitespace(&elem_text(e))),
                e.value().attr("href").map(str::to_string),
            )
        })
        .unwrap_or((None, None));

    let email_selector = Selector::parse("a[href^='mailto:']").unwrap();
    let email = document
        .select(&email_selector)
        .next()
        .and_then(|e| e.value().attr("href"))
        .map(|h| h.trim_start_matches("mailto:").to_string());

    let tel_selector = Selector::parse("a[href^='tel:']").unwrap();
    let telephone = document
        .select(&tel_selector)
        .next()
        .and_then(|e| e.value().attr("href"))
        .map(|h| h.trim_start_matches("tel:").to_string());

    let position_selector = Selector::parse(".position.ongoing h4").unwrap();
    let current_position = document
        .select(&position_selector)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)));

    let place_selector = Selector::parse(".position.ongoing a[href^='/place/']").unwrap();
    let constituency = document
        .select(&place_selector)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)));

    Ok(PersonDetails {
        name,
        slug,
        summary,
        party,
        party_url,
        email,
        telephone,
        current_position,
        constituency,
    })
}

fn parse_hansard_entry(url: &str, display_text: &str) -> Result<HansardListing, ParseError> {
    let parts: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();

    if parts.len() < 4 {
        return Err(ParseError::UrlParseError(format!(
            "URL has insufficient parts: {}",
            url
        )));
    }

    let house = House::from_str(parts[parts.len() - 2])?;
    let (date, start_time, end_time) = parse_date_time(parts[parts.len() - 1], display_text)?;

    let full_url = if url.starts_with("http") {
        url.to_string()
    } else {
        format!("{}{}", crate::BASE_URL, url)
    };

    Ok(HansardListing {
        house,
        date,
        start_time,
        end_time,
        url: full_url,
        display_text: display_text.to_string(),
    })
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

    let parse_u32 = |s: &str, label: &str| -> Result<u32, ParseError> {
        s.parse()
            .map_err(|_| ParseError::DateParseError(format!("Invalid {}: {}", label, s)))
    };

    let year = parts[0]
        .parse::<i32>()
        .map_err(|_| ParseError::DateParseError(format!("Invalid year: {}", parts[0])))?;
    let month = parse_u32(parts[1], "month")?;
    let day = parse_u32(parts[2], "day")?;

    let date = NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| {
        ParseError::DateParseError(format!("Invalid date: {}-{}-{}", year, month, day))
    })?;

    let start_time = if parts.len() >= 6 {
        let hour = parse_u32(parts[3], "hour")?;
        let minute = parse_u32(parts[4], "minute")?;
        let second = parse_u32(parts[5], "second")?;

        Some(
            NaiveTime::from_hms_opt(hour, minute, second).ok_or_else(|| {
                ParseError::TimeParseError(format!("Invalid time: {}:{}:{}", hour, minute, second))
            })?,
        )
    } else {
        None
    };

    let end_time = parse_end_time_from_display(display_text)?;

    Ok((date, start_time, end_time))
}

fn parse_end_time_from_display(display_text: &str) -> Result<Option<NaiveTime>, ParseError> {
    let Some(caps) = RE_END_TIME.captures(display_text) else {
        return Ok(None);
    };

    let hour: u32 = caps[1]
        .parse()
        .map_err(|_| ParseError::TimeParseError(format!("Invalid end hour: {}", &caps[1])))?;
    let minute: u32 = caps[2]
        .parse()
        .map_err(|_| ParseError::TimeParseError(format!("Invalid end minute: {}", &caps[2])))?;

    NaiveTime::from_hms_opt(hour, minute, 0)
        .ok_or_else(|| ParseError::TimeParseError(format!("Invalid end time: {}:{}", hour, minute)))
        .map(Some)
}

fn parse_sections(document: &Html) -> Result<Vec<HansardSection>, ParseError> {
    let mut sections: Vec<HansardSection> = Vec::new();
    let mut current: Option<HansardSection> = None;

    let all_items_selector = Selector::parse("li.heading, li.speech, li.scene").unwrap();

    for element in document.select(&all_items_selector) {
        let class = element.value().attr("class").unwrap_or("");

        if class.contains("heading") {
            if let Some(section) = current.take() {
                sections.push(section);
            }

            let heading = normalize_whitespace(&elem_text(element));

            if heading.contains("PARLIAMENT")
                || heading.contains("SENATE")
                || heading.contains("NATIONAL ASSEMBLY")
            {
                continue;
            }

            current = Some(HansardSection {
                section_type: heading,
                title: None,
                contributions: Vec::new(),
            });
        } else if class.contains("speech") {
            if let Some(ref mut section) = current
                && let Ok(contribution) = parse_contribution(element)
            {
                section.contributions.push(contribution);
            }
        } else if class.contains("scene")
            && let Some(ref mut section) = current
        {
            let scene = normalize_whitespace(&elem_text(element));
            if !scene.is_empty()
                && !section.contributions.is_empty()
                && let Some(last) = section.contributions.last_mut()
            {
                last.procedural_notes.push(scene);
            }
        }
    }

    if let Some(section) = current {
        sections.push(section);
    }

    Ok(sections)
}

fn parse_contribution(element: ElementRef) -> Result<Contribution, ParseError> {
    let strong_selector = Selector::parse("strong").unwrap();
    let a_selector = Selector::parse("a").unwrap();

    let (mut speaker_name, speaker_url) =
        if let Some(strong_elem) = element.select(&strong_selector).next() {
            if let Some(a_elem) = strong_elem.select(&a_selector).next() {
                let name = normalize_whitespace(&elem_text(a_elem));
                let url = a_elem.value().attr("href").map(str::to_string);
                (name, url)
            } else {
                (normalize_whitespace(&elem_text(strong_elem)), None)
            }
        } else {
            return Err(ParseError::MissingField("speaker name".to_string()));
        };

    let strong_text = element
        .select(&strong_selector)
        .next()
        .map(|e| elem_text(e))
        .unwrap_or_default();

    let p_selector = Selector::parse("p").unwrap();
    let full_text = elem_text(element);
    let content_text = element
        .select(&p_selector)
        .map(|p| elem_text(p))
        .collect::<Vec<_>>()
        .join("");

    let header_text = full_text
        .replace(&strong_text, "")
        .replace(&content_text, "");

    let mut speaker_role = extract_parenthesized(&header_text);

    // XXX: Normalize speaker name/role inconsistencies from hansard authors.
    // Sometimes they write "<strong>Hon. Lusaka</strong> (The Speaker)" and other times
    // "<strong>The Speaker (Hon. Lusaka)</strong>" or "<strong>Mwala, UDA</strong> (Hon. Vincent Musau)".
    // We detect and normalize these cases by swapping when appropriate.

    if let Some(role) = &speaker_role {
        // case 1: name is "Constituency, Party", role is the actual person name
        let name_is_constituency =
            RE_CONSTITUENCY.is_match(&speaker_name) && !RE_NAME_PREFIX.is_match(&speaker_name);
        let role_is_name = RE_NAME_PREFIX.is_match(role);

        if name_is_constituency && role_is_name {
            std::mem::swap(&mut speaker_name, speaker_role.as_mut().unwrap());
        }
    }

    if let Some(role) = &speaker_role {
        // case 2: name looks like a role title, role looks like a person name
        if RE_NAME_PREFIX.is_match(role) && RE_ROLE_PREFIX.is_match(&speaker_name) {
            std::mem::swap(&mut speaker_name, speaker_role.as_mut().unwrap());
        }
    }

    // case 3: name itself contains "Role (Hon. Name)" — extract and swap
    if speaker_role.is_none()
        && let Some(caps) = RE_NAME_IN_PARENS.captures(&speaker_name)
    {
        let outer = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let inner = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        if RE_NAME_PREFIX.is_match(&inner) && RE_ROLE_PREFIX.is_match(&outer) {
            speaker_name = inner;
            speaker_role = Some(outer);
        }
    }
    let content = element
        .select(&p_selector)
        .map(|p| normalize_whitespace(&elem_text(p)))
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(Contribution {
        speaker_name,
        speaker_role,
        speaker_url,
        speaker_details: None,
        content,
        procedural_notes: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::House;
    use chrono::{NaiveDate, Timelike};
    use std::fs;

    #[test]
    fn test_parse_hansard_list_from_sample() {
        let html = fs::read_to_string("fixtures/root-page/Hansard __ Mzalendo")
            .expect("Failed to read sample HTML file");

        let listings = parse_hansard_list(&html).expect("Failed to parse hansard list");

        assert!(!listings.is_empty(), "Should parse at least one listing");

        println!("Parsed {} hansard listings", listings.len());
        for (i, listing) in listings.iter().take(5).enumerate() {
            println!("Entry {}: {:?}", i + 1, listing);
        }

        let first = &listings[0];
        assert_eq!(first.house, House::Senate);
        assert_eq!(first.date, NaiveDate::from_ymd_opt(2025, 7, 17).unwrap());
        assert_eq!(first.display_text, "Senate 2025-07-17");

        let with_time = listings
            .iter()
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

        let listings = parse_hansard_list(html).expect("Failed to parse");

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

        let listings = parse_hansard_list(html).expect("Failed to parse");

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

        let listings = parse_hansard_list(html).expect("Failed to parse");

        assert_eq!(listings.len(), 3);
        assert_eq!(listings[0].house, House::Senate);
        assert_eq!(listings[1].house, House::Senate);
        assert_eq!(listings[2].house, House::NationalAssembly);
    }

    #[test]
    fn test_parse_hansard_detail_2020() {
        let html =
            fs::read_to_string("fixtures/hansard_detail_2020").expect("Failed to read sample file");
        let url = "https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00";

        let detail = parse_hansard_detail(&html, url).expect("Failed to parse hansard detail");

        assert_eq!(detail.house, House::Senate);
        assert_eq!(detail.date.to_string(), "2020-12-29");
        assert!(detail.parliament_number.contains("PARLIAMENT"));
        assert!(detail.session_type.contains("Sitting"));
        assert!(!detail.sections.is_empty());

        let has_contributions = detail
            .sections
            .iter()
            .any(|section| !section.contributions.is_empty());
        assert!(has_contributions, "Should have at least one contribution");

        let has_speaker_urls = detail.sections.iter().any(|section| {
            section
                .contributions
                .iter()
                .any(|c| c.speaker_url.is_some())
        });
        assert!(has_speaker_urls, "2020 hansard should have speaker URLs");
    }

    #[test]
    fn test_parse_person_details_farhiya() {
        let html = fs::read_to_string("fixtures/persons/person_farhiya")
            .expect("Failed to read sample file");
        let url = "/person/farhiya-ali-haji/";

        let person = parse_person_details(&html, url).expect("Failed to parse person details");

        assert_eq!(person.name, "Farhiya Ali Haji");
        assert_eq!(person.slug, "farhiya-ali-haji");
        assert!(person.summary.is_some());
        assert_eq!(person.party, Some("Jubilee Party".to_string()));
        assert_eq!(
            person.party_url,
            Some("/organisation/jubilee_party/".to_string())
        );
        assert_eq!(person.email, Some("farhiyaali1@gmail.com".to_string()));
        assert_eq!(person.telephone, Some("0722801011".to_string()));
        assert!(person.current_position.is_some());
    }

    #[test]
    fn test_parse_person_details_samson() {
        let html = fs::read_to_string("fixtures/persons/person_samson")
            .expect("Failed to read sample file");
        let url = "/person/cherarkey-k-samson/";

        let person = parse_person_details(&html, url).expect("Failed to parse person details");

        assert_eq!(person.name, "Cherarkey K Samson");
        assert_eq!(person.slug, "cherarkey-k-samson");
        assert!(
            person.party.is_none()
                || person.party == Some("Not a member of any parties or coalitions".to_string())
        );
    }
}
