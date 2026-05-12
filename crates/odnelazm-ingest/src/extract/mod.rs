pub mod bills;
pub mod speakers;

pub use bills::{ExtractedBillMention, extract_bills};
pub use speakers::extract_speakers;
