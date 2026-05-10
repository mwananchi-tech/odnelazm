use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};

pub use crate::types::House;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardListing {
    pub house: House,
    pub date: NaiveDate,
    pub session_type: String,
    pub url: String,
    pub title: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardSubsection {
    pub title: String,
    pub contributions: Vec<Contribution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardSection {
    pub section_type: String,
    pub subsections: Vec<HansardSubsection>,
    pub contributions: Vec<Contribution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contribution {
    pub speaker_name: String,
    pub speaker_url: Option<String>,
    pub content: String,
    pub procedural_notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub url: String,
    pub house: House,
    pub role: Option<String>,
    pub constituency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bill {
    pub name: String,
    pub year: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteRecord {
    pub date: String,
    pub title: String,
    pub url: Option<String>,
    pub decision: String,
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

// TODO: verify validity of counts to actual length of parsed data
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
