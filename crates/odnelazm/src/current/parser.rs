use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveTime};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use super::types::{
    Bill, Contribution, HansardListing, HansardSection, HansardSitting, House, Member,
    MemberProfile, ParliamentaryActivity, VoteRecord,
};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Failed to parse URL: {0}")]
    UrlParse(String),
    #[error("Failed to parse date: {0}")]
    DateParse(String),
    #[error("Failed to parse time: {0}")]
    TimeParse(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
}

static RE_LISTING_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\w+),\s+(\d+)\w*\s+(\w+),?\s+(\d{4})\s*[-–]\s*(.+)")
        .expect("invalid regex: listing title")
});

static RE_SPEECHES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"has made\D+(\d+)\D+speeches last year\D+(\d+)\D+speeches")
        .expect("invalid regex: speeches")
});

static RE_BILLS_TOTAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"has sponsored\D+(\d+)\D+bill").expect("invalid regex: bills total")
});

fn elem_text(element: ElementRef) -> String {
    element.text().collect::<String>()
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_month(month: &str) -> Result<u32, ParseError> {
    match month.to_lowercase().as_str() {
        "january" => Ok(1),
        "february" => Ok(2),
        "march" => Ok(3),
        "april" => Ok(4),
        "may" => Ok(5),
        "june" => Ok(6),
        "july" => Ok(7),
        "august" => Ok(8),
        "september" => Ok(9),
        "october" => Ok(10),
        "november" => Ok(11),
        "december" => Ok(12),
        _ => Err(ParseError::DateParse(format!("Unknown month: {}", month))),
    }
}

fn parse_time_12h(time_str: &str) -> Result<NaiveTime, ParseError> {
    let s = time_str.trim();
    let pos = s
        .rfind(' ')
        .ok_or_else(|| ParseError::TimeParse(format!("Invalid time: {}", s)))?;
    let (t, ampm) = (&s[..pos], s[pos + 1..].trim());

    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() != 2 {
        return Err(ParseError::TimeParse(format!("Invalid time format: {}", s)));
    }

    let hour: u32 = parts[0]
        .parse()
        .map_err(|_| ParseError::TimeParse(format!("Invalid hour: {}", parts[0])))?;
    let minute: u32 = parts[1]
        .parse()
        .map_err(|_| ParseError::TimeParse(format!("Invalid minute: {}", parts[1])))?;

    let hour_24 = match ampm.to_uppercase().as_str() {
        "AM" => {
            if hour == 12 {
                0
            } else {
                hour
            }
        }
        "PM" => {
            if hour == 12 {
                12
            } else {
                hour + 12
            }
        }
        _ => return Err(ParseError::TimeParse(format!("Invalid AM/PM: {}", ampm))),
    };

    NaiveTime::from_hms_opt(hour_24, minute, 0)
        .ok_or_else(|| ParseError::TimeParse(format!("Invalid time: {}:{}", hour_24, minute)))
}

fn parse_date_from_title(title: &str) -> Result<(NaiveDate, String, String), ParseError> {
    let caps = RE_LISTING_TITLE.captures(title).ok_or_else(|| {
        ParseError::DateParse(format!("Could not match date pattern in: {}", title))
    })?;

    let day_of_week = caps[1].to_string();
    let day: u32 = caps[2]
        .parse()
        .map_err(|_| ParseError::DateParse(format!("Invalid day: {}", &caps[2])))?;
    let month = parse_month(&caps[3])?;
    let year: i32 = caps[4]
        .parse()
        .map_err(|_| ParseError::DateParse(format!("Invalid year: {}", &caps[4])))?;
    let session_type = normalize_whitespace(caps[5].trim());

    let date = NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| {
        ParseError::DateParse(format!("Invalid date: {}-{}-{}", year, month, day))
    })?;

    Ok((date, day_of_week, session_type))
}

fn parse_date_from_url_slug(url: &str) -> Result<(NaiveDate, String, String), ParseError> {
    let slug = url
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .ok_or_else(|| ParseError::UrlParse(format!("Invalid URL: {}", url)))?;

    let parts: Vec<&str> = slug.split('-').collect();
    if parts.len() < 5 {
        return Err(ParseError::UrlParse(format!(
            "Slug has too few parts: {}",
            slug
        )));
    }

    let day_of_week = parts[0].to_string();
    let day_str = parts[1].trim_end_matches(|c: char| c.is_alphabetic());
    let day: u32 = day_str
        .parse()
        .map_err(|_| ParseError::DateParse(format!("Invalid day: {}", parts[1])))?;
    let month = parse_month(parts[2])?;
    let year: i32 = parts[3]
        .parse()
        .map_err(|_| ParseError::DateParse(format!("Invalid year: {}", parts[3])))?;

    let session_words: Vec<&str> = parts[4..]
        .iter()
        .take_while(|p| !p.chars().all(|c| c.is_ascii_digit()))
        .cloned()
        .collect();
    let session_type = session_words
        .iter()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let date = NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| {
        ParseError::DateParse(format!("Invalid date: {}-{}-{}", year, month, day))
    })?;

    Ok((date, day_of_week, session_type))
}

pub fn parse_page_info(html: &str) -> Option<(u32, u32)> {
    let document = Html::parse_document(html);

    let active_sel = Selector::parse("li.active.active_number_box span").unwrap();
    let current_page = document
        .select(&active_sel)
        .next()
        .and_then(|e| normalize_whitespace(&elem_text(e)).parse::<u32>().ok())?;

    let page_label_sel = Selector::parse("a.page_label[href]").unwrap();
    let total_pages = document
        .select(&page_label_sel)
        .filter_map(|e| {
            let href = e.value().attr("href")?;
            let after = href.split("page=").nth(1)?;
            after
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
        })
        .max()?;

    Some((current_page, total_pages))
}

pub fn parse_bills_page_info(html: &str) -> Option<(u32, u32)> {
    let document = Html::parse_document(html);

    let active_sel = Selector::parse("nav.bills-pagination li.active_number_box span").unwrap();
    let current_page = document
        .select(&active_sel)
        .next()
        .and_then(|e| normalize_whitespace(&elem_text(e)).parse::<u32>().ok())?;

    let link_sel = Selector::parse("nav.bills-pagination a[href]").unwrap();
    let total_pages = document
        .select(&link_sel)
        .filter_map(|e| {
            let href = e.value().attr("href")?;
            let after = href.split("bills_page=").nth(1)?;
            after
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(current_page);

    Some((current_page, total_pages))
}

pub fn parse_bills(html: &str) -> Vec<Bill> {
    let document = Html::parse_document(html);
    let item_sel = Selector::parse("div.bill-item").unwrap();
    let name_sel = Selector::parse("h3.bill-name").unwrap();
    let year_sel = Selector::parse("span.bill-year").unwrap();
    let stage_sel = Selector::parse("div.bill-stage").unwrap();

    document
        .select(&item_sel)
        .filter_map(|item| {
            let name = item
                .select(&name_sel)
                .next()
                .map(|e| normalize_whitespace(&elem_text(e)))
                .filter(|s| !s.is_empty())?;

            let year = item
                .select(&year_sel)
                .next()
                .map(|e| normalize_whitespace(&elem_text(e)))
                .unwrap_or_default();

            let status = item
                .select(&stage_sel)
                .next()
                .map(|e| {
                    normalize_whitespace(&elem_text(e))
                        .strip_prefix("Status:")
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| normalize_whitespace(&elem_text(e)))
                })
                .unwrap_or_default();

            Some(Bill { name, year, status })
        })
        .collect()
}

pub fn parse_voting_patterns(html: &str) -> Vec<VoteRecord> {
    let document = Html::parse_document(html);
    let row_sel = Selector::parse("div.voting-patterns-row").unwrap();
    let date_sel = Selector::parse("div.voting-cell.voting-date").unwrap();
    let title_sel = Selector::parse("div.voting-cell.voting-title a").unwrap();
    let decision_sel =
        Selector::parse("div.voting-cell.voting-decision span.decision-badge").unwrap();

    document
        .select(&row_sel)
        .filter_map(|row| {
            let date = row
                .select(&date_sel)
                .next()
                .map(|e| normalize_whitespace(&elem_text(e)))?;

            let title_elem = row.select(&title_sel).next()?;
            let title = normalize_whitespace(&elem_text(title_elem));
            let url = title_elem.value().attr("href").map(str::to_string);

            let decision = row
                .select(&decision_sel)
                .next()
                .map(|e| normalize_whitespace(&elem_text(e)))
                .unwrap_or_default();

            Some(VoteRecord {
                date,
                title,
                url,
                decision,
            })
        })
        .collect()
}

pub fn parse_activity_page_info(html: &str) -> Option<(u32, u32)> {
    let document = Html::parse_document(html);

    let active_sel =
        Selector::parse("nav.contributions-pagination li.active_number_box span").unwrap();
    let current_page = document
        .select(&active_sel)
        .next()
        .and_then(|e| normalize_whitespace(&elem_text(e)).parse::<u32>().ok())?;

    let link_sel = Selector::parse("nav.contributions-pagination a[href]").unwrap();
    let total_pages = document
        .select(&link_sel)
        .filter_map(|e| {
            let href = e.value().attr("href")?;
            let after = href.split("contributions_page=").nth(1)?;
            after
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(current_page);

    Some((current_page, total_pages))
}

pub fn parse_parliamentary_activity(html: &str) -> Vec<ParliamentaryActivity> {
    let document = Html::parse_document(html);
    let group_sel = Selector::parse("div.contribution-group").unwrap();
    let topic_sel = Selector::parse("span.topic-badge.topic-badge-large").unwrap();
    let date_sel = Selector::parse("span.group-date").unwrap();
    let subgroup_sel = Selector::parse("div.conversation-subgroup").unwrap();
    let type_sel = Selector::parse("span.conversation-type-badge").unwrap();
    let title_sel = Selector::parse("a.conversation-title").unwrap();
    let item_sel = Selector::parse("div.contribution-item").unwrap();
    let link_sel = Selector::parse("a.contribution-text-link").unwrap();
    let text_sel = Selector::parse("p.contribution-text").unwrap();

    let mut items = Vec::new();

    for group in document.select(&group_sel) {
        let topic = group
            .select(&topic_sel)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .unwrap_or_default();

        let date = group
            .select(&date_sel)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .unwrap_or_default();

        for subgroup in group.select(&subgroup_sel) {
            let contribution_type = subgroup
                .select(&type_sel)
                .next()
                .map(|e| normalize_whitespace(&elem_text(e)))
                .unwrap_or_default();

            let (section_title, sitting_url) = subgroup
                .select(&title_sel)
                .next()
                .map(|e| {
                    let title = normalize_whitespace(&elem_text(e));
                    let raw_url = e.value().attr("href").unwrap_or("").to_string();
                    let sitting_url = raw_url.split('#').next().unwrap_or(&raw_url).to_string();
                    (title, sitting_url)
                })
                .unwrap_or_default();

            for item in subgroup.select(&item_sel) {
                let Some(link) = item.select(&link_sel).next() else {
                    continue;
                };
                let url = link.value().attr("href").unwrap_or("").to_string();
                let text_preview = link
                    .select(&text_sel)
                    .next()
                    .map(|e| normalize_whitespace(&elem_text(e)))
                    .unwrap_or_default();

                if url.is_empty() {
                    continue;
                }

                items.push(ParliamentaryActivity {
                    date: date.clone(),
                    topic: topic.clone(),
                    contribution_type: contribution_type.clone(),
                    section_title: section_title.clone(),
                    sitting_url: sitting_url.clone(),
                    text_preview,
                    url,
                });
            }
        }
    }

    items
}

pub fn parse_hansard_list(
    html: &str,
    house_filter: Option<House>,
) -> Result<Vec<HansardListing>, ParseError> {
    let document = Html::parse_document(html);
    let split_selector = Selector::parse("div.split-docs").unwrap();
    let link_selector = Selector::parse("div.hansard-document h3 a").unwrap();

    let mut listings = Vec::new();

    for (i, split_div) in document.select(&split_selector).enumerate() {
        let house = if i == 0 {
            House::NationalAssembly
        } else {
            House::Senate
        };

        if house_filter.as_ref().is_some_and(|f| f != &house) {
            continue;
        }

        for link_elem in split_div.select(&link_selector) {
            let url = match link_elem.value().attr("href") {
                Some(href) => href.to_string(),
                None => continue,
            };

            let title = normalize_whitespace(&elem_text(link_elem));
            if title.is_empty() {
                continue;
            }

            match parse_date_from_title(&title) {
                Ok((date, _, session_type)) => {
                    listings.push(HansardListing {
                        house,
                        date,
                        session_type,
                        url,
                        title,
                    });
                }
                Err(e) => log::warn!("Skipping listing '{}': {}", title, e),
            }
        }
    }

    Ok(listings)
}

pub fn parse_hansard_sitting(html: &str, url: &str) -> Result<HansardSitting, ParseError> {
    let document = Html::parse_document(html);

    let house_selector = Selector::parse("span.house").unwrap();
    let house_text = document
        .select(&house_selector)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .unwrap_or_default();

    let house = if house_text.contains("National Assembly") {
        House::NationalAssembly
    } else if house_text.contains("Senate") {
        House::Senate
    } else {
        let house_title_sel = Selector::parse("h1.house-title").unwrap();
        let house_title = document
            .select(&house_title_sel)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .unwrap_or_default();
        if house_title.contains("NATIONAL ASSEMBLY") {
            House::NationalAssembly
        } else {
            House::Senate
        }
    };

    let breadcrumb_sel = Selector::parse("li.breadcrumb-item.current").unwrap();
    let breadcrumb_text = document
        .select(&breadcrumb_sel)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .unwrap_or_default();

    let (date, day_of_week, session_type) = if !breadcrumb_text.is_empty() {
        parse_date_from_title(&breadcrumb_text).or_else(|_| parse_date_from_url_slug(url))?
    } else {
        parse_date_from_url_slug(url)?
    };

    let session_sel = Selector::parse("span.session").unwrap();
    let session_type = document
        .select(&session_sel)
        .next()
        .map(|e| {
            normalize_whitespace(&elem_text(e))
                .replace("Session:", "")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or(session_type);

    let time_sel = Selector::parse("span.time").unwrap();
    let time = document
        .select(&time_sel)
        .next()
        .map(|e| {
            normalize_whitespace(&elem_text(e))
                .replace("Time:", "")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .and_then(|t| parse_time_12h(&t).ok());

    let pdf_sel = Selector::parse("div.document-thumbnail a").unwrap();
    let pdf_url = document
        .select(&pdf_sel)
        .next()
        .and_then(|e| e.value().attr("href"))
        .filter(|h| h.ends_with(".pdf"))
        .map(str::to_string);

    let doc_summary_sel = Selector::parse("div.doc-summary").unwrap();
    let (summary, sentiment) = document
        .select(&doc_summary_sel)
        .next()
        .map(|elem| parse_doc_summary(elem))
        .unwrap_or((None, None));

    let sections = parse_sitting_sections(&document)?;

    Ok(HansardSitting {
        house,
        date,
        day_of_week,
        session_type,
        time,
        summary,
        sentiment,
        pdf_url,
        sections,
    })
}

fn parse_doc_summary(elem: ElementRef) -> (Option<String>, Option<String>) {
    let full = normalize_whitespace(&elem_text(elem));

    let body = full
        .strip_prefix("Hansard Summary")
        .map(|s| s.trim())
        .unwrap_or(full.as_str());

    let (summary_raw, sentiment_raw) = if let Some(pos) = body.find("Sentimental Analysis") {
        let s = body[..pos].trim();
        let rest = body[pos..]
            .strip_prefix("Sentimental Analysis")
            .map(|r| r.trim())
            .unwrap_or("");
        (s, rest)
    } else {
        (body, "")
    };

    let summary = if summary_raw.is_empty() {
        None
    } else {
        Some(summary_raw.to_string())
    };

    let sentiment = if sentiment_raw.is_empty() {
        None
    } else {
        Some(sentiment_raw.to_string())
    };

    (summary, sentiment)
}

fn parse_sitting_sections(document: &Html) -> Result<Vec<HansardSection>, ParseError> {
    let article_sel = Selector::parse("article.hansard-document").unwrap();
    let Some(article) = document.select(&article_sel).next() else {
        return Ok(Vec::new());
    };

    let mut sections: Vec<HansardSection> = Vec::new();
    let mut current_section: Option<HansardSection> = None;
    let mut pending_speaker: Option<(String, Option<String>)> = None;

    for child in article.children() {
        let Some(element) = ElementRef::wrap(child) else {
            continue;
        };

        let tag = element.value().name();
        let class = element.value().attr("class").unwrap_or("");

        if tag == "h2" && class.contains("major-section-header") {
            flush_pending_speaker(&mut pending_speaker, &mut current_section);

            if let Some(section) = current_section.take() {
                sections.push(section);
            }

            let heading = normalize_whitespace(&elem_text(element));
            if !heading.is_empty() {
                current_section = Some(HansardSection {
                    section_type: heading,
                    contributions: Vec::new(),
                });
            }
        } else if tag == "h2" && class.contains("header-section") {
            flush_pending_speaker(&mut pending_speaker, &mut current_section);
        } else if tag == "div" && class.contains("contributor-name") {
            flush_pending_speaker(&mut pending_speaker, &mut current_section);

            let a_sel = Selector::parse("a").unwrap();
            let (name, speaker_url) = if let Some(a) = element.select(&a_sel).next() {
                let name = normalize_whitespace(&elem_text(a));
                let url = a.value().attr("href").map(str::to_string);
                (name, url)
            } else {
                (normalize_whitespace(&elem_text(element)), None)
            };

            if !name.is_empty() {
                pending_speaker = Some((name, speaker_url));
            }
        } else if tag == "div" && class.contains("speech-content") {
            if let Some((name, url)) = pending_speaker.take() {
                let p_sel = Selector::parse("p").unwrap();
                let procedural_sel = Selector::parse("aside.procedural-note").unwrap();

                let content = element
                    .select(&p_sel)
                    .map(|p| normalize_whitespace(&elem_text(p)))
                    .collect::<Vec<_>>()
                    .join("\n\n");

                let procedural_notes = element
                    .select(&procedural_sel)
                    .map(|a| normalize_whitespace(&elem_text(a)))
                    .collect();

                if let Some(ref mut section) = current_section {
                    section.contributions.push(Contribution {
                        speaker_name: name,
                        speaker_url: url,
                        content,
                        procedural_notes,
                    });
                }
            }
        } else if tag == "div" && class.contains("scene-description") {
            let scene = normalize_whitespace(&elem_text(element));
            if !scene.is_empty()
                && let Some(ref mut section) = current_section
                && let Some(last) = section.contributions.last_mut()
            {
                last.procedural_notes.push(scene);
            }
        }
    }

    flush_pending_speaker(&mut pending_speaker, &mut current_section);

    if let Some(section) = current_section {
        sections.push(section);
    }

    Ok(sections)
}

fn flush_pending_speaker(
    pending: &mut Option<(String, Option<String>)>,
    section: &mut Option<HansardSection>,
) {
    if let Some((name, url)) = pending.take()
        && let Some(s) = section
    {
        s.contributions.push(Contribution {
            speaker_name: name,
            speaker_url: url,
            content: String::new(),
            procedural_notes: Vec::new(),
        });
    }
}

pub fn parse_member_list(html: &str, house: House) -> Result<Vec<Member>, ParseError> {
    let document = Html::parse_document(html);
    let item_sel = Selector::parse("a.members-list--item").unwrap();
    let name_sel = Selector::parse("div.members-list--name").unwrap();
    let leader_role_sel = Selector::parse("p.leader-role").unwrap();
    let repr_sel = Selector::parse("div.members-list--representation").unwrap();

    let mut members = Vec::new();

    for item in document.select(&item_sel) {
        let url = match item.value().attr("href") {
            Some(href) => href.to_string(),
            None => continue,
        };

        let name = item
            .select(&name_sel)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .unwrap_or_default();

        if name.is_empty() {
            continue;
        }

        let role = item
            .select(&leader_role_sel)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .filter(|s| !s.is_empty());

        let constituency = item
            .select(&repr_sel)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .filter(|s| !s.is_empty());

        members.push(Member {
            name,
            url,
            house,
            role,
            constituency,
        });
    }

    Ok(members)
}

pub fn parse_member_profile(html: &str, url: &str) -> Result<MemberProfile, ParseError> {
    let document = Html::parse_document(html);

    let slug = url
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .ok_or_else(|| ParseError::UrlParse("Could not extract slug from URL".to_string()))?
        .to_string();

    let name_sel = Selector::parse("h1.page-heading").unwrap();
    let name = document
        .select(&name_sel)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .ok_or_else(|| ParseError::MissingField("member name".to_string()))?;

    let bio_sel = Selector::parse("section.member-biography div.biography-content").unwrap();
    let biography = document
        .select(&bio_sel)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty());

    let position_type_sel = Selector::parse("h2.assembly-entry").unwrap();
    let position_type = document
        .select(&position_type_sel)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty());

    let elected_post_sel = Selector::parse("p.elected-post").unwrap();
    let mut elected_posts = document.select(&elected_post_sel);
    let position = elected_posts
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty());
    let party = elected_posts
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty());

    let committee_sel = Selector::parse("li.committee-item").unwrap();
    let committees = document
        .select(&committee_sel)
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty())
        .collect();

    let activity_sel = Selector::parse("div.activity-section p").unwrap();
    let (speeches_last_year, speeches_total) = document
        .select(&activity_sel)
        .next()
        .and_then(|e| {
            let text = elem_text(e);
            let caps = RE_SPEECHES.captures(&text)?;
            let last_year: u32 = caps[1].parse().ok()?;
            let total: u32 = caps[2].parse().ok()?;
            Some((Some(last_year), Some(total)))
        })
        .unwrap_or((None, None));

    let bills_summary_sel = Selector::parse("p.bills-summary").unwrap();
    let bills_total = document.select(&bills_summary_sel).next().and_then(|e| {
        let text = elem_text(e);
        let caps = RE_BILLS_TOTAL.captures(&text)?;
        caps[1].parse::<u32>().ok()
    });

    let bills = parse_bills(html);

    let bills_pages = parse_bills_page_info(html)
        .map(|(_, total)| total)
        .unwrap_or(if bills.is_empty() { 0 } else { 1 });

    let voting_patterns = parse_voting_patterns(html);

    let activity = parse_parliamentary_activity(html);

    let activity_pages = parse_activity_page_info(html)
        .map(|(_, total)| total)
        .unwrap_or(if activity.is_empty() { 0 } else { 1 });

    Ok(MemberProfile {
        name,
        slug,
        biography,
        position_type,
        position,
        party,
        committees,
        speeches_last_year,
        speeches_total,
        bills,
        bills_total,
        bills_pages,
        voting_patterns,
        activity,
        activity_pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_page_info_hansard_list() {
        let html = fs::read_to_string("fixtures/current/Hansard_list_paginated")
            .expect("Failed to read fixture");

        let (current, total) = parse_page_info(&html).expect("Should parse pagination");
        assert_eq!(current, 1);
        assert_eq!(total, 120);
    }

    #[test]
    fn test_parse_page_info_member_list() {
        let html =
            fs::read_to_string("fixtures/current/national_assembly_13th_parliament_paginated")
                .expect("Failed to read fixture");

        let (current, total) = parse_page_info(&html).expect("Should parse pagination");
        assert_eq!(current, 1);
        assert_eq!(total, 8);
    }

    #[test]
    fn test_parse_hansard_list_from_fixture() {
        let html = fs::read_to_string("fixtures/current/Hansard_list_paginated")
            .expect("Failed to read fixture");

        let listings = parse_hansard_list(&html, None).expect("Failed to parse hansard list");

        assert!(!listings.is_empty(), "Should parse at least one listing");
        println!("Parsed {} listings", listings.len());

        let na = listings
            .iter()
            .filter(|l| l.house == House::NationalAssembly)
            .count();
        let senate = listings.iter().filter(|l| l.house == House::Senate).count();
        assert!(na > 0, "Should have National Assembly listings");
        assert!(senate > 0, "Should have Senate listings");

        let first = &listings[0];
        assert_eq!(first.house, House::NationalAssembly);
        assert!(
            first.session_type.contains("Sitting"),
            "Session type should contain 'Sitting'"
        );
        println!("First listing: {}", first);
    }

    #[test]
    fn test_parse_hansard_list_filter_national_assembly() {
        let html = fs::read_to_string("fixtures/current/Hansard_list_paginated")
            .expect("Failed to read fixture");

        let listings = parse_hansard_list(&html, Some(House::NationalAssembly))
            .expect("Failed to parse hansard list");

        assert!(!listings.is_empty(), "Should have listings");
        assert!(
            listings.iter().all(|l| l.house == House::NationalAssembly),
            "All listings should be National Assembly"
        );
    }

    #[test]
    fn test_parse_hansard_list_filter_senate() {
        let html = fs::read_to_string("fixtures/current/Hansard_list_paginated")
            .expect("Failed to read fixture");

        let listings =
            parse_hansard_list(&html, Some(House::Senate)).expect("Failed to parse hansard list");

        assert!(!listings.is_empty(), "Should have listings");
        assert!(
            listings.iter().all(|l| l.house == House::Senate),
            "All listings should be Senate"
        );
    }

    #[test]
    fn test_parse_hansard_list_filter_excludes_other_house() {
        let html = fs::read_to_string("fixtures/current/Hansard_list_paginated")
            .expect("Failed to read fixture");

        let na = parse_hansard_list(&html, Some(House::NationalAssembly))
            .expect("Failed to parse NA listings");
        let senate = parse_hansard_list(&html, Some(House::Senate))
            .expect("Failed to parse Senate listings");
        let all = parse_hansard_list(&html, None).expect("Failed to parse all listings");

        assert_eq!(
            na.len() + senate.len(),
            all.len(),
            "Filtered counts should sum to total"
        );
    }

    #[test]
    fn test_parse_hansard_list_specific_entries() {
        let html = fs::read_to_string("fixtures/current/Hansard_list_paginated")
            .expect("Failed to read fixture");

        let listings = parse_hansard_list(&html, None).expect("Failed to parse");

        let feb12 = listings
            .iter()
            .find(|l| {
                l.date == chrono::NaiveDate::from_ymd_opt(2026, 2, 12).unwrap()
                    && l.house == House::NationalAssembly
            })
            .expect("Should find 12th Feb 2026 NA entry");

        assert_eq!(feb12.session_type, "Afternoon Sitting");
        assert!(feb12.url.contains("2438"), "URL should contain sitting ID");
    }

    #[test]
    fn test_parse_national_assembly_sitting() {
        let html = fs::read_to_string("fixtures/current/national_assembly_hansard_sitting")
            .expect("Failed to read fixture");
        let url = "https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2438/";

        let sitting = parse_hansard_sitting(&html, url).expect("Failed to parse sitting");

        assert_eq!(sitting.house, House::NationalAssembly);
        assert_eq!(sitting.date.to_string(), "2026-02-12");
        assert_eq!(sitting.session_type, "Afternoon Sitting");
        assert!(sitting.time.is_some(), "Should have a time");
        assert!(sitting.summary.is_some(), "Should have a summary");
        assert!(sitting.pdf_url.is_some(), "Should have a PDF URL");
        assert!(
            !sitting.sections.is_empty(),
            "Should have at least one section"
        );

        let has_contributions = sitting.sections.iter().any(|s| !s.contributions.is_empty());
        assert!(has_contributions, "Should have at least one contribution");

        println!("Sitting: {}", sitting);
    }

    #[test]
    fn test_parse_senate_sitting() {
        let html = fs::read_to_string("fixtures/current/senate_hansard_sitting")
            .expect("Failed to read fixture");
        let url = "https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2434/";

        let sitting = parse_hansard_sitting(&html, url).expect("Failed to parse sitting");

        assert_eq!(sitting.house, House::Senate);
        assert_eq!(sitting.date.to_string(), "2026-02-12");
        assert!(!sitting.sections.is_empty(), "Should have sections");
    }

    #[test]
    fn test_parse_sitting_contributions_have_speaker_urls() {
        let html = fs::read_to_string("fixtures/current/national_assembly_hansard_sitting")
            .expect("Failed to read fixture");
        let url = "https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2438/";

        let sitting = parse_hansard_sitting(&html, url).expect("Failed to parse sitting");

        let with_url = sitting
            .sections
            .iter()
            .flat_map(|s| &s.contributions)
            .any(|c| c.speaker_url.is_some());
        assert!(
            with_url,
            "Should have at least one contribution with a speaker URL"
        );
    }

    #[test]
    fn test_parse_member_list() {
        let html =
            fs::read_to_string("fixtures/current/national_assembly_13th_parliament_paginated")
                .expect("Failed to read fixture");

        let members =
            parse_member_list(&html, House::NationalAssembly).expect("Failed to parse members");

        assert!(!members.is_empty(), "Should parse at least one member");
        assert!(
            members.iter().all(|m| m.house == House::NationalAssembly),
            "All members should be National Assembly"
        );

        let speaker = members
            .iter()
            .find(|m| {
                m.name.contains("Wetangula") || m.role.as_deref().unwrap_or("").contains("Speaker")
            })
            .expect("Should find the Speaker");
        assert!(speaker.role.is_some(), "Speaker should have a role");

        println!("Parsed {} members", members.len());
    }

    #[test]
    fn test_parse_member_profile() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");
        let url = "https://mzalendo.com/mps-performance/national-assembly/13th-parliament/boss-gladys-jepkosgei/";

        let profile = parse_member_profile(&html, url).expect("Failed to parse member profile");

        assert_eq!(profile.name, "Boss Gladys Jepkosgei");
        assert_eq!(profile.slug, "boss-gladys-jepkosgei");
        assert!(profile.biography.is_some(), "Should have biography");
        assert!(profile.position.is_some(), "Should have position");
        assert!(profile.party.is_some(), "Should have party");
        assert!(!profile.committees.is_empty(), "Should have committees");
        assert_eq!(profile.speeches_last_year, Some(514));
        assert_eq!(profile.speeches_total, Some(675));
        assert_eq!(profile.bills_total, Some(8));
        assert!(!profile.bills.is_empty(), "Should have bills");
        assert_eq!(profile.bills_pages, 2);
        assert!(
            !profile.voting_patterns.is_empty(),
            "Should have voting records"
        );

        println!("{}", profile);
    }

    #[test]
    fn test_parse_activity_page_info() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");

        let (current, total) =
            parse_activity_page_info(&html).expect("Should parse activity pagination");
        assert_eq!(current, 1);
        assert_eq!(total, 11);
    }

    #[test]
    fn test_parse_parliamentary_activity() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");

        let items = parse_parliamentary_activity(&html);

        assert!(!items.is_empty(), "Should parse at least one activity item");
        for item in &items {
            assert!(!item.date.is_empty(), "Date should not be empty");
            assert!(!item.topic.is_empty(), "Topic should not be empty");
            assert!(!item.url.is_empty(), "URL should not be empty");
            assert!(
                item.url.contains("#chunk-"),
                "URL should link to a specific chunk"
            );
            assert!(
                !item.sitting_url.contains('#'),
                "sitting_url should have no fragment"
            );
        }
        println!("Parsed {} activity items", items.len());
        println!("First: {}", items[0]);
    }

    #[test]
    fn test_parse_member_profile_activity() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");
        let url = "https://mzalendo.com/mps-performance/national-assembly/13th-parliament/boss-gladys-jepkosgei/";

        let profile = parse_member_profile(&html, url).expect("Failed to parse member profile");

        assert!(!profile.activity.is_empty(), "Should have activity items");
        assert_eq!(profile.activity_pages, 11);
    }

    #[test]
    fn test_parse_bills() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");

        let bills = parse_bills(&html);

        assert!(!bills.is_empty(), "Should parse at least one bill");
        let first = &bills[0];
        assert!(!first.name.is_empty(), "Bill name should not be empty");
        assert!(!first.year.is_empty(), "Bill year should not be empty");
        assert!(!first.status.is_empty(), "Bill status should not be empty");
        assert!(
            !first.status.starts_with("Status:"),
            "Status prefix should be stripped"
        );
        println!("First bill: {}", first);
    }

    #[test]
    fn test_parse_bills_page_info() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");

        let (current, total) = parse_bills_page_info(&html).expect("Should parse bills pagination");
        assert_eq!(current, 1);
        assert_eq!(total, 2);
    }

    #[test]
    fn test_parse_voting_patterns() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");

        let votes = parse_voting_patterns(&html);

        assert!(!votes.is_empty(), "Should parse at least one vote record");
        for vote in &votes {
            assert!(!vote.date.is_empty(), "Date should not be empty");
            assert!(!vote.title.is_empty(), "Title should not be empty");
            assert!(!vote.decision.is_empty(), "Decision should not be empty");
            assert!(vote.url.is_some(), "Should have a URL");
        }
        println!("Parsed {} vote records", votes.len());
        println!("First vote: {}", votes[0]);
    }

    #[test]
    fn test_parse_date_from_title() {
        let cases = [
            (
                "Thursday, 12th February, 2026 - Afternoon Sitting",
                (2026i32, 2u32, 12u32),
                "Thursday",
                "Afternoon Sitting",
            ),
            (
                "Wednesday, 26th November, 2025 - Morning Sitting",
                (2025, 11, 26),
                "Wednesday",
                "Morning Sitting",
            ),
            (
                "Hansard Report - Thursday, 4th December 2025 - Evening Sitting",
                (2025, 12, 4),
                "Thursday",
                "Evening Sitting",
            ),
        ];

        for (title, (year, month, day), weekday, session) in cases {
            let (date, dow, sess) = parse_date_from_title(title)
                .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", title, e));
            assert_eq!(date, NaiveDate::from_ymd_opt(year, month, day).unwrap());
            assert_eq!(dow.to_lowercase(), weekday.to_lowercase());
            assert_eq!(sess, session);
        }
    }

    #[test]
    fn test_parse_time_12h() {
        assert_eq!(
            parse_time_12h("2:30 PM").unwrap(),
            NaiveTime::from_hms_opt(14, 30, 0).unwrap()
        );
        assert_eq!(
            parse_time_12h("10:00 AM").unwrap(),
            NaiveTime::from_hms_opt(10, 0, 0).unwrap()
        );
        assert_eq!(
            parse_time_12h("12:00 PM").unwrap(),
            NaiveTime::from_hms_opt(12, 0, 0).unwrap()
        );
        assert_eq!(
            parse_time_12h("12:00 AM").unwrap(),
            NaiveTime::from_hms_opt(0, 0, 0).unwrap()
        );
    }
}
