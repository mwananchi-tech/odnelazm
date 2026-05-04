use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use std::fmt::Display;

/// Options for [`HansardScraper::list_sittings`].
///
/// The data source is chosen automatically:
/// - No dates → current source, paged via `page`/`all`.
/// - `end_date` before 2013-03-28 → archive only.
/// - `start_date` on or after 2013-03-28 → current only, paged via `page`/`all`.
/// - Range spans the cutoff (or one bound is absent while the other crosses it)
///   → both sources fetched in parallel and merged; `page`/`all` are ignored.
///
/// `limit` and `offset` are applied client-side after any merging and sorting.
#[derive(Debug, Clone, Default)]
pub struct SittingListOptions {
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub house: Option<House>,
    /// Page number for current-source pagination (default: 1). Ignored for cross-source queries.
    pub page: u32,
    /// Fetch all pages at once from the current source. Ignored for cross-source queries.
    pub all: bool,
    /// Maximum results to return (applied after merging and sorting).
    pub limit: Option<usize>,
    /// Results to skip (applied after merging and sorting).
    pub offset: Option<usize>,
}

pub use crate::current::types::{Bill, Member, MemberProfile, ParliamentaryActivity, VoteRecord};
pub use crate::types::House;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    Archive,
    Current,
}

impl Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataSource::Archive => write!(f, "{}", crate::archive::BASE_URL),
            DataSource::Current => write!(f, "{}", crate::current::BASE_URL),
        }
    }
}

impl Serialize for DataSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardListing {
    pub house: House,
    pub date: NaiveDate,
    pub url: String,
    pub title: String,
    pub session_type: Option<String>,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub source: DataSource,
}

impl Display for HansardListing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {} — {}", self.house, self.date, self.title)?;
        if let Some(session_type) = &self.session_type {
            write!(f, " ({})", session_type)?;
        }
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => write!(f, "\n   Time: {} – {}", start, end)?,
            (Some(start), None) => write!(f, "\n   Start: {}", start)?,
            _ => {}
        }
        write!(f, "\n   {}", self.url)
    }
}

impl From<crate::archive::types::HansardListing> for HansardListing {
    fn from(l: crate::archive::types::HansardListing) -> Self {
        Self {
            house: l.house,
            date: l.date,
            url: l.url,
            title: l.display_text,
            session_type: None,
            start_time: l.start_time,
            end_time: l.end_time,
            source: DataSource::Archive,
        }
    }
}

impl From<crate::current::types::HansardListing> for HansardListing {
    fn from(l: crate::current::types::HansardListing) -> Self {
        Self {
            house: l.house,
            date: l.date,
            url: l.url,
            title: l.title,
            session_type: Some(l.session_type),
            start_time: None,
            end_time: None,
            source: DataSource::Current,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardSitting {
    pub house: House,
    pub date: NaiveDate,
    pub url: String,
    pub session_type: String,
    pub sections: Vec<HansardSection>,
    pub source: DataSource,
    pub day_of_week: Option<String>,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub parliament_number: Option<String>,
    pub session_number: Option<String>,
    pub speaker_in_chair: Option<String>,
    pub summary: Option<String>,
    pub sentiment: Option<String>,
    pub pdf_url: Option<String>,
}

impl HansardSitting {
    pub(crate) fn from_archive(
        sitting: crate::archive::types::HansardSitting,
        url: String,
    ) -> Self {
        Self {
            house: sitting.house,
            date: sitting.date,
            url,
            session_type: sitting.session_type,
            sections: sitting
                .sections
                .into_iter()
                .map(HansardSection::from)
                .collect(),
            source: DataSource::Archive,
            day_of_week: None,
            start_time: sitting.start_time,
            end_time: sitting.end_time,
            parliament_number: Some(sitting.parliament_number),
            session_number: Some(sitting.session_number),
            speaker_in_chair: Some(sitting.speaker_in_chair),
            summary: None,
            sentiment: None,
            pdf_url: None,
        }
    }

    pub(crate) fn from_current(
        sitting: crate::current::types::HansardSitting,
        url: String,
    ) -> Self {
        Self {
            house: sitting.house,
            date: sitting.date,
            url,
            session_type: sitting.session_type,
            sections: sitting
                .sections
                .into_iter()
                .map(HansardSection::from)
                .collect(),
            source: DataSource::Current,
            day_of_week: Some(sitting.day_of_week),
            start_time: sitting.time,
            end_time: None,
            parliament_number: None,
            session_number: None,
            speaker_in_chair: None,
            summary: sitting.summary,
            sentiment: sitting.sentiment,
            pdf_url: sitting.pdf_url,
        }
    }
}

impl Display for HansardSitting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "┌─ {} ─ {} ─ {}",
            self.house, self.date, self.session_type
        )?;
        writeln!(f, "│  Source: {}", self.source)?;
        writeln!(f, "│  URL:    {}", self.url)?;
        if let Some(dow) = &self.day_of_week {
            writeln!(f, "│  Day:    {}", dow)?;
        }
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => writeln!(f, "│  Time:   {} – {}", start, end)?,
            (Some(start), None) => writeln!(f, "│  Time:   {}", start)?,
            _ => {}
        }
        if let Some(parl) = &self.parliament_number {
            writeln!(
                f,
                "│  Parliament: {} · Session: {} ({})",
                parl,
                self.session_number.as_deref().unwrap_or(""),
                self.session_type
            )?;
        }
        if let Some(chair) = &self.speaker_in_chair {
            writeln!(f, "│  Chair: {}", chair)?;
        }
        if let Some(summary) = &self.summary {
            let preview: String = summary.chars().take(120).collect();
            writeln!(f, "│  Summary: {}…", preview)?;
        }
        writeln!(f, "└─ {} section(s)", self.sections.len())?;
        writeln!(f)?;
        for (i, section) in self.sections.iter().enumerate() {
            writeln!(f, "{:>2}. {}", i + 1, section)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardSection {
    pub section_type: String,
    pub subsections: Vec<HansardSubsection>,
    pub contributions: Vec<Contribution>,
}

impl From<crate::archive::types::HansardSection> for HansardSection {
    fn from(s: crate::archive::types::HansardSection) -> Self {
        let section_type = match s.title {
            Some(title) => format!("{}: {}", s.section_type, title),
            None => s.section_type,
        };
        Self {
            section_type,
            subsections: vec![],
            contributions: s
                .contributions
                .into_iter()
                .map(Contribution::from)
                .collect(),
        }
    }
}

impl From<crate::current::types::HansardSection> for HansardSection {
    fn from(s: crate::current::types::HansardSection) -> Self {
        Self {
            section_type: s.section_type,
            subsections: s
                .subsections
                .into_iter()
                .map(HansardSubsection::from)
                .collect(),
            contributions: s
                .contributions
                .into_iter()
                .map(Contribution::from)
                .collect(),
        }
    }
}

impl Display for HansardSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "── {}", self.section_type)?;
        for contrib in &self.contributions {
            write!(f, "{}", contrib)?;
        }
        for subsection in &self.subsections {
            write!(f, "{}", subsection)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardSubsection {
    pub title: String,
    pub contributions: Vec<Contribution>,
}

impl From<crate::current::types::HansardSubsection> for HansardSubsection {
    fn from(s: crate::current::types::HansardSubsection) -> Self {
        Self {
            title: s.title,
            contributions: s
                .contributions
                .into_iter()
                .map(Contribution::from)
                .collect(),
        }
    }
}

impl Display for HansardSubsection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "  ── {}", self.title)?;
        for contrib in &self.contributions {
            write!(f, "{}", contrib)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contribution {
    pub speaker_name: String,
    pub speaker_role: Option<String>,
    pub speaker_url: Option<String>,
    pub content: String,
    pub procedural_notes: Vec<String>,
}

impl From<crate::archive::types::Contribution> for Contribution {
    fn from(c: crate::archive::types::Contribution) -> Self {
        Self {
            speaker_name: c.speaker_name,
            speaker_role: c.speaker_role,
            speaker_url: c.speaker_url,
            content: c.content,
            procedural_notes: c.procedural_notes,
        }
    }
}

impl From<crate::current::types::Contribution> for Contribution {
    fn from(c: crate::current::types::Contribution) -> Self {
        Self {
            speaker_name: c.speaker_name,
            speaker_role: None,
            speaker_url: c.speaker_url,
            content: c.content,
            procedural_notes: c.procedural_notes,
        }
    }
}

impl Display for Contribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "  ▸ {}", self.speaker_name)?;
        if let Some(role) = &self.speaker_role {
            write!(f, " ({})", role)?;
        }
        writeln!(f)?;
        let preview: String = self.content.chars().take(120).collect();
        writeln!(f, "    {}", preview)?;
        for note in &self.procedural_notes {
            writeln!(f, "    [{}]", note)?;
        }
        Ok(())
    }
}
