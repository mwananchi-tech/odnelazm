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

    let date = NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| {
        ParseError::DateParseError(format!("Invalid date: {}-{}-{}", year, month, day))
    })?;

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
    if let Some(to_pos) = display_text.find(" to ") {
        let time_str = &display_text[to_pos + 4..];
        let time_parts: Vec<&str> = time_str.split(':').take(2).collect();

        if time_parts.len() == 2 {
            let hour = time_parts[0].trim().parse::<u32>().map_err(|_| {
                ParseError::TimeParseError(format!("Invalid end hour: {}", time_parts[0]))
            })?;
            let minute = time_parts[1].trim().parse::<u32>().map_err(|_| {
                ParseError::TimeParseError(format!("Invalid end minute: {}", time_parts[1]))
            })?;

            return Ok(Some(NaiveTime::from_hms_opt(hour, minute, 0).ok_or_else(
                || ParseError::TimeParseError(format!("Invalid end time: {}:{}", hour, minute)),
            )?));
        }
    }

    Ok(None)
}

pub fn parse_hansard_detail(html: &str, url: &str) -> Result<HansardDetail, ParseError> {
    let document = Html::parse_document(html);

    let parts: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();
    let house_str = parts
        .get(parts.len() - 2)
        .ok_or_else(|| ParseError::UrlParseError("Could not extract house from URL".to_string()))?;
    let house = match house_str.to_lowercase().as_str() {
        "senate" => House::Senate,
        "national_assembly" => House::NationalAssembly,
        _ => return Err(ParseError::InvalidHouse(house_str.to_string())),
    };

    let date_time_str = parts
        .last()
        .ok_or_else(|| ParseError::UrlParseError("Could not extract date from URL".to_string()))?;
    let (date, start_time, _end_time) = parse_date_time(date_time_str, "")?;

    let h2_selector = Selector::parse("h2").unwrap();
    let parliament_number = document
        .select(&h2_selector)
        .map(|elem| elem.text().collect::<String>().trim().to_string())
        .find(|text| text.contains("PARLIAMENT"))
        .unwrap_or_else(|| "PARLIAMENT OF KENYA".to_string());

    let session_info = document
        .select(&h2_selector)
        .map(|elem| elem.text().collect::<String>().trim().to_string())
        .find(|text| text.contains("Session"))
        .unwrap_or_else(String::new);

    let session_number = if session_info.is_empty() {
        "Unknown Session".to_string()
    } else {
        session_info.clone()
    };

    let page_number_selector = Selector::parse("li.page_number").unwrap();
    let session_type = if let Some(page_elem) = document.select(&page_number_selector).next() {
        let text = page_elem.text().collect::<String>();
        if text.contains("Special Sitting") {
            "Special Sitting".to_string()
        } else if text.contains("Morning") {
            "Morning Sitting".to_string()
        } else if text.contains("Afternoon") {
            "Afternoon Sitting".to_string()
        } else {
            "Regular Sitting".to_string()
        }
    } else {
        "Regular Sitting".to_string()
    };

    let scene_selector = Selector::parse("li.scene").unwrap();
    let speaker_in_chair = document
        .select(&scene_selector)
        .find_map(|elem| {
            let text = elem.text().collect::<String>();
            if text.contains("in the Chair") {
                Some(text.trim().to_string())
            } else {
                None
            }
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

fn parse_sections(document: &Html) -> Result<Vec<HansardSection>, ParseError> {
    let mut sections = Vec::new();
    let mut current_section: Option<HansardSection> = None;

    let all_items_selector = Selector::parse("li.heading, li.speech, li.scene").unwrap();

    for element in document.select(&all_items_selector) {
        let class = element.value().attr("class").unwrap_or("");

        if class.contains("heading") {
            if let Some(section) = current_section.take() {
                sections.push(section);
            }

            let heading_text = element.text().collect::<String>().trim().to_string();

            if heading_text.contains("PARLIAMENT")
                || heading_text.contains("SENATE")
                || heading_text.contains("NATIONAL ASSEMBLY")
            {
                continue;
            }

            current_section = Some(HansardSection {
                section_type: heading_text.clone(),
                title: None,
                contributions: Vec::new(),
            });
        } else if class.contains("speech") {
            if let Some(ref mut section) = current_section
                && let Ok(contribution) = parse_contribution(element)
            {
                section.contributions.push(contribution);
            }
        } else if class.contains("scene")
            && let Some(ref mut section) = current_section
        {
            let scene_text = element.text().collect::<String>().trim().to_string();
            if !scene_text.is_empty()
                && !section.contributions.is_empty()
                && let Some(last_contribution) = section.contributions.last_mut()
            {
                last_contribution.procedural_notes.push(scene_text);
            }
        }
    }

    if let Some(section) = current_section {
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
                let name = a_elem.text().collect::<String>().trim().to_string();
                let url = a_elem.value().attr("href").map(|s| s.to_string());
                (name, url)
            } else {
                let name = strong_elem.text().collect::<String>().trim().to_string();
                (name, None)
            }
        } else {
            return Err(ParseError::MissingField("speaker name".to_string()));
        };

    // Extract role from text between <strong> and <p> tags (not from content)
    let p_selector = Selector::parse("p").unwrap();
    let strong_text = element
        .select(&strong_selector)
        .next()
        .map(|e| e.text().collect::<String>())
        .unwrap_or_default();

    let full_text = element.text().collect::<String>();
    let content_text = element
        .select(&p_selector)
        .map(|p| p.text().collect::<String>())
        .collect::<Vec<_>>()
        .join("");

    // Get text between strong tag and content (where role usually appears)
    let header_text = full_text
        .replace(&strong_text, "")
        .replace(&content_text, "");

    let mut speaker_role = if header_text.contains('(') && header_text.contains(')') {
        let start = header_text.find('(').unwrap();
        let end = header_text.rfind(')').unwrap(); // Use rfind to get the LAST closing paren
        if end > start {
            Some(header_text[start + 1..end].trim().to_string())
        } else {
            None
        }
    } else {
        None
    };

    // XXX: Normalize speaker name/role inconsistencies from hansard authors.
    // Sometimes they write "<strong>Hon. Lusaka</strong> (The Speaker)" and other times
    // "<strong>The Speaker (Hon. Lusaka)</strong>" or "<strong>Mwala, UDA</strong> (Hon. Vincent Musau)".
    // We detect and normalize these cases by swapping when appropriate.

    // Regex patterns for detecting names, roles, and constituencies
    let name_pattern = Regex::new(r"^(Hon\.|Sen\.)\s").unwrap();
    let role_pattern = Regex::new(r"^The\s|Ayes|Noes|Teller|Speaker|Chairperson").unwrap();
    let constituency_pattern = Regex::new(r".+,\s*.+").unwrap(); // Matches "Mwala, UDA" format

    // Case 1: Name is constituency/party and role is actual person name - swap them
    // e.g., name="Mwala, UDA" role="Hon. Vincent Musau" -> swap
    if let Some(role) = &speaker_role {
        let name_is_constituency =
            constituency_pattern.is_match(&speaker_name) && !name_pattern.is_match(&speaker_name);
        let role_is_person_name = name_pattern.is_match(role);

        if name_is_constituency && role_is_person_name {
            let temp = speaker_name;
            speaker_name = role.clone();
            speaker_role = Some(temp);
        }
    }

    // Case 2: Role in parentheses looks like a name, and name looks like a role - swap them
    // e.g., name="The Speaker" role="Hon. Lusaka" -> swap
    if let Some(role) = &speaker_role {
        let role_looks_like_name = name_pattern.is_match(role);
        let name_looks_like_role = role_pattern.is_match(&speaker_name);

        if role_looks_like_name && name_looks_like_role {
            let temp = speaker_name;
            speaker_name = role.clone();
            speaker_role = Some(temp);
        }
    }

    // Case 3: Name contains parentheses with what looks like an actual name inside
    // e.g., "The Speaker (Hon. Lusaka)" -> extract "Hon. Lusaka" as name, "The Speaker" as role
    if speaker_role.is_none() {
        let parentheses_pattern = Regex::new(r"^(.+?)\s*\((.+?)\)$").unwrap();
        let name_clone = speaker_name.clone();
        if let Some(caps) = parentheses_pattern.captures(&name_clone) {
            let outer = caps
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let inner = caps
                .get(2)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();

            let inner_looks_like_name = name_pattern.is_match(&inner);
            let outer_looks_like_role = role_pattern.is_match(&outer);

            if inner_looks_like_name && outer_looks_like_role {
                speaker_name = inner;
                speaker_role = Some(outer);
            }
        }
    }

    let p_selector = Selector::parse("p").unwrap();
    let content = element
        .select(&p_selector)
        .map(|p| p.text().collect::<String>().trim().to_string())
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

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
        .map(|elem| normalize_whitespace(&elem.text().collect::<String>()))
        .ok_or_else(|| ParseError::MissingField("name".to_string()))?;

    let p_selector = Selector::parse("p").unwrap();
    let summary = document
        .select(&p_selector)
        .find(|elem| {
            let text = elem.text().collect::<String>();
            !text.trim().is_empty()
                && !text.contains("Email")
                && !text.contains("Telephone")
                && !text.contains("@")
        })
        .map(|elem| normalize_whitespace(&elem.text().collect::<String>()));

    let party_selector = Selector::parse(".party-membership").unwrap();
    let (party, party_url) = if let Some(party_elem) = document.select(&party_selector).next() {
        let party_name = normalize_whitespace(&party_elem.text().collect::<String>());
        let party_link = party_elem.value().attr("href").map(|s| s.to_string());
        (Some(party_name), party_link)
    } else {
        (None, None)
    };

    let email_selector = Selector::parse("a[href^='mailto:']").unwrap();
    let email = document
        .select(&email_selector)
        .next()
        .and_then(|elem| elem.value().attr("href"))
        .map(|href| href.trim_start_matches("mailto:").to_string());

    let tel_selector = Selector::parse("a[href^='tel:']").unwrap();
    let telephone = document
        .select(&tel_selector)
        .next()
        .and_then(|elem| elem.value().attr("href"))
        .map(|href| href.trim_start_matches("tel:").to_string());

    let position_selector = Selector::parse(".position.ongoing h4").unwrap();
    let current_position = document
        .select(&position_selector)
        .next()
        .map(|elem| normalize_whitespace(&elem.text().collect::<String>()));

    let place_selector = Selector::parse(".position.ongoing a[href^='/place/']").unwrap();
    let constituency = document
        .select(&place_selector)
        .next()
        .map(|elem| normalize_whitespace(&elem.text().collect::<String>()));

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
        let html = fs::read_to_string("samples/hansard_detail_2020.html")
            .expect("Failed to read sample file");
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
        let html = fs::read_to_string("samples/persons/person_farhiya.html")
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
        let html = fs::read_to_string("samples/persons/person_samson.html")
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
