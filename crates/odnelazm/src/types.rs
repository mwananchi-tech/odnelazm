use std::{fmt::Display, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
#[error("Invalid house '{0}'. Accepted values: 'senate', 'national_assembly', 'na'")]
pub struct HouseParseError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum House {
    Senate,
    NationalAssembly,
}

impl House {
    pub fn slug(&self) -> &'static str {
        match self {
            House::Senate => "senate",
            House::NationalAssembly => "national-assembly",
        }
    }
}

impl FromStr for House {
    type Err = HouseParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "senate" => Ok(House::Senate),
            "national_assembly" | "na" => Ok(House::NationalAssembly),
            _ => Err(HouseParseError(s.to_string())),
        }
    }
}

impl Display for House {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            House::Senate => write!(f, "Senate"),
            House::NationalAssembly => write!(f, "National Assembly"),
        }
    }
}
