use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;
use scraper::{ElementRef, Html, Selector, error::SelectorErrorKind};

use super::types::{
    Bill, HansardListing, HansardListingKind, House, Member, MemberProfile, ParliamentaryActivity,
    VoteRecord,
};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Failed to parse URL: {0}")]
    UrlParse(String),
    #[error("Failed to parse date: {0}")]
    DateParse(String),
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
    Regex::new(r"(?i)(\w+),\s+(\d+)\w*\s+(\w+),?\s+(\d{4})(?:\s*[-–]\s*(.+))?")
        .expect("invalid regex: listing title")
});

static RE_SPEECHES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"has made\D+(\d+)\D+speeches last year\D+(\d+)\D+speeches")
        .expect("invalid regex: speeches")
});

static RE_BILLS_TOTAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"has sponsored\D+(\d+)\D+bill").expect("invalid regex: bills total")
});

static RE_ACTIVITY_TOTALS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\d+)\s+counted contributions.*?,\s*(\d+)\s+of them")
        .expect("invalid regex: activity totals")
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
    let session_type = caps
        .get(5)
        .map(|session| normalize_whitespace(session.as_str()))
        .unwrap_or_default();

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
    let Some(current_page) = document
        .select(&active_sel)
        .next()
        .and_then(|e| normalize_whitespace(&elem_text(e)).parse::<u32>().ok())
    else {
        return Ok(None);
    };

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
    let Some(current_page) = document
        .select(&active_sel)
        .next()
        .and_then(|e| normalize_whitespace(&elem_text(e)).parse::<u32>().ok())
    else {
        return Ok(None);
    };

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
    let row_selector = Selector::parse("div.leg-row")?;
    let row_link_selector = Selector::parse("a.leg-row__link")?;
    let row_title_selector = Selector::parse("span.leg-row__title")?;
    let row_house_selector = Selector::parse("span.leg-badge")?;
    let split_selector = Selector::parse("div.split-docs")?;
    let link_selector = Selector::parse("div.hansard-document h3 a")?;

    let mut listings = Vec::new();
    let mut recognized_rows = 0;

    for row in document.select(&row_selector) {
        recognized_rows += 1;
        let link_elem = row
            .select(&row_link_selector)
            .next()
            .ok_or_else(|| ParseError::MissingField("Hansard listing link element".to_owned()))?;
        let url = link_elem
            .value()
            .attr("href")
            .map(str::to_string)
            .ok_or_else(|| ParseError::MissingField("Hansard listing URL".to_owned()))?;
        let title = link_elem
            .select(&row_title_selector)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .unwrap_or_else(|| normalize_whitespace(&elem_text(link_elem)));
        let house_text = row
            .select(&row_house_selector)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .unwrap_or_default();
        let house = if house_text.contains("National Assembly") {
            House::NationalAssembly
        } else if house_text.contains("Senate") {
            House::Senate
        } else {
            return Err(ParseError::MissingField(format!(
                "Unknown Hansard listing house '{house_text}' for '{title}'"
            )));
        };

        if house_filter.as_ref().is_some_and(|f| f != &house) {
            continue;
        }

        let (date, _, session_type) = parse_date_from_title(&title)?;
        listings.push(HansardListing {
            house,
            date,
            session_type,
            kind: HansardListingKind::Transcript,
            url,
            title,
        });
    }

    if recognized_rows > 0 {
        return Ok(listings);
    }

    for (i, split_div) in document.select(&split_selector).enumerate() {
        recognized_rows += 1;
        let house = if i == 0 {
            House::NationalAssembly
        } else {
            House::Senate
        };

        if house_filter.as_ref().is_some_and(|f| f != &house) {
            continue;
        }

        for link_elem in split_div.select(&link_selector) {
            let url = link_elem
                .value()
                .attr("href")
                .map(str::to_string)
                .ok_or_else(|| ParseError::MissingField("Hansard listing URL".to_owned()))?;

            let title = normalize_whitespace(&elem_text(link_elem));
            if title.is_empty() {
                return Err(ParseError::MissingField("Hansard listing title".to_owned()));
            }

            let (date, _, session_type) = parse_date_from_title(&title)?;
            listings.push(HansardListing {
                house,
                date,
                session_type,
                kind: HansardListingKind::Transcript,
                url,
                title,
            });
        }
    }

    if recognized_rows == 0 {
        Err(ParseError::MissingField(
            "No recognized Hansard listings".to_owned(),
        ))
    } else {
        Ok(listings)
    }
}

pub fn parse_hansard_pdf_url(html: &str, page_url: &str) -> Result<String, ParseError> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(
        "div.document-thumbnail a, a[href*='/source/'], a[href*='.pdf'], a[href*='.PDF']",
    )?;

    document
        .select(&selector)
        .filter_map(|element| element.value().attr("href"))
        .find(|href| {
            let path = href.split(['?', '#']).next().unwrap_or(href);
            path.to_ascii_lowercase().ends_with(".pdf") || path.ends_with("/source/")
        })
        .map(|href| resolve_url(page_url, href))
        .ok_or_else(|| ParseError::MissingField("Hansard PDF URL".to_owned()))
}

#[derive(Clone)]
pub struct HansardPageIdentity {
    pub house: House,
    pub date: NaiveDate,
    pub session_type: String,
    pub summary: Option<String>,
    pub sentiment: Option<String>,
}

pub fn parse_hansard_page_identity(html: &str) -> Result<HansardPageIdentity, ParseError> {
    let document = Html::parse_document(html);
    let house_selector = Selector::parse("span.house, h1.house-title")?;
    let house_text = document
        .select(&house_selector)
        .map(elem_text)
        .collect::<Vec<_>>()
        .join(" ");
    let house =
        if house_text.contains("National Assembly") || house_text.contains("NATIONAL ASSEMBLY") {
            House::NationalAssembly
        } else if house_text.contains("Senate") || house_text.contains("SENATE") {
            House::Senate
        } else {
            return Err(ParseError::MissingField("Hansard house".to_owned()));
        };

    let title_selector =
        Selector::parse("li.breadcrumb-item.current, meta[property='og:title'], title")?;
    let (date, _, session_type) = document
        .select(&title_selector)
        .filter_map(|element| {
            let value = element
                .value()
                .attr("content")
                .map(str::to_owned)
                .unwrap_or_else(|| elem_text(element));
            parse_date_from_title(&value).ok()
        })
        .next()
        .ok_or_else(|| ParseError::MissingField("Hansard date".to_owned()))?;
    let summary_selector = Selector::parse("div.doc-summary")?;
    let (summary, sentiment) = document
        .select(&summary_selector)
        .next()
        .map(parse_doc_summary)
        .unwrap_or((None, None));

    Ok(HansardPageIdentity {
        house,
        date,
        session_type,
        summary,
        sentiment,
    })
}

fn parse_doc_summary(element: ElementRef<'_>) -> (Option<String>, Option<String>) {
    let full = normalize_whitespace(&elem_text(element));
    let body = full
        .strip_prefix("Hansard Summary")
        .map(str::trim)
        .unwrap_or(&full);
    let (summary, sentiment) = body
        .split_once("Sentimental Analysis")
        .map(|(summary, sentiment)| (summary.trim(), sentiment.trim()))
        .unwrap_or((body, ""));
    (
        (!summary.is_empty()).then(|| summary.to_owned()),
        (!sentiment.is_empty()).then(|| sentiment.to_owned()),
    )
}

fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http") {
        return href.to_string();
    }

    reqwest::Url::parse(base)
        .and_then(|url| url.join(href))
        .map(|url| url.to_string())
        .unwrap_or_else(|_| href.to_string())
}

pub fn parse_member_list(html: &str, house: House) -> Result<Vec<Member>, ParseError> {
    let document = Html::parse_document(html);
    let item_sel = Selector::parse("a.members-list--item, a.senators-list--item")?;
    let name_sel = Selector::parse("div.members-list--name, div.senators-list--name")?;
    let leader_role_sel = Selector::parse("p.leader-role")?;
    let strong_sel = Selector::parse("strong")?;
    let repr_sel =
        Selector::parse("div.members-list--representation, div.senators-list--representation")?;

    let mut members = Vec::new();

    for item in document.select(&item_sel) {
        let url = item
            .value()
            .attr("href")
            .map(str::to_string)
            .ok_or_else(|| ParseError::MissingField("Member profile URL".to_owned()))?;

        let name = item
            .select(&name_sel)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .unwrap_or_default();

        if name.is_empty() {
            return Err(ParseError::MissingField("Member name".to_owned()));
        }

        let role = item
            .select(&leader_role_sel)
            .next()
            .map(|e| normalize_whitespace(&elem_text(e)))
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let representation = item.select(&repr_sel).next()?;
                let text = normalize_whitespace(&elem_text(representation));
                if !(text.contains("Speaker") || text.contains("Leader") || text.contains("Whip")) {
                    return None;
                }
                representation
                    .select(&strong_sel)
                    .next()
                    .map(|e| normalize_whitespace(&elem_text(e)))
                    .filter(|s| !s.is_empty())
            });

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

    if members.is_empty() {
        Err(ParseError::MissingField("No recognized members".to_owned()))
    } else {
        Ok(members)
    }
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

    let activity_sel = Selector::parse("div.activity-section p, p.activity-totals")?;
    let (speeches_last_year, speeches_total) = document
        .select(&activity_sel)
        .next()
        .and_then(|e| {
            let text = elem_text(e);
            if let Some(caps) = RE_SPEECHES.captures(&text) {
                let last_year: u32 = caps[1].parse().ok()?;
                let total: u32 = caps[2].parse().ok()?;
                Some((Some(last_year), Some(total)))
            } else {
                let caps = RE_ACTIVITY_TOTALS.captures(&text)?;
                let total: u32 = caps[1].parse().ok()?;
                let current_year: u32 = caps[2].parse().ok()?;
                Some((Some(current_year), Some(total)))
            }
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
        println!("First listing: {:#?}", first);
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
    fn test_parse_redesigned_hansard_list() {
        let html = r#"
            <div class="leg-row leg-row--inline">
              <a class="leg-row__link" href="/democracy-tools/hansard/document/3096/">
                <span class="leg-row__title">Wednesday, 1st July, 2026 - Morning Sitting</span>
                <span class="leg-row__meta"><span class="leg-badge">National Assembly</span></span>
              </a>
            </div>
            <div class="leg-row leg-row--inline">
              <a class="leg-row__link" href="/democracy-tools/hansard/document/3097/">
                <span class="leg-row__title">Wednesday, 1st July, 2026 - Afternoon Sitting</span>
                <span class="leg-row__meta"><span class="leg-badge">Senate</span></span>
              </a>
            </div>
        "#;

        let listings = parse_hansard_list(html, None).unwrap();

        assert_eq!(listings.len(), 2);
        assert_eq!(listings[0].house, House::NationalAssembly);
        assert_eq!(listings[0].date.to_string(), "2026-07-01");
        assert_eq!(listings[0].session_type, "Morning Sitting");
        assert_eq!(listings[0].url, "/democracy-tools/hansard/document/3096/");
        assert_eq!(listings[1].house, House::Senate);
    }

    #[test]
    fn treats_direct_parliament_pdf_listing_as_actionable() {
        let html = r#"
            <div class="leg-row leg-row--inline">
              <a class="leg-row__link" href="https://www.parliament.go.ke/sites/default/files/2026-07/Hansard.pdf?download=1">
                <span class="leg-row__title">Wednesday, 1st July, 2026 - Morning Sitting</span>
                <span class="leg-row__meta"><span class="leg-badge">National Assembly</span></span>
              </a>
            </div>
        "#;

        let listings = parse_hansard_list(html, None).unwrap();

        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].kind, HansardListingKind::Transcript);
    }

    #[test]
    fn extracts_pdf_url_without_parsing_transcript_content() {
        let html = r#"
            <meta property="og:title" content="Thursday, 2nd July, 2026">
            <span class="house"><strong>House:</strong> National Assembly</span>
            <div class="doc-summary">
              Hansard Summary Debate summary. Sentimental Analysis Constructive.
            </div>
            <article class="hansard-content"><div>Unstable transcript markup</div></article>
            <a href="/democracy-tools/hansard/document/3137/source/">Official PDF</a>
        "#;

        assert_eq!(
            parse_hansard_pdf_url(
                html,
                "https://mzalendo.com/democracy-tools/hansard/document/3137/"
            )
            .unwrap(),
            "https://mzalendo.com/democracy-tools/hansard/document/3137/source/"
        );

        let identity = parse_hansard_page_identity(html).unwrap();
        assert_eq!(identity.house, House::NationalAssembly);
        assert_eq!(identity.date.to_string(), "2026-07-02");
        assert_eq!(identity.session_type, "");
        assert_eq!(identity.summary.as_deref(), Some("Debate summary."));
        assert_eq!(identity.sentiment.as_deref(), Some("Constructive."));
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
    fn test_parse_redesigned_member_leadership_role() {
        let html = r#"
            <div class="members-list leadership-grid">
              <a href="/mps-performance/national-assembly/13th-parliament/anthony-kimani-ichungwah/" class="members-list--item">
                <div class="members-list--name">ICHUNG'WAH ANTONY KIMANI</div>
                <div class="members-list--representation"><strong>Majority Leader</strong> · MNA for Kikuyu</div>
              </a>
            </div>
        "#;

        let members = parse_member_list(html, House::NationalAssembly).unwrap();

        assert_eq!(members.len(), 1);
        assert_eq!(members[0].role.as_deref(), Some("Majority Leader"));
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

        println!("{:#?}", profile);
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
        println!("First: {:#?}", items[0]);
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
    fn test_parse_redesigned_member_activity_totals() {
        let html = r#"
            <h1 class="page-heading">ICHUNG'WAH ANTONY KIMANI</h1>
            <p class="activity-totals">
              <span class="activity-totals__count">1758</span>
              counted contributions in this Parliament, 365 of them in 2026.
            </p>
        "#;
        let url = "https://mzalendo.com/mps-performance/national-assembly/13th-parliament/anthony-kimani-ichungwah/";

        let profile = parse_member_profile(html, url).unwrap();

        assert_eq!(profile.speeches_last_year, Some(365));
        assert_eq!(profile.speeches_total, Some(1758));
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
        println!("First bill: {:#?}", first);
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
        println!("First vote: {:#?}", votes[0]);
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
            ("Thursday, 20th August, 2026", (2026, 8, 20), "Thursday", ""),
        ];

        for (title, (year, month, day), weekday, session) in cases {
            let (date, dow, sess) = parse_date_from_title(title)
                .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", title, e));
            assert_eq!(date, NaiveDate::from_ymd_opt(year, month, day).unwrap());
            assert_eq!(dow.to_lowercase(), weekday.to_lowercase());
            assert_eq!(sess, session);
        }
    }
}
