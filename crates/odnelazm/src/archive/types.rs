use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use std::fmt::Display;

pub use crate::types::House;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardListing {
    pub house: House,
    pub date: NaiveDate,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub url: String,
    pub display_text: String,
}

impl Display for HansardListing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {} — {}", self.house, self.date, self.display_text)?;

        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => write!(f, "\n   Time: {} – {}", start, end),
            (Some(start), None) => write!(f, "\n   Start: {}", start),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardSitting {
    pub house: House,
    pub date: NaiveDate,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub parliament_number: String,
    pub session_number: String,
    pub session_type: String,
    pub speaker_in_chair: String,
    pub sections: Vec<HansardSection>,
}

impl Display for HansardSitting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "┌─ {} ─ {}", self.house, self.date)?;
        if let Some(start) = self.start_time {
            write!(f, "│  Time:    {}", start)?;
            if let Some(end) = self.end_time {
                write!(f, " – {}", end)?;
            }
            writeln!(f)?;
        }
        writeln!(
            f,
            "│  Parliament: {} · Session: {} ({})",
            self.parliament_number, self.session_number, self.session_type
        )?;
        writeln!(f, "│  Chair: {}", self.speaker_in_chair)?;
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
    pub title: Option<String>,
    pub contributions: Vec<Contribution>,
}

impl Display for HansardSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "── {}", self.section_type)?;
        if let Some(title) = &self.title {
            write!(f, ": {}", title)?;
        }
        writeln!(f)?;
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
    pub speaker_details: Option<PersonDetails>,
    pub content: String,
    pub procedural_notes: Vec<String>,
}

impl Display for Contribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "  ▸ {}", self.speaker_name)?;
        if let Some(role) = &self.speaker_role {
            write!(f, " ({})", role)?;
        }
        writeln!(f)?;
        if let Some(details) = &self.speaker_details {
            writeln!(f, "    {}", details)?;
        }
        writeln!(f, "    {}", self.content)?;
        for note in &self.procedural_notes {
            writeln!(f, "    [{}]", note)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonDetails {
    pub name: String,
    pub slug: String,
    pub summary: Option<String>,
    pub party: Option<String>,
    pub party_url: Option<String>,
    pub email: Option<String>,
    pub telephone: Option<String>,
    pub current_position: Option<String>,
    pub constituency: Option<String>,
}

impl Display for PersonDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some(pos) = &self.current_position {
            write!(f, " · {}", pos)?;
        }
        if let Some(party) = &self.party {
            write!(f, " · {}", party)?;
        }
        if let Some(constituency) = &self.constituency {
            write!(f, "\n      Constituency: {}", constituency)?;
        }
        if let Some(email) = &self.email {
            write!(f, "\n      Email:          {}", email)?;
        }
        if let Some(tel) = &self.telephone {
            write!(f, "\n      Tel:            {}", tel)?;
        }
        Ok(())
    }
}
