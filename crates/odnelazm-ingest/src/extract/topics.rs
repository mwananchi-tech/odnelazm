use std::collections::HashMap;

use odnelazm::HansardSitting;

use crate::extract::speakers::is_noise_speaker;
use crate::store::SpeakerRecord;

pub struct TopicContributor {
    pub speaker: SpeakerRecord,
    pub speech_count: u32,
    pub contributions_text: String,
}

/// Section types we extract as topics. Order matters — more specific first.
static TOPIC_SECTION_TYPES: &[&str] = &[
    "QUESTIONS AND STATEMENTS",
    "STATEMENTS",
    "STATEMENT",
    "NOTICES OF MOTIONS",
    "NOTICES OF MOTION",
    "NOTICE OF MOTION",
    "COMMUNICATION FROM THE CHAIR",
    "COMMUNICATIONS FROM THE CHAIR",
    "ADJOURNMENT",
];

/// A non-bill discussion topic extracted from a sitting (question, statement, motion, etc.).
pub struct ExtractedTopic {
    pub section_type: String,
    pub title: String,
    pub speech_count: u32,
    pub contributors: Vec<TopicContributor>,
}

fn is_topic_section(section_type: &str) -> bool {
    let upper = section_type.trim().to_uppercase();
    TOPIC_SECTION_TYPES.contains(&upper.as_str())
}

/// Normalise an ALL-CAPS subsection title to title case.
fn normalise_title(raw: &str) -> String {
    raw.split_whitespace()
        .map(|word| {
            let (prefix, rest) =
                word.split_at(word.find(|c: char| c.is_alphabetic()).unwrap_or(word.len()));
            let mut chars = rest.chars();
            match chars.next() {
                None => prefix.to_string(),
                Some(first) => format!(
                    "{}{}{}",
                    prefix,
                    first.to_uppercase(),
                    chars.as_str().to_lowercase()
                ),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn tally_speakers<'a>(
    contribs: impl Iterator<Item = &'a odnelazm::Contribution>,
) -> Vec<TopicContributor> {
    type Key = (String, Option<String>);
    type Val<'b> = (u32, Vec<&'b str>);
    let mut map: HashMap<Key, Val<'a>> = HashMap::new();
    for c in contribs {
        if is_noise_speaker(&c.speaker_name) {
            continue;
        }
        let entry = map
            .entry((c.speaker_name.clone(), c.speaker_url.clone()))
            .or_default();
        entry.0 += 1;
        entry.1.push(c.content.as_str());
    }
    map.into_iter()
        .map(|((name, url), (speech_count, texts))| TopicContributor {
            speaker: SpeakerRecord { name, url },
            speech_count,
            contributions_text: texts.join("\n\n"),
        })
        .collect()
}

/// Extract questions, statements, and other non-bill discussion topics from a sitting.
///
/// Only subsections with at least one non-noise contribution are returned.
/// Bill-related subsections (title ends with BILL or ACT) are skipped — those
/// are already captured by the bill extractor.
pub fn extract_topics(sitting: &HansardSitting) -> Vec<ExtractedTopic> {
    let mut topics: Vec<ExtractedTopic> = Vec::new();

    for section in &sitting.sections {
        if !is_topic_section(&section.section_type) {
            continue;
        }

        for subsection in &section.subsections {
            let title = subsection.title.trim();

            if title.is_empty() || title.len() < 5 {
                continue;
            }

            // Skip bill/act debates — handled by the bill extractor
            let upper = title.to_uppercase();
            if upper.ends_with("BILL") || upper.ends_with(" ACT") {
                continue;
            }

            let contributors = tally_speakers(subsection.contributions.iter());
            let speech_count = subsection.contributions.len() as u32;

            // Skip subsections with no meaningful contributions
            if contributors.is_empty() {
                continue;
            }

            topics.push(ExtractedTopic {
                section_type: normalise_title(&section.section_type),
                title: normalise_title(title),
                speech_count,
                contributors,
            });
        }
    }

    topics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_caps_title() {
        assert_eq!(
            normalise_title("UPGRADING OF A ROAD IN SUBUKIA"),
            "Upgrading Of A Road In Subukia"
        );
        assert_eq!(
            normalise_title("EFFECTS OF THE ONGOING US/ISRAEL-IRAN CONFLICT"),
            "Effects Of The Ongoing Us/israel-iran Conflict"
        );
    }
}
