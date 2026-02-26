use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveTime};
use regex::Regex;
use scraper::{ElementRef, Html, Selector, error::SelectorErrorKind};

use super::types::{
    Bill, Contribution, HansardListing, HansardSection, HansardSitting, HansardSubsection, House,
    Member, MemberProfile, ParliamentaryActivity, VoteRecord,
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
    #[error("Failed to parse selector: {0}")]
    HtmlSelector(String),
}

impl<'a> From<SelectorErrorKind<'a>> for ParseError {
    fn from(err: SelectorErrorKind<'a>) -> Self {
        ParseError::HtmlSelector(format!("{err:?}"))
    }
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

pub fn parse_page_info(html: &str) -> Result<Option<(u32, u32)>, ParseError> {
    let document = Html::parse_document(html);

    let active_sel = Selector::parse("li.active.active_number_box span")?;
    let current_page = document
        .select(&active_sel)
        .next()
        .and_then(|e| normalize_whitespace(&elem_text(e)).parse::<u32>().ok())
        .ok_or_else(|| ParseError::MissingField("Missing pagination elements".to_string()))?;

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
        .max()
        .unwrap_or(current_page);

    Ok(Some((current_page, total_pages)))
}

pub fn parse_bills_page_info(html: &str) -> Result<Option<(u32, u32)>, ParseError> {
    let document = Html::parse_document(html);

    let active_sel = Selector::parse("nav.bills-pagination li.active_number_box span")?;
    let current_page = document
        .select(&active_sel)
        .next()
        .and_then(|e| normalize_whitespace(&elem_text(e)).parse::<u32>().ok())
        .ok_or_else(|| ParseError::MissingField("Missing pagination elements".to_string()))?;

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

    Ok(Some((current_page, total_pages)))
}

pub fn parse_bills(html: &str) -> Result<Vec<Bill>, ParseError> {
    let document = Html::parse_document(html);
    let item_sel = Selector::parse("div.bill-item")?;
    let name_sel = Selector::parse("h3.bill-name")?;
    let year_sel = Selector::parse("span.bill-year")?;
    let stage_sel = Selector::parse("div.bill-stage")?;

    let bills = document
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
        .collect();

    Ok(bills)
}

pub fn parse_voting_patterns(html: &str) -> Result<Vec<VoteRecord>, ParseError> {
    let document = Html::parse_document(html);
    let row_sel = Selector::parse("div.voting-patterns-row")?;
    let date_sel = Selector::parse("div.voting-cell.voting-date")?;
    let title_sel = Selector::parse("div.voting-cell.voting-title a")?;
    let decision_sel = Selector::parse("div.voting-cell.voting-decision span.decision-badge")?;

    let vote_records = document
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
        .collect();

    Ok(vote_records)
}

pub fn parse_activity_page_info(html: &str) -> Result<Option<(u32, u32)>, ParseError> {
    let document = Html::parse_document(html);

    let active_sel = Selector::parse("nav.contributions-pagination li.active_number_box span")?;
    let current_page = document
        .select(&active_sel)
        .next()
        .and_then(|e| normalize_whitespace(&elem_text(e)).parse::<u32>().ok())
        .ok_or_else(|| ParseError::MissingField("Missing pagination elements".to_string()))?;

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

    Ok(Some((current_page, total_pages)))
}

pub fn parse_parliamentary_activity(html: &str) -> Result<Vec<ParliamentaryActivity>, ParseError> {
    let document = Html::parse_document(html);
    let group_sel = Selector::parse("div.contribution-group")?;
    let topic_sel = Selector::parse("span.topic-badge.topic-badge-large")?;
    let date_sel = Selector::parse("span.group-date")?;
    let subgroup_sel = Selector::parse("div.conversation-subgroup")?;
    let type_sel = Selector::parse("span.conversation-type-badge")?;
    let title_sel = Selector::parse("a.conversation-title")?;
    let item_sel = Selector::parse("div.contribution-item")?;
    let link_sel = Selector::parse("a.contribution-text-link")?;
    let text_sel = Selector::parse("p.contribution-text")?;

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

    Ok(items)
}

pub fn parse_hansard_list(
    html: &str,
    house_filter: Option<House>,
) -> Result<Vec<HansardListing>, ParseError> {
    let document = Html::parse_document(html);
    let split_selector = Selector::parse("div.split-docs")?;
    let link_selector = Selector::parse("div.hansard-document h3 a")?;

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

    let house_selector = Selector::parse("span.house")?;
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
        let house_title_sel = Selector::parse("h1.house-title")?;
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

    let breadcrumb_sel = Selector::parse("li.breadcrumb-item.current")?;
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

    // XXX: do not trust span.session — it can contain stale/incorrect metadata on the site
    // (e.g. shows "Afternoon Sitting" for a morning sitting). the breadcrumb and URL slug
    // parsed above are the authoritative source for session_type.

    let time_sel = Selector::parse("span.time")?;
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

    let pdf_sel = Selector::parse("div.document-thumbnail a")?;
    let pdf_url = document
        .select(&pdf_sel)
        .next()
        .and_then(|e| e.value().attr("href"))
        .filter(|h| h.ends_with(".pdf"))
        .map(str::to_string);

    let doc_summary_sel = Selector::parse("div.doc-summary")?;
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
            .unwrap_or_default();
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
    // XXX: support both HTML formats:
    //   old: article.hansard-document → semantic elements as direct children
    //   new: div.hansard-content → div.chunk-wrapper → semantic elements
    let article_sel = Selector::parse("article.hansard-document")?;
    let content_sel = Selector::parse("div.hansard-content")?;

    let container = document
        .select(&article_sel)
        .next()
        .or_else(|| document.select(&content_sel).next());

    let Some(container) = container else {
        return Ok(Vec::new());
    };

    // XXX: flatten chunk-wrappers so the state machine sees a uniform element stream
    // regardless of format. in the new format contributor-name and speech-content
    // are paired inside the same chunk-wrapper; unwrapping produces the same
    // sequential order as the old format.
    let elements: Vec<ElementRef> = container
        .children()
        .filter_map(ElementRef::wrap)
        .flat_map(|child| -> Vec<ElementRef> {
            let tag = child.value().name();
            let class = child.value().attr("class").unwrap_or_default();
            if tag == "div" && class.contains("chunk-wrapper") {
                child.children().filter_map(ElementRef::wrap).collect()
            } else {
                vec![child]
            }
        })
        .collect();

    let mut sections: Vec<HansardSection> = Vec::new();
    let mut current_section: Option<HansardSection> = None;
    let mut current_subsection: Option<HansardSubsection> = None;
    let mut pending_speaker: Option<(String, Option<String>)> = None;

    for element in elements {
        let tag = element.value().name();
        let class = element.value().attr("class").unwrap_or_default();

        if tag == "h2" && class.contains("major-section-header") {
            if let Some(contrib) = take_pending_contribution(&mut pending_speaker) {
                push_contribution(contrib, &mut current_subsection, &mut current_section);
            }
            flush_subsection(&mut current_subsection, &mut current_section);
            if let Some(section) = current_section.take() {
                sections.push(section);
            }

            let heading = normalize_whitespace(&elem_text(element));
            if !heading.is_empty() {
                current_section = Some(HansardSection {
                    section_type: heading,
                    subsections: Vec::new(),
                    contributions: Vec::new(),
                });
            }
        } else if tag == "h2" && class.contains("header-section") {
            if let Some(contrib) = take_pending_contribution(&mut pending_speaker) {
                push_contribution(contrib, &mut current_subsection, &mut current_section);
            }
            flush_subsection(&mut current_subsection, &mut current_section);

            let heading = normalize_whitespace(&elem_text(element));
            if !heading.is_empty() {
                // XXX: for resumption sittings there may be no preceding major-section-header;
                // create an implicit unnamed section so subsections are not silently dropped.
                if current_section.is_none() {
                    current_section = Some(HansardSection {
                        section_type: String::new(),
                        subsections: Vec::new(),
                        contributions: Vec::new(),
                    });
                }
                current_subsection = Some(HansardSubsection {
                    title: heading,
                    contributions: Vec::new(),
                });
            }
        } else if tag == "div" && class.contains("contributor-name") {
            if let Some(contrib) = take_pending_contribution(&mut pending_speaker) {
                push_contribution(contrib, &mut current_subsection, &mut current_section);
            }

            let a_sel = Selector::parse("a")?;
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
                let p_sel = Selector::parse("p")?;
                let procedural_sel = Selector::parse("aside.procedural-note")?;

                let content = element
                    .select(&p_sel)
                    .map(|p| normalize_whitespace(&elem_text(p)))
                    .collect::<Vec<_>>()
                    .join("\n\n");

                let procedural_notes = element
                    .select(&procedural_sel)
                    .map(|a| normalize_whitespace(&elem_text(a)))
                    .collect();

                push_contribution(
                    Contribution {
                        speaker_name: name,
                        speaker_url: url,
                        content,
                        procedural_notes,
                    },
                    &mut current_subsection,
                    &mut current_section,
                );
            }
        } else if tag == "div" && class.contains("scene-description") {
            let scene = normalize_whitespace(&elem_text(element));
            if !scene.is_empty() {
                if let Some(ref mut sub) = current_subsection {
                    if let Some(last) = sub.contributions.last_mut() {
                        last.procedural_notes.push(scene);
                    }
                } else if let Some(ref mut sec) = current_section
                    && let Some(last) = sec.contributions.last_mut()
                {
                    last.procedural_notes.push(scene);
                }
            }
        } else if tag == "p" {
            let text = normalize_whitespace(&elem_text(element));
            if !text.is_empty() {
                append_text_to_active("\n\n", text, &mut current_subsection, &mut current_section);
            }
        } else if tag == "ol" && class.contains("content-list") {
            // XXX: auto-generated list from PDF conversion — often a direct continuation of a
            // preceding <p> (e.g. bill metadata or NG-CDF constituency lists). flatten all
            // <li> text and append with a space so it reads as part of the same sentence.
            let li_sel = Selector::parse("li")?;
            let text = element
                .select(&li_sel)
                .map(|li| normalize_whitespace(&elem_text(li)))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if !text.is_empty() {
                append_text_to_active(" ", text, &mut current_subsection, &mut current_section);
            }
        }
    }

    if let Some(contrib) = take_pending_contribution(&mut pending_speaker) {
        push_contribution(contrib, &mut current_subsection, &mut current_section);
    }
    flush_subsection(&mut current_subsection, &mut current_section);
    if let Some(section) = current_section {
        sections.push(section);
    }

    Ok(sections)
}

// XXX: appends `text` to the last contribution in the active target (subsection → section).
// `sep` is the separator inserted when content is non-empty (e.g. `"\n\n"` for paragraphs,
// `" "` for inline continuations like ol.content-list fragments).
fn append_text_to_active(
    sep: &str,
    text: String,
    current_subsection: &mut Option<HansardSubsection>,
    current_section: &mut Option<HansardSection>,
) {
    let target_contributions = if let Some(sub) = current_subsection {
        &mut sub.contributions
    } else if let Some(sec) = current_section {
        &mut sec.contributions
    } else {
        return;
    };

    if let Some(last) = target_contributions.last_mut() {
        if !last.content.is_empty() {
            last.content.push_str(sep);
        }
        last.content.push_str(&text);
    } else {
        target_contributions.push(Contribution {
            speaker_name: String::new(),
            speaker_url: None,
            content: text,
            procedural_notes: Vec::new(),
        });
    }
}

fn take_pending_contribution(
    pending: &mut Option<(String, Option<String>)>,
) -> Option<Contribution> {
    pending.take().map(|(name, url)| Contribution {
        speaker_name: name,
        speaker_url: url,
        content: String::new(),
        procedural_notes: Vec::new(),
    })
}

// XXX: pushes a contribution to the active subsection or section. if neither exists
// (content before any section header), creates an implicit unnamed section so
// contributions from resumption sittings are not silently dropped.
fn push_contribution(
    contrib: Contribution,
    current_subsection: &mut Option<HansardSubsection>,
    current_section: &mut Option<HansardSection>,
) {
    if let Some(sub) = current_subsection {
        sub.contributions.push(contrib);
    } else {
        let sec = current_section.get_or_insert_with(|| HansardSection {
            section_type: String::new(),
            subsections: Vec::new(),
            contributions: Vec::new(),
        });
        sec.contributions.push(contrib);
    }
}

fn flush_subsection(
    current_subsection: &mut Option<HansardSubsection>,
    current_section: &mut Option<HansardSection>,
) {
    if let Some(subsection) = current_subsection.take()
        && let Some(section) = current_section
    {
        section.subsections.push(subsection);
    }
}

pub fn parse_member_list(html: &str, house: House) -> Result<Vec<Member>, ParseError> {
    let document = Html::parse_document(html);
    let item_sel = Selector::parse("a.members-list--item, a.senators-list--item")?;
    let name_sel = Selector::parse("div.members-list--name, div.senators-list--name")?;
    let leader_role_sel = Selector::parse("p.leader-role")?;
    let repr_sel =
        Selector::parse("div.members-list--representation, div.senators-list--representation")?;

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

    let name_sel = Selector::parse("h1.page-heading")?;
    let name = document
        .select(&name_sel)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .ok_or_else(|| ParseError::MissingField("member name".to_string()))?;

    let bio_sel = Selector::parse("section.member-biography div.biography-content")?;
    let biography = document
        .select(&bio_sel)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty());

    let position_type_sel = Selector::parse("h2.assembly-entry")?;
    let position_type = document
        .select(&position_type_sel)
        .next()
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty());

    let photo_sel = Selector::parse("img.member-list--image")?;
    let photo_url = document
        .select(&photo_sel)
        .next()
        .and_then(|e| e.value().attr("src"))
        .map(str::to_string);

    let header_two_sel = Selector::parse("h2.header-two")?;
    let parties_heading_sel = Selector::parse("h2.header-two, h2.header-three")?;
    let p_sel = Selector::parse("p")?;

    // XXX: (positions) collect all p under "CURRENT POSITIONS" h2.header-two,
    // handling both NA (wrapped in div.position-section) and Senate (direct p.elected-post siblings).
    let positions: Vec<String> = document
        .select(&header_two_sel)
        .find(|h| elem_text(*h).contains("CURRENT POSITIONS"))
        .map(|h| {
            let mut results = Vec::new();
            for sibling in h.next_siblings().filter_map(ElementRef::wrap) {
                if sibling.value().name() == "h2" {
                    break;
                }
                if sibling.value().name() == "div"
                    && sibling
                        .value()
                        .attr("class")
                        .unwrap_or_default()
                        .contains("position-section")
                {
                    results.extend(
                        sibling
                            .select(&p_sel)
                            .map(|e| normalize_whitespace(&elem_text(e)))
                            .filter(|s| !s.is_empty()),
                    );
                } else if sibling.value().name() == "p" {
                    let text = normalize_whitespace(&elem_text(sibling));
                    if !text.is_empty() {
                        results.push(text);
                    }
                }
            }
            results
        })
        .unwrap_or_default();

    // XXX: (party) first p.elected-post that follows the "Parties and Coalitions" heading
    let party = document
        .select(&parties_heading_sel)
        .find(|h| elem_text(*h).contains("Parties"))
        .and_then(|h| {
            h.next_siblings().filter_map(ElementRef::wrap).find(|e| {
                e.value().name() == "p"
                    && e.value()
                        .attr("class")
                        .unwrap_or_default()
                        .contains("elected-post")
            })
        })
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty());

    let committee_sel = Selector::parse("li.committee-item")?;
    let committees = document
        .select(&committee_sel)
        .map(|e| normalize_whitespace(&elem_text(e)))
        .filter(|s| !s.is_empty())
        .collect();

    let activity_sel = Selector::parse("div.activity-section p")?;
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

    let bills = parse_bills(html)?;

    let bills_pages = parse_bills_page_info(html)?
        .map(|(_, total)| total)
        .unwrap_or(if bills.is_empty() { 0 } else { 1 });

    let voting_patterns = parse_voting_patterns(html)?;

    let activity = parse_parliamentary_activity(html)?;

    let activity_pages = parse_activity_page_info(html)?
        .map(|(_, total)| total)
        .unwrap_or(if activity.is_empty() { 0 } else { 1 });

    Ok(MemberProfile {
        name,
        slug,
        photo_url,
        biography,
        position_type,
        positions,
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

        let (current, total) = parse_page_info(&html)
            .unwrap()
            .expect("Should parse pagination");
        assert_eq!(current, 1);
        assert_eq!(total, 120);
    }

    #[test]
    fn test_parse_page_info_member_list() {
        let html =
            fs::read_to_string("fixtures/current/national_assembly_13th_parliament_paginated")
                .expect("Failed to read fixture");

        let (current, total) = parse_page_info(&html)
            .unwrap()
            .expect("Should parse pagination");
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
    fn test_parse_national_assembly_sitting_new_format() {
        let html =
            fs::read_to_string("fixtures/current/national_assembly_hansard_sitting_new_format")
                .expect("Failed to read new-format fixture");
        let url = "https://mzalendo.com/democracy-tools/hansard/thursday-19th-february-2026-afternoon-sitting-2440/";

        let sitting =
            parse_hansard_sitting(&html, url).expect("Failed to parse new-format sitting");

        assert_eq!(sitting.house, House::NationalAssembly);
        assert_eq!(sitting.date.to_string(), "2026-02-19");
        assert!(
            !sitting.sections.is_empty(),
            "New-format sitting should have sections"
        );

        let all_contributions: Vec<_> = sitting
            .sections
            .iter()
            .flat_map(|s| {
                s.contributions.iter().chain(
                    s.subsections
                        .iter()
                        .flat_map(|sub| sub.contributions.iter()),
                )
            })
            .collect();
        assert!(
            !all_contributions.is_empty(),
            "New-format sitting should have contributions"
        );

        let notices = sitting
            .sections
            .iter()
            .find(|s| s.section_type == "NOTICES OF MOTIONS");
        assert!(
            notices.is_some(),
            "New-format sitting should have NOTICES OF MOTIONS section"
        );
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
            .flat_map(|s| {
                s.contributions.iter().chain(
                    s.subsections
                        .iter()
                        .flat_map(|sub| sub.contributions.iter()),
                )
            })
            .any(|c| c.speaker_url.is_some());
        assert!(
            with_url,
            "Should have at least one contribution with a speaker URL"
        );
    }

    #[test]
    fn test_parse_sitting_subsections_notices_of_motions() {
        let html = fs::read_to_string("fixtures/current/national_assembly_hansard_sitting")
            .expect("Failed to read fixture");
        let url = "https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2438/";

        let sitting = parse_hansard_sitting(&html, url).expect("Failed to parse sitting");

        let notices = sitting
            .sections
            .iter()
            .find(|s| s.section_type == "NOTICES OF MOTIONS")
            .expect("Should have a NOTICES OF MOTIONS section");

        assert!(
            !notices.subsections.is_empty(),
            "NOTICES OF MOTIONS should have subsections"
        );

        let titles: Vec<&str> = notices
            .subsections
            .iter()
            .map(|s| s.title.as_str())
            .collect();
        assert!(
            titles.iter().any(|t| t.contains("POLLUTION OF ATHI RIVER")),
            "Should include Athi River motion subsection, got: {:?}",
            titles
        );
        assert!(
            titles
                .iter()
                .any(|t| t.contains("HARDSHIP AREAS") || t.contains("MWALA")),
            "Should include Mwala/Kalama hardship areas subsection, got: {:?}",
            titles
        );
    }

    #[test]
    fn test_parse_sitting_subsections_questions_and_statements() {
        let html = fs::read_to_string("fixtures/current/national_assembly_hansard_sitting")
            .expect("Failed to read fixture");
        let url = "https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2438/";

        let sitting = parse_hansard_sitting(&html, url).expect("Failed to parse sitting");

        let qs = sitting
            .sections
            .iter()
            .find(|s| s.section_type == "QUESTIONS AND STATEMENTS")
            .expect("Should have a QUESTIONS AND STATEMENTS section");

        assert!(
            !qs.subsections.is_empty(),
            "QUESTIONS AND STATEMENTS should have subsections"
        );

        let titles: Vec<&str> = qs.subsections.iter().map(|s| s.title.as_str()).collect();
        assert!(
            titles.iter().any(|t| t.contains("REQUESTS FOR STATEMENTS")),
            "Should include REQUESTS FOR STATEMENTS subsection, got: {:?}",
            titles
        );
        assert!(
            titles.iter().any(|t| t.contains("MURDER OF CHIEF")),
            "Should include MURDER OF CHIEF AND TEACHER subsection, got: {:?}",
            titles
        );
    }

    #[test]
    fn test_parse_sitting_subsections_bills() {
        let html = fs::read_to_string("fixtures/current/national_assembly_hansard_sitting")
            .expect("Failed to read fixture");
        let url = "https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2438/";

        let sitting = parse_hansard_sitting(&html, url).expect("Failed to parse sitting");

        let bills_section = sitting
            .sections
            .iter()
            .find(|s| s.section_type == "BILLS" || s.section_type == "BILL")
            .expect("Should have a BILLS section");

        assert!(
            !bills_section.subsections.is_empty(),
            "BILLS section should have subsections"
        );

        let titles: Vec<&str> = bills_section
            .subsections
            .iter()
            .map(|s| s.title.as_str())
            .collect();
        assert!(
            titles.iter().any(|t| t.contains("HEALTH")),
            "Should include Health Amendment Bill subsection, got: {:?}",
            titles
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
    fn test_parse_senate_member_list() {
        let html = fs::read_to_string("fixtures/current/senate_13th_parliament_paginated")
            .expect("Failed to read fixture");

        let members =
            parse_member_list(&html, House::Senate).expect("Failed to parse senate members");

        assert!(!members.is_empty(), "Should parse at least one senator");
        assert!(
            members.iter().all(|m| m.house == House::Senate),
            "All members should be Senate"
        );

        let speaker = members
            .iter()
            .find(|m| m.role.as_deref().unwrap_or("").contains("Speaker"))
            .expect("Should find the Speaker");
        assert!(speaker.role.is_some(), "Speaker should have a role");

        println!("Parsed {} senators", members.len());
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
        assert!(!profile.positions.is_empty(), "Should have positions");
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

        let (current, total) = parse_activity_page_info(&html)
            .unwrap()
            .expect("Should parse activity pagination");
        assert_eq!(current, 1);
        assert_eq!(total, 11);
    }

    #[test]
    fn test_parse_parliamentary_activity() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");

        let items = parse_parliamentary_activity(&html).unwrap();

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

        let bills = parse_bills(&html).unwrap();

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

        let (current, total) = parse_bills_page_info(&html)
            .unwrap()
            .expect("Should parse bills pagination");
        assert_eq!(current, 1);
        assert_eq!(total, 2);
    }

    #[test]
    fn test_parse_voting_patterns() {
        let html = fs::read_to_string(
            "fixtures/current/Boss_Gladys_Jepkosgei_with_paginated_contributions",
        )
        .expect("Failed to read fixture");

        let votes = parse_voting_patterns(&html).unwrap();

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
