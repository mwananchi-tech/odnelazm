use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum House {
    Senate,
    NationalAssembly,
}

impl House {
    pub fn to_string(&self) -> &str {
        match self {
            House::Senate => "Senate",
            House::NationalAssembly => "National Assembly",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardListing {
    pub house: House,
    pub date: NaiveDate,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub url: String,
    pub display_text: String,
}

impl HansardListing {
    pub fn new(
        house: House,
        date: NaiveDate,
        start_time: Option<NaiveTime>,
        end_time: Option<NaiveTime>,
        url: String,
        display_text: String,
    ) -> Self {
        Self {
            house,
            date,
            start_time,
            end_time,
            url,
            display_text,
        }
    }

    pub fn house_name(&self) -> &str {
        match self.house {
            House::Senate => "Senate",
            House::NationalAssembly => "National Assembly",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardDetail {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HansardSection {
    pub section_type: String,
    pub title: Option<String>,
    pub contributions: Vec<Contribution>,
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
