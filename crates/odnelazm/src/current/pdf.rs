use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveTime, Timelike};
use pdf_inspector::{MarkdownOptions, PdfOptions, process_pdf_mem_with_options};
use regex::Regex;

use super::types::{Contribution, HansardSection, HansardSitting, HansardSubsection, House};

static DATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday),?\s+(\d{1,2})\s*(?:st|nd|rd|th)?\s+(January|February|March|April|May|June|July|August|September|October|November|December),?\s+(\d{4})",
    )
    .expect("valid PDF date regex")
});
static TIME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)The (?:House|Senate) met (?:at|in)(?:[^\n\d]{0,120}\bat)?\s+(\d{1,2})[.:](\d{2})\s*([ap])\.?m\.?",
    )
    .expect("valid PDF time regex")
});
static SPEAKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<speaker>(?:Hon\. |Sen\.? |The (?:Deputy |Temporary )?Speaker|An Hon\. Member|The Chairperson)[^:]{0,150}):\s*(?P<content>.*)$",
    )
    .expect("valid PDF speaker regex")
});
static MARKED_SPEAKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\*\*(?P<speaker>(?:Hon\. |Sen\.? |The (?:Deputy |Temporary )?Speaker|An Hon\. Member|The Chairperson)[^*:\n]{0,150})\*\*(?P<suffix>\s*\([^:\n]{0,100}\))?:",
    )
    .expect("valid marked speaker regex")
});
static MARKED_SPEAKER_INNER_COLON_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\*\*(?P<speaker>(?:Hon\. |Sen\.? |The (?:Deputy |Temporary )?Speaker|An Hon\. Member|The Chairperson)[^*:\n]{0,150}):\*\*",
    )
    .expect("valid marked speaker regex")
});
static MALFORMED_MARKED_SPEAKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\*\*(?P<speaker>The (?:Deputy |Temporary )?Speaker \(\*\*Hon\.[^:\n]{0,100}\)):")
        .expect("valid malformed marked speaker regex")
});
static MARKED_HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\*\*(?P<heading>[A-Z][A-Z0-9 &'(),./-]{2,150})\*\*")
        .expect("valid marked heading regex")
});

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("PDF inspection failed: {0}")]
    Inspector(#[source] pdf_inspector::PdfError),
    #[error("PDF inspection task failed: {0}")]
    InspectorTask(#[source] tokio::task::JoinError),
    #[error("PDF pages require OCR: {0:?}")]
    RequiresOcr(Vec<u32>),
    #[error("PDF contains no extractable text")]
    NoExtractableText,
    #[error("Missing required PDF field: {0}")]
    MissingField(&'static str),
    #[error("Invalid PDF metadata: {0}")]
    InvalidMetadata(String),
    #[error("PDF transcript failed validation: {0}")]
    Validation(String),
}

pub async fn extract_text(pdf: Vec<u8>) -> Result<String, PdfError> {
    let result = tokio::task::spawn_blocking(move || {
        process_pdf_mem_with_options(
            &pdf,
            PdfOptions::new().markdown(MarkdownOptions {
                strip_headers_footers: true,
                ..MarkdownOptions::default()
            }),
        )
    })
    .await
    .map_err(PdfError::InspectorTask)?
    .map_err(PdfError::Inspector)?;
    if !result.pages_needing_ocr.is_empty() {
        return Err(PdfError::RequiresOcr(result.pages_needing_ocr));
    }
    let text = prepare_markdown(&result.markdown.unwrap_or_default());
    if text.trim().len() < 100 {
        return Err(PdfError::NoExtractableText);
    }
    Ok(text)
}

pub fn parse_sitting(text: &str, pdf_url: &str) -> Result<HansardSitting, PdfError> {
    let house = parse_house(text)?;
    let (date, day_of_week) = parse_date(text)?;
    let time = parse_time(text)?;
    let session_type = match time.map(|value| value.hour()) {
        Some(hour) if hour < 12 => "Morning Sitting",
        Some(_) => "Afternoon Sitting",
        None => "Sitting",
    }
    .to_owned();
    let sections = parse_sections(&extract_blocks(text));

    validate_sections(&sections)?;

    Ok(HansardSitting {
        house,
        date,
        day_of_week,
        session_type,
        time,
        summary: None,
        sentiment: None,
        pdf_url: Some(pdf_url.to_owned()),
        sections,
    })
}

fn parse_house(text: &str) -> Result<House, PdfError> {
    for line in text.lines() {
        match clean_markdown_line(line).as_str() {
            "NATIONAL ASSEMBLY" | "THE NATIONAL ASSEMBLY" => {
                return Ok(House::NationalAssembly);
            }
            "SENATE" | "THE SENATE" => return Ok(House::Senate),
            _ => {}
        }
    }
    Err(PdfError::MissingField("house"))
}

fn parse_date(text: &str) -> Result<(NaiveDate, String), PdfError> {
    let captures = DATE_RE
        .captures(text)
        .ok_or(PdfError::MissingField("date"))?;
    let day = captures[2]
        .parse::<u32>()
        .map_err(|error| PdfError::InvalidMetadata(error.to_string()))?;
    let month = match captures[3].to_ascii_lowercase().as_str() {
        "january" => 1,
        "february" => 2,
        "march" => 3,
        "april" => 4,
        "may" => 5,
        "june" => 6,
        "july" => 7,
        "august" => 8,
        "september" => 9,
        "october" => 10,
        "november" => 11,
        "december" => 12,
        _ => return Err(PdfError::InvalidMetadata(captures[3].to_owned())),
    };
    let year = captures[4]
        .parse::<i32>()
        .map_err(|error| PdfError::InvalidMetadata(error.to_string()))?;
    let date = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| PdfError::InvalidMetadata(captures[0].to_owned()))?;
    let parsed_day = date.format("%A").to_string();
    if !parsed_day.eq_ignore_ascii_case(&captures[1]) {
        return Err(PdfError::InvalidMetadata(format!(
            "{} was labelled {}",
            date, &captures[1]
        )));
    }
    Ok((date, captures[1].to_owned()))
}

fn parse_time(text: &str) -> Result<Option<NaiveTime>, PdfError> {
    let Some(captures) = TIME_RE.captures(text) else {
        return Ok(None);
    };
    let mut hour = captures[1]
        .parse::<u32>()
        .map_err(|error| PdfError::InvalidMetadata(error.to_string()))?;
    let minute = captures[2]
        .parse::<u32>()
        .map_err(|error| PdfError::InvalidMetadata(error.to_string()))?;
    let afternoon = captures[3].eq_ignore_ascii_case("p");
    if afternoon && hour != 12 {
        hour += 12;
    } else if !afternoon && hour == 12 {
        hour = 0;
    }
    NaiveTime::from_hms_opt(hour, minute, 0)
        .map(Some)
        .ok_or_else(|| PdfError::InvalidMetadata(captures[0].to_owned()))
}

fn extract_blocks(text: &str) -> Vec<String> {
    let pages: Vec<&str> = text.split('\u{c}').collect();
    let first_transcript_page = pages
        .iter()
        .position(|page| page.lines().any(is_sitting_start))
        .unwrap_or(0);
    let mut blocks = Vec::new();
    let mut transcript_started = false;

    for page in pages.into_iter().skip(first_transcript_page) {
        let mut current = Vec::new();
        let mut current_is_heading = false;
        let mut current_is_note = false;

        for raw_line in page.lines() {
            let cleaned = clean_markdown_line(raw_line);
            let line = cleaned.trim();
            if !transcript_started {
                if is_sitting_start(line) {
                    transcript_started = true;
                    continue;
                } else {
                    continue;
                }
            }
            if is_page_header(line) {
                continue;
            }
            if line == "th" {
                continue;
            }
            if line.starts_with("Disclaimer: The electronic version") {
                flush_block(&mut current, &mut blocks);
                current_is_heading = false;
                current_is_note = false;
                continue;
            }
            if line.is_empty() {
                flush_block(&mut current, &mut blocks);
                current_is_heading = false;
                current_is_note = false;
                continue;
            }

            let line_is_heading = is_heading(line);
            let line_starts_speaker = split_speaker(line).is_some();
            let line_starts_note = line.starts_with('(') || line.starts_with('[');
            if line_starts_speaker
                || (line_is_heading && !current_is_heading)
                || (line_starts_note && !current_is_note)
                || (current_is_heading && !line_is_heading)
            {
                flush_block(&mut current, &mut blocks);
                current_is_heading = false;
                current_is_note = false;
            }

            if current.is_empty() {
                current_is_heading = line_is_heading;
                current_is_note = line_starts_note;
            }
            current.push(line.to_owned());

            if current_is_note && (line.ends_with(')') || line.ends_with(']')) {
                flush_block(&mut current, &mut blocks);
                current_is_note = false;
            }
        }
        flush_block(&mut current, &mut blocks);
    }

    blocks
}

fn is_page_header(line: &str) -> bool {
    line.chars()
        .next()
        .is_some_and(|value| value.is_ascii_digit())
        && line.contains("Debates")
}

fn is_sitting_start(line: &str) -> bool {
    [
        "The House met at",
        "The House met in",
        "The Senate met at",
        "The Senate met in",
    ]
    .iter()
    .any(|marker| line.contains(marker))
}

fn flush_block(current: &mut Vec<String>, blocks: &mut Vec<String>) {
    if !current.is_empty() {
        blocks.push(normalize(&current.join(" ")));
        current.clear();
    }
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn prepare_markdown(markdown: &str) -> String {
    let with_malformed_speakers = MALFORMED_MARKED_SPEAKER_RE
        .replace_all(markdown, "\n${speaker}:\n")
        .into_owned();
    let with_inner_colons = MARKED_SPEAKER_INNER_COLON_RE
        .replace_all(&with_malformed_speakers, "\n${speaker}:\n")
        .into_owned();
    let with_speaker_boundaries = MARKED_SPEAKER_RE
        .replace_all(&with_inner_colons, "\n${speaker}${suffix}:\n")
        .into_owned();
    let with_heading_boundaries = MARKED_HEADING_RE
        .replace_all(&with_speaker_boundaries, "\n${heading}\n")
        .into_owned();
    with_heading_boundaries
        .lines()
        .map(clean_markdown_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn clean_markdown_line(line: &str) -> String {
    line.trim_start_matches('#')
        .trim()
        .replace("**", "")
        .replace('*', "")
}

fn is_heading(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|value| value.is_alphabetic()).collect();
    if letters.len() < 3
        || letters.iter().any(|value| value.is_lowercase())
        || text.len() > 190
        || text.ends_with('.')
    {
        return false;
    }
    text.split_whitespace().count() >= 2 || is_major_heading(text)
}

fn is_major_heading(text: &str) -> bool {
    matches!(
        text,
        "PRAYER"
            | "PRAYERS"
            | "QUORUM"
            | "COMMUNICATION FROM THE CHAIR"
            | "COMMUNICATIONS FROM THE CHAIR"
            | "MESSAGES"
            | "PETITION"
            | "PETITIONS"
            | "PAPERS"
            | "PAPERS LAID"
            | "NOTICES OF MOTION"
            | "NOTICES OF MOTIONS"
            | "QUESTIONS AND STATEMENTS"
            | "PROCEDURAL MOTIONS"
            | "STATEMENT"
            | "STATEMENTS"
            | "MOTION"
            | "MOTIONS"
            | "BILL"
            | "BILLS"
            | "COMMITTEE OF THE WHOLE"
            | "COMMITTEE OF THE WHOLE HOUSE"
            | "ADJOURNMENT"
    )
}

fn split_speaker(text: &str) -> Option<(&str, &str)> {
    let captures = SPEAKER_RE.captures(text)?;
    Some((
        captures.name("speaker")?.as_str().trim(),
        captures.name("content")?.as_str().trim(),
    ))
}

fn is_procedural_note(text: &str) -> bool {
    (text.starts_with('(') && text.ends_with(')')) || (text.starts_with('[') && text.ends_with(']'))
}

fn parse_sections(blocks: &[String]) -> Vec<HansardSection> {
    let mut sections = Vec::new();
    let mut section: Option<HansardSection> = None;
    let mut subsection: Option<HansardSubsection> = None;
    let mut contribution: Option<Contribution> = None;

    for block in blocks {
        if block == "THE HANSARD" || DATE_RE.is_match(block) {
            continue;
        }
        if is_heading(block) {
            flush_contribution(&mut contribution, &mut subsection, &mut section);
            if is_major_heading(block) {
                flush_subsection(&mut subsection, &mut section);
                if let Some(previous) = section.take() {
                    sections.push(previous);
                }
                section = Some(HansardSection {
                    section_type: block.clone(),
                    subsections: Vec::new(),
                    contributions: Vec::new(),
                });
            } else if section.is_some() {
                flush_subsection(&mut subsection, &mut section);
                subsection = Some(HansardSubsection {
                    title: block.clone(),
                    contributions: Vec::new(),
                });
            } else {
                section = Some(HansardSection {
                    section_type: block.clone(),
                    subsections: Vec::new(),
                    contributions: Vec::new(),
                });
            }
            continue;
        }

        if let Some((speaker_name, content)) = split_speaker(block) {
            flush_contribution(&mut contribution, &mut subsection, &mut section);
            contribution = Some(Contribution {
                speaker_name: speaker_name.to_owned(),
                speaker_url: None,
                content: content.to_owned(),
                procedural_notes: Vec::new(),
            });
            continue;
        }

        let active = contribution.get_or_insert_with(|| Contribution {
            speaker_name: String::new(),
            speaker_url: None,
            content: String::new(),
            procedural_notes: Vec::new(),
        });
        if is_procedural_note(block) {
            active.procedural_notes.push(block.clone());
        } else {
            if !active.content.is_empty() {
                active.content.push_str("\n\n");
            }
            active.content.push_str(block);
        }
    }

    flush_contribution(&mut contribution, &mut subsection, &mut section);
    flush_subsection(&mut subsection, &mut section);
    if let Some(section) = section {
        sections.push(section);
    }
    sections
}

fn flush_contribution(
    contribution: &mut Option<Contribution>,
    subsection: &mut Option<HansardSubsection>,
    section: &mut Option<HansardSection>,
) {
    let Some(contribution) = contribution.take() else {
        return;
    };
    let section = section.get_or_insert_with(|| HansardSection {
        section_type: "OPENING".to_owned(),
        subsections: Vec::new(),
        contributions: Vec::new(),
    });
    if let Some(subsection) = subsection {
        subsection.contributions.push(contribution);
    } else {
        section.contributions.push(contribution);
    }
}

fn flush_subsection(
    subsection: &mut Option<HansardSubsection>,
    section: &mut Option<HansardSection>,
) {
    if let Some(subsection) = subsection.take()
        && let Some(section) = section
    {
        section.subsections.push(subsection);
    }
}

fn validate_sections(sections: &[HansardSection]) -> Result<(), PdfError> {
    if sections.is_empty() {
        return Err(PdfError::Validation("no sections were parsed".to_owned()));
    }
    let contributions = sections.iter().flat_map(|section| {
        section.contributions.iter().chain(
            section
                .subsections
                .iter()
                .flat_map(|subsection| subsection.contributions.iter()),
        )
    });
    if !contributions
        .into_iter()
        .any(|contribution| !contribution.content.trim().is_empty())
    {
        return Err(PdfError::Validation(
            "no contribution content was parsed".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
                    NATIONAL ASSEMBLY
                     THE HANSARD

                    20th August 2026
20th August 2026          National Assembly Debates          1

                     THE HANSARD
                  Thursday, 20th August 2026

                   The House met at 2.30 p.m.

       [The Deputy Speaker (Hon. Gladys Boss) in the Chair]

                             PRAYERS

       Hon. Deputy Speaker: First Order.

                            PETITIONS

                COMPLIANCE WITH STATUTORY OBLIGATIONS

       Hon. Deputy Speaker: Hon. Members, this is the first
paragraph of the Petition.

                         (Several Members entered the
                          Chamber and took their seats)

       Hon. Example Member (Example, TEST): I support this
Petition.

 Disclaimer: The electronic version of the Official Hansard Report is for information
 purposes only. A certified copy can be obtained from the Hansard Editor.
20th August 2026          National Assembly Debates          2

       My contribution continues on the next page.

                            ADJOURNMENT

       Hon. Deputy Speaker: The House stands adjourned.
"#;

    #[test]
    fn parses_pdf_text_into_the_existing_sitting_shape() {
        let sitting = parse_sitting(SAMPLE, "https://parliament.go.ke/hansard.pdf").unwrap();

        assert_eq!(sitting.house, House::NationalAssembly);
        assert_eq!(sitting.date.to_string(), "2026-08-20");
        assert_eq!(sitting.day_of_week, "Thursday");
        assert_eq!(sitting.session_type, "Afternoon Sitting");
        assert_eq!(sitting.time.unwrap().to_string(), "14:30:00");
        assert_eq!(sitting.sections.len(), 4);
        assert_eq!(sitting.sections[2].section_type, "PETITIONS");
        assert_eq!(
            sitting.sections[2].subsections[0].title,
            "COMPLIANCE WITH STATUTORY OBLIGATIONS"
        );
        let contributions = &sitting.sections[2].subsections[0].contributions;
        assert_eq!(contributions.len(), 2);
        assert_eq!(contributions[0].speaker_name, "Hon. Deputy Speaker");
        assert_eq!(contributions[0].procedural_notes.len(), 1);
        assert!(
            contributions[1]
                .content
                .contains("continues on the next page")
        );
    }

    #[test]
    fn rejects_structurally_empty_transcripts() {
        let text = "NATIONAL ASSEMBLY\nThursday, 20th August 2026\nThe House met at 2.30 p.m.";
        assert!(matches!(
            parse_sitting(text, "https://parliament.go.ke/empty.pdf"),
            Err(PdfError::Validation(_))
        ));
    }

    #[test]
    fn recognizes_senate_speakers_and_plural_notices_heading() {
        assert_eq!(
            split_speaker("Sen. Example Member: Thank you.").unwrap(),
            ("Sen. Example Member", "Thank you.")
        );
        assert!(is_major_heading("NOTICES OF MOTIONS"));
        for heading in [
            "PRAYER",
            "PAPERS LAID",
            "PETITION",
            "STATEMENTS",
            "MOTION",
            "COMMUNICATIONS FROM THE CHAIR",
            "COMMITTEE OF THE WHOLE",
        ] {
            assert!(is_major_heading(heading), "missing heading: {heading}");
        }
    }

    #[test]
    fn parses_senate_house_and_chamber_time() {
        let text = "THE SENATE\nThursday, 20th August 2026\n\
            The House met in the Senate Chamber, Parliament Buildings, at 2.33 p.m.\n\
            THE NATIONAL ASSEMBLY";

        assert_eq!(parse_house(text).unwrap(), House::Senate);
        assert_eq!(parse_time(text).unwrap().unwrap().to_string(), "14:33:00");
    }

    #[test]
    fn prepares_markdown_speaker_boundaries() {
        let markdown = "Opening. **Hon. First Member** (Place, PARTY): First. \
            **Hon. Deputy Speaker:** Second. **PAPERS** **Sen Cherarkey:** Third. \
            **The Speaker (**Hon. Kingi): Fourth.";

        assert_eq!(
            prepare_markdown(markdown),
            "Opening.\nHon. First Member (Place, PARTY):\nFirst.\nHon. Deputy Speaker:\nSecond.\nPAPERS\n\nSen Cherarkey:\nThird.\nThe Speaker (Hon. Kingi):\nFourth."
        );
    }

    #[tokio::test]
    async fn rejects_non_pdf_downloads_before_running_the_extractor() {
        assert!(matches!(
            extract_text(b"<html>not a pdf</html>".to_vec()).await,
            Err(PdfError::Inspector(_))
        ));
    }
}
