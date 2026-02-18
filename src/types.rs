use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum House {
    Senate,
    NationalAssembly,
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

