use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use odnelazm::HansardSitting;

use crate::store::BillRecord;

/// A member who spoke during a bill's debate segment.
#[derive(Debug, Clone)]
pub struct BillContributor {
    pub name: String,
    pub url: Option<String>,
    pub speech_count: u32,
}

/// An extracted bill mention ready to be handed to the pipeline.
#[derive(Debug, Clone)]
pub struct ExtractedBillMention {
    pub bill: BillRecord,
    /// Legislative stage detected from title or contribution text.
    pub stage: Option<String>,
    /// The section / subsection title this mention was found under.
    pub section_title: String,
    /// Number of contributions in this debate segment.
    pub speech_count: u32,
    /// Members who spoke during this bill's debate segment.
    pub contributors: Vec<BillContributor>,
}

// Matches formal bill numbers in contribution text.
// e.g. "National Assembly Bill No.20 of 2026" or "Senate Bill No. 5 of 2025"
static RE_BILL_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(National Assembly|Senate)\s+Bill\s+No\.?\s*(\d+)\s+of\s+(\d{4})")
        .expect("invalid bill number regex")
});

// Matches reading stage in contribution text.
// e.g. "be read a Second Time"
static RE_READING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)be read a (First|Second|Third) Time").expect("invalid reading regex")
});

fn stage_from_title(title: &str) -> Option<String> {
    let u = title.to_uppercase();
    if u.contains("FIRST READING") {
        Some("First Reading".into())
    } else if u.contains("SECOND READING") {
        Some("Second Reading".into())
    } else if u.contains("THIRD READING") {
        Some("Third Reading".into())
    } else if u.contains("COMMITTEE OF THE WHOLE HOUSE") || u.contains("IN THE COMMITTEE") {
        Some("Committee Stage".into())
    } else if u.contains("CONSIDERATION OF REPORT") {
        Some("Report Stage".into())
    } else if u.contains("APPROVAL OF MEDIATED VERSION") {
        Some("Mediation Approval".into())
    } else {
        None
    }
}

fn stage_from_text(text: &str) -> Option<String> {
    if let Some(caps) = RE_READING.captures(text) {
        return Some(format!("{} Reading", title_case(&caps[1])));
    }
    if text.to_uppercase().contains("COMMITTEE OF THE WHOLE HOUSE") {
        return Some("Committee Stage".into());
    }
    None
}

/// Stage prefixes that appear before the actual bill name in subsection titles.
static STAGE_PREFIXES: &[&str] = &[
    "APPROVAL OF MEDIATED VERSION OF THE ",
    "CONSIDERATION OF REPORT ON THE ",
    "ADOPTION OF REPORT ON THE ",
    "SECOND READING OF THE ",
    "THIRD READING OF THE ",
    "FIRST READING OF THE ",
];

/// Return the canonical bill name if the title describes a bill, or None.
fn bill_name_from_title(title: &str) -> Option<String> {
    let upper = title.trim().to_uppercase();

    // Must end with BILL or ACT (common Kenyan parliamentary naming)
    if !upper.ends_with("BILL") && !upper.ends_with(" ACT") {
        return None;
    }

    // Strip any known stage prefix
    let stripped = STAGE_PREFIXES
        .iter()
        .find_map(|prefix| upper.strip_prefix(prefix))
        .unwrap_or(&upper);

    // Strip leading "THE "
    let stripped = stripped.strip_prefix("THE ").unwrap_or(stripped);

    Some(title_case(stripped))
}

fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            // Strip leading punctuation like '(' to capitalise the letter, then reattach.
            let (prefix, rest) =
                word.split_at(word.find(|c: char| c.is_alphabetic()).unwrap_or(word.len()));
            let mut chars = rest.chars();
            match chars.next() {
                None => prefix.to_string(),
                Some(first) => {
                    format!(
                        "{}{}{}",
                        prefix,
                        first.to_uppercase(),
                        chars.as_str().to_lowercase()
                    )
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_bill_number(text: &str) -> Option<String> {
    RE_BILL_NUMBER
        .captures(text)
        .map(|caps| caps[0].split_whitespace().collect::<Vec<_>>().join(" "))
}

fn tally_contributors<'a>(
    contribs: impl Iterator<Item = &'a odnelazm::Contribution>,
) -> Vec<BillContributor> {
    let mut counts: HashMap<(String, Option<String>), u32> = HashMap::new();
    for c in contribs {
        *counts
            .entry((c.speaker_name.clone(), c.speaker_url.clone()))
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(|((name, url), speech_count)| BillContributor {
            name,
            url,
            speech_count,
        })
        .collect()
}

/// Walk a sitting's sections and subsections and return every bill mention found.
pub fn extract_bills(sitting: &HansardSitting) -> Vec<ExtractedBillMention> {
    let mut mentions: Vec<ExtractedBillMention> = Vec::new();

    for section in &sitting.sections {
        // Check subsection titles
        for subsection in &section.subsections {
            let title = &subsection.title;

            let Some(bill_name) = bill_name_from_title(title) else {
                continue;
            };

            let contrib_text: String = subsection
                .contributions
                .iter()
                .map(|c| c.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            let bill_number = extract_bill_number(&contrib_text);
            let year = bill_number.as_ref().and_then(|n| {
                RE_BILL_NUMBER
                    .captures(n)
                    .and_then(|c| c[3].parse::<i32>().ok())
            });

            let sponsor = subsection.contributions.iter().find_map(|c| {
                if c.content.contains("I beg to move") {
                    Some(c.speaker_name.clone())
                } else {
                    None
                }
            });

            let stage = stage_from_title(title).or_else(|| stage_from_text(&contrib_text));
            let contributors = tally_contributors(subsection.contributions.iter());

            mentions.push(ExtractedBillMention {
                bill: BillRecord {
                    name: bill_name,
                    bill_number,
                    year,
                    sponsor,
                },
                stage,
                section_title: title.clone(),
                speech_count: subsection.contributions.len() as u32,
                contributors,
            });
        }

        // Check the section_type itself (archive-style flat sections)
        if section.subsections.is_empty()
            && let Some(bill_name) = bill_name_from_title(&section.section_type)
        {
            let contrib_text: String = section
                .contributions
                .iter()
                .map(|c| c.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            let bill_number = extract_bill_number(&contrib_text);
            let year = bill_number.as_ref().and_then(|n| {
                RE_BILL_NUMBER
                    .captures(n)
                    .and_then(|c| c[3].parse::<i32>().ok())
            });

            let sponsor = section.contributions.iter().find_map(|c| {
                if c.content.contains("I beg to move") {
                    Some(c.speaker_name.clone())
                } else {
                    None
                }
            });

            let stage =
                stage_from_title(&section.section_type).or_else(|| stage_from_text(&contrib_text));
            let contributors = tally_contributors(section.contributions.iter());

            mentions.push(ExtractedBillMention {
                bill: BillRecord {
                    name: bill_name,
                    bill_number,
                    year,
                    sponsor,
                },
                stage,
                section_title: section.section_type.clone(),
                speech_count: section.contributions.len() as u32,
                contributors,
            });
        }
    }

    mentions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::speakers::extract_speakers;

    fn load_sitting(path: &str) -> odnelazm::HansardSitting {
        let json = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("cannot read {path}"));
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("deserialize {path}: {e}"))
    }

    #[test]
    fn extraction_against_current_sitting() {
        let path = "/tmp/current_sitting.json";
        if !std::path::Path::new(path).exists() {
            eprintln!("skip: {path} not present");
            return;
        }
        let sitting = load_sitting(path);

        println!("\n=== SITTING: {} {} ===\n", sitting.house, sitting.date);

        let bills = extract_bills(&sitting);
        println!("── Bills found: {} ──", bills.len());
        for b in &bills {
            println!("  name:         {}", b.bill.name);
            if let Some(n) = &b.bill.bill_number {
                println!("  bill_number:  {n}");
            }
            if let Some(y) = b.bill.year {
                println!("  year:         {y}");
            }
            if let Some(s) = &b.bill.sponsor {
                println!("  sponsor:      {s}");
            }
            println!(
                "  stage:        {}",
                b.stage.as_deref().unwrap_or("(unknown)")
            );
            println!("  section:      {}", b.section_title);
            println!("  speeches:     {}", b.speech_count);
            println!();
        }

        let speakers = extract_speakers(&sitting);
        println!("── Speakers found: {} ──", speakers.len());
        let mut sorted = speakers.clone();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (sp, count) in sorted.iter().take(10) {
            println!(
                "  {:>3} speech(es)  {}{}",
                count,
                sp.name,
                sp.url
                    .as_deref()
                    .map(|u| format!("  [{u}]"))
                    .unwrap_or_default()
            );
        }
        if speakers.len() > 10 {
            println!("  ... and {} more", speakers.len() - 10);
        }
    }

    #[test]
    fn bill_name_from_subsection_title() {
        assert_eq!(
            bill_name_from_title("THE INCOME TAX (AMENDMENT) BILL"),
            Some("Income Tax (Amendment) Bill".into())
        );
        assert_eq!(
            bill_name_from_title(
                "APPROVAL OF MEDIATED VERSION OF THE NATIONAL DISASTER RISK MANAGEMENT BILL"
            ),
            Some("National Disaster Risk Management Bill".into())
        );
        assert_eq!(
            bill_name_from_title(
                "CONSIDERATION OF REPORT ON THE FOREST CONSERVATION AND MANAGEMENT (AMENDMENT) BILL"
            ),
            Some("Forest Conservation And Management (Amendment) Bill".into())
        );
        assert_eq!(
            bill_name_from_title("KILLING OF CIVILIANS IN MWINGI NORTH"),
            None
        );
    }

    #[test]
    fn stage_from_contribution_text() {
        assert_eq!(
            stage_from_text("I beg to move that the Bill be read a Second Time."),
            Some("Second Reading".into())
        );
        assert_eq!(stage_from_text("No stage here."), None);
    }

    #[test]
    fn bill_number_regex() {
        let text = "the Income Tax (Amendment) Bill (National Assembly Bill No.20 of 2026) be read";
        assert_eq!(
            extract_bill_number(text),
            Some("National Assembly Bill No.20 of 2026".into())
        );
    }
}
