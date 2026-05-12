use std::collections::HashMap;

use odnelazm::HansardSitting;

use crate::store::SpeakerRecord;

/// Walk every contribution in a sitting and tally speech counts per unique
/// (name, url) pair. Returns one record per distinct speaker.
pub fn extract_speakers(sitting: &HansardSitting) -> Vec<(SpeakerRecord, u32)> {
    let mut counts: HashMap<(String, Option<String>), u32> = HashMap::new();

    for section in &sitting.sections {
        for contrib in &section.contributions {
            *counts
                .entry((contrib.speaker_name.clone(), contrib.speaker_url.clone()))
                .or_default() += 1;
        }
        for subsection in &section.subsections {
            for contrib in &subsection.contributions {
                *counts
                    .entry((contrib.speaker_name.clone(), contrib.speaker_url.clone()))
                    .or_default() += 1;
            }
        }
    }

    counts
        .into_iter()
        .map(|((name, url), count)| (SpeakerRecord { name, url }, count))
        .collect()
}
