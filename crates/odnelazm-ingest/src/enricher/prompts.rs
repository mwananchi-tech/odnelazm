use crate::{
    store::{
        PendingBillAppearanceSummary, PendingBillJourneySummary, PendingSittingSummary,
        PendingTopicAppearanceSummary,
    },
    summarize::SummaryContext,
};

/// Convert a sitting's raw_json to a human-readable transcript string.
/// The JSON uses camelCase keys (postgres.js transform).
pub fn transcript_to_text(raw_json: &serde_json::Value) -> String {
    let mut out = String::new();
    let empty = vec![];
    let sections = raw_json["sections"].as_array().unwrap_or(&empty);

    for section in sections {
        let section_type = section["sectionType"].as_str().unwrap_or("").trim();
        if !section_type.is_empty() {
            out.push_str(&format!("\n## {section_type}\n"));
        }

        // Top-level contributions
        for c in section["contributions"].as_array().unwrap_or(&empty) {
            push_contribution(&mut out, c);
        }

        // Subsections
        for sub in section["subsections"].as_array().unwrap_or(&empty) {
            let title = sub["title"].as_str().unwrap_or("").trim();
            if !title.is_empty() {
                out.push_str(&format!("\n### {title}\n"));
            }
            for c in sub["contributions"].as_array().unwrap_or(&empty) {
                push_contribution(&mut out, c);
            }
        }
    }
    out
}

fn push_contribution(out: &mut String, c: &serde_json::Value) {
    let speaker = c["speakerName"].as_str().unwrap_or("Unknown").trim();
    let content = c["content"].as_str().unwrap_or("").trim();
    if !content.is_empty() {
        out.push_str(&format!("[{speaker}]: {content}\n"));
    }
}

pub fn member_contribution_prompt(ctx: &SummaryContext, contributions_text: &str) -> String {
    let stage_line = ctx
        .stage
        .as_deref()
        .map(|s| format!("Stage: {s}\n"))
        .unwrap_or_default();

    format!(
        "You are analysing parliamentary contributions from the Parliament of Kenya.\n\
         \n\
         Member: {member}\n\
         {item_type_label}: {title}\n\
         {stage_line}\
         House: {house}\n\
         Date: {date}\n\
         \n\
         The member's contributions during this debate:\n\
         ---\n\
         {text}\n\
         ---\n\
         \n\
         In 2 to 3 sentences, summarise this member's position, key arguments, and any \
         notable statements. Be factual and concise. Do not invent details.",
        member = ctx.member_name,
        item_type_label = if ctx.item_type == "bill" {
            "Bill"
        } else {
            "Topic"
        },
        title = ctx.title,
        stage_line = stage_line,
        house = ctx.house,
        date = ctx.date,
        text = contributions_text,
    )
}

pub fn topic_appearance_prompt(p: &PendingTopicAppearanceSummary) -> String {
    let transcript = transcript_to_text(&p.sitting_raw_json);

    format!(
        "You are analysing a sitting of the Kenya Parliament.\n\
         \n\
         House: {house} | Date: {date} | Session: {session}\n\
         \n\
         Full sitting transcript:\n\
         ---\n\
         {transcript}\n\
         ---\n\
         \n\
         Focus only on the section titled: \"{title}\"\n\
         This is a {section_type} item.\n\
         \n\
         Summarise only that section in 3 to 5 sentences covering:\n\
         - What was raised, asked, or stated\n\
         - Key positions or responses from members or the government\n\
         - Any notable controversy, strong reaction, or follow-up action\n\
         - The outcome or resolution, if any\n\
         \n\
         Reply in markdown. If the section was procedural with no substantive discussion, say so briefly.",
        house = p.house,
        date = p.date,
        session = p.session_type,
        transcript = transcript,
        title = p.title,
        section_type = p.section_type,
    )
}

pub fn bill_appearance_prompt(p: &PendingBillAppearanceSummary) -> String {
    let transcript = transcript_to_text(&p.sitting_raw_json);

    let stage_line = p
        .stage
        .as_deref()
        .map(|s| format!("Stage: {s}\n"))
        .unwrap_or_default();

    format!(
        "You are analysing a sitting of the Kenya Parliament.\n\
         \n\
         House: {house} | Date: {date} | Session: {session}\n\
         \n\
         Full sitting transcript:\n\
         ---\n\
         {transcript}\n\
         ---\n\
         \n\
         Focus only on the section titled: \"{section_title}\"\n\
         This concerns: {bill_name}{bill_number_line}\n\
         {stage_line}\
         \n\
         Summarise only that section in 3 to 5 sentences covering:\n\
         - Was the bill substantively debated or briefly mentioned/deferred?\n\
         - Key arguments made for or against\n\
         - Any references to public participation, petitions, or civil society input\n\
         - Any controversies, strong opposition, or walkouts\n\
         - The outcome for the bill in this sitting\n\
         \n\
         Reply in markdown. If the section was procedural with no real debate, say so briefly.",
        house = p.house,
        date = p.date,
        session = p.session_type,
        transcript = transcript,
        section_title = p.section_title,
        bill_name = p.bill_name,
        bill_number_line = p
            .bill_number
            .as_deref()
            .map(|n| format!(" ({n})"))
            .unwrap_or_default(),
        stage_line = stage_line,
    )
}

pub fn bill_journey_prompt(p: &PendingBillJourneySummary) -> String {
    let mention_lines: String = p
        .mentions
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let stage = m.stage.as_deref().unwrap_or("Unknown stage");
            let summary = m
                .summary
                .as_deref()
                .unwrap_or_else(|| m.speakers_text.as_deref().unwrap_or("(no record)"));
            format!(
                "{}. {} | {} | {}\n   {}\n",
                i + 1,
                m.date,
                m.house,
                stage,
                summary
            )
        })
        .collect();

    let sponsor_line = p
        .sponsor
        .as_deref()
        .map(|s| format!("Moved by: {s}\n"))
        .unwrap_or_default();

    format!(
        "You are summarising the full legislative journey of a Kenyan parliamentary bill.\n\
         \n\
         Bill: {name}\n\
         {bill_number}\
         {year}\
         {sponsor}\
         \n\
         The bill appeared in {n} parliamentary sitting(s). Summary of each appearance:\n\
         \n\
         {mentions}\n\
         Write a 3 to 4 paragraph markdown summary covering:\n\
         1. What the bill proposes and its stated purpose\n\
         2. How it progressed through Parliament and whether it advanced or stalled\n\
         3. How it was received: support, opposition, political dynamics\n\
         4. Any references to public participation, petitions, or civil society pressure\n\
         5. Any controversies, walkouts, or politically charged moments\n\
         6. Current status or final outcome if discernible\n\
         \n\
         Be specific and factual. Do not invent details.",
        name = p.bill_name,
        bill_number = p
            .bill_number
            .as_deref()
            .map(|n| format!("Bill number: {n}\n"))
            .unwrap_or_default(),
        year = p.year.map(|y| format!("Year: {y}\n")).unwrap_or_default(),
        sponsor = sponsor_line,
        n = p.mentions.len(),
        mentions = mention_lines,
    )
}

pub fn sitting_prompt(p: &PendingSittingSummary) -> String {
    let transcript = transcript_to_text(&p.raw_json);

    let existing = p
        .existing_summary
        .as_deref()
        .map(|s| format!("\nExisting summary (for reference only):\n{s}\n"))
        .unwrap_or_default();

    format!(
        "You are summarising a Kenya parliamentary sitting.\n\
         \n\
         House: {house} | Date: {date} | Session: {session}\n\
         {existing}\n\
         Full transcript:\n\
         ---\n\
         {transcript}\n\
         ---\n\
         \n\
         Write a structured markdown summary with these sections (omit any section with no activity):\n\
         \n\
         ## Bills and Legislation\n\
         ## Questions and Statements\n\
         ## Motions and Notices\n\
         ## Key Debates and Controversies\n\
         ## Overall Tone\n\
         \n\
         Be specific: name members, name bills, note outcomes and positions. \
         Keep the total under 500 words.",
        house = p.house,
        date = p.date,
        session = p.session_type,
        existing = existing,
        transcript = transcript,
    )
}
