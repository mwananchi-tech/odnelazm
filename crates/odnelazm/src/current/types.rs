use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use std::fmt::Display;

pub use crate::types::House;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardListing {
    pub house: House,
    pub date: NaiveDate,
    pub session_type: String,
    pub url: String,
    pub title: String,
}

impl Display for HansardListing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {} — {}", self.house, self.date, self.title)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardSitting {
    pub house: House,
    pub date: NaiveDate,
    pub day_of_week: String,
    pub session_type: String,
    pub time: Option<NaiveTime>,
    pub summary: Option<String>,
    pub sentiment: Option<String>,
    pub pdf_url: Option<String>,
    pub sections: Vec<HansardSection>,
}

impl Display for HansardSitting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "┌─ {} ─ {} ─ {}",
            self.house, self.date, self.session_type
        )?;
        if let Some(time) = self.time {
            writeln!(f, "│  Time: {}", time)?;
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
    pub contributions: Vec<Contribution>,
}

impl Display for HansardSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "── {}", self.section_type)?;
        for contrib in &self.contributions {
            write!(f, "{}", contrib)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contribution {
    pub speaker_name: String,
    pub speaker_url: Option<String>,
    pub content: String,
    pub procedural_notes: Vec<String>,
}

impl Display for Contribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "  ▸ {}", self.speaker_name)?;
        let preview: String = self.content.chars().take(120).collect();
        writeln!(f, "    {}", preview)?;
        for note in &self.procedural_notes {
            writeln!(f, "    [{}]", note)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub url: String,
    pub house: House,
    pub role: Option<String>,
    pub constituency: Option<String>,
}

impl Display for Member {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some(role) = &self.role {
            write!(f, " ({})", role)?;
        }
        if let Some(constituency) = &self.constituency {
            write!(f, " — {}", constituency)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bill {
    pub name: String,
    pub year: String,
    pub status: String,
}

impl Display for Bill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}) — {}", self.name, self.year, self.status)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteRecord {
    pub date: String,
    pub title: String,
    pub url: Option<String>,
    pub decision: String,
}

impl Display for VoteRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} — {} [{}]", self.date, self.title, self.decision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParliamentaryActivity {
    pub date: String,
    pub topic: String,
    pub contribution_type: String,
    pub section_title: String,
    pub sitting_url: String,
    pub text_preview: String,
    pub url: String,
}

impl Display for ParliamentaryActivity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} — {} ({})",
            self.date, self.section_title, self.topic, self.contribution_type
        )
    }
}

// TODO: Verify validity of counts to actual length of parsed data
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberProfile {
    pub name: String,
    pub slug: String,
    pub photo_url: Option<String>,
    pub biography: Option<String>,
    pub position_type: Option<String>,
    pub positions: Vec<String>,
    pub party: Option<String>,
    pub committees: Vec<String>,
    pub speeches_last_year: Option<u32>,
    pub speeches_total: Option<u32>,
    pub bills: Vec<Bill>,
    pub bills_total: Option<u32>,
    pub bills_pages: u32,
    pub voting_patterns: Vec<VoteRecord>,
    pub activity: Vec<ParliamentaryActivity>,
    pub activity_pages: u32,
}

impl Display for MemberProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.name)?;
        if !self.positions.is_empty() {
            writeln!(f, "  Positions: {}", self.positions.join(", "))?;
        }
        if let Some(party) = &self.party {
            writeln!(f, "  Party: {}", party)?;
        }
        if !self.committees.is_empty() {
            writeln!(f, "  Committees: {}", self.committees.join(", "))?;
        }
        if let Some(total) = self.speeches_total {
            writeln!(f, "  Total speeches: {}", total)?;
        }
        if let Some(total) = self.bills_total {
            writeln!(
                f,
                "  Bills sponsored: {} ({} page(s))",
                total, self.bills_pages
            )?;
        }
        if !self.voting_patterns.is_empty() {
            writeln!(f, "  Voting records: {}", self.voting_patterns.len())?;
        }
        if !self.activity.is_empty() {
            writeln!(
                f,
                "  Activity items: {} ({} page(s))",
                self.activity.len(),
                self.activity_pages
            )?;
        }
        Ok(())
    }
}
