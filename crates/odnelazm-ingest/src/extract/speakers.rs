use std::collections::HashMap;

use odnelazm::HansardSitting;

use crate::store::SpeakerRecord;

/// Returns true for names that should never be stored as speakers:
/// - Collective address terms ("Hon. Members", "Hon. Senators")
/// - Presiding-role-plus-motion artifacts ("Hon. Speaker, I beg to move")
/// - Bare role fragments ("Hon", "Sen")
/// - Digits-first garbage (table row leakage)
/// - Excessively long names (multiple lines merged by the parser)
pub fn is_noise_speaker(name: &str) -> bool {
    let t = name.trim();

    if t.len() < 4 {
        return true;
    }
    if t.starts_with(|c: char| c.is_ascii_digit()) {
        return true;
    }
    // Parser artefact: role label concatenated with opening motion phrase
    if t.contains("I beg to move") {
        return true;
    }
    // Implausibly long — almost certainly a multi-line parse error
    if t.len() > 150 {
        return true;
    }

    let lower = t.to_lowercase();
    matches!(
        lower.as_str(),
        "hon"
            | "hon."
            | "sen"
            | "sen."
            | "hon. members"
            | "hon members"
            | "hon. member"
            | "hon member"
            | "hon. senators"
            | "hon senators"
            | "hon. senator"
            | "hon senator"
            | "hon. chairman"
            | "hon chairman"
    )
}

/// Walk every contribution in a sitting and tally speech counts per unique
/// (name, url) pair. Noise speaker names are silently skipped.
pub fn extract_speakers(sitting: &HansardSitting) -> Vec<(SpeakerRecord, u32)> {
    let mut counts: HashMap<(String, Option<String>), u32> = HashMap::new();

    for section in &sitting.sections {
        for contrib in &section.contributions {
            if !is_noise_speaker(&contrib.speaker_name) {
                *counts
                    .entry((contrib.speaker_name.clone(), contrib.speaker_url.clone()))
                    .or_default() += 1;
            }
        }
        for subsection in &section.subsections {
            for contrib in &subsection.contributions {
                if !is_noise_speaker(&contrib.speaker_name) {
                    *counts
                        .entry((contrib.speaker_name.clone(), contrib.speaker_url.clone()))
                        .or_default() += 1;
                }
            }
        }
    }

    counts
        .into_iter()
        .map(|((name, url), count)| (SpeakerRecord { name, url }, count))
        .collect()
}
