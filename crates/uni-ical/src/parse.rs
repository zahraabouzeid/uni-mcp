//! Events from iCalendar text (RFC 5545). No recurrence rules: timetable
//! feeds list every date as its own event.

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RawEvent {
    pub summary: String,
    pub start: String,
    pub end: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
}

pub fn parse(text: &str) -> Vec<RawEvent> {
    let mut events = Vec::new();
    let mut current: Option<RawEvent> = None;

    for line in unfold(text) {
        match line.as_str() {
            "BEGIN:VEVENT" => current = Some(RawEvent::default()),
            "END:VEVENT" => {
                events.extend(current.take().filter(|event| !event.start.is_empty()));
            }
            _ => {
                let (Some(event), Some((name, value))) = (current.as_mut(), line.split_once(':'))
                else {
                    continue;
                };
                // Parameters such as `;VALUE=DATE` follow the name.
                match name.split(';').next().unwrap_or_default() {
                    "SUMMARY" => event.summary = unescape(value),
                    "DTSTART" => event.start = value.trim().to_owned(),
                    "DTEND" => event.end = non_empty(value.trim().to_owned()),
                    "LOCATION" => event.location = non_empty(unescape(value)),
                    "DESCRIPTION" => event.description = non_empty(unescape(value)),
                    "URL" => event.url = non_empty(value.trim().to_owned()),
                    _ => {}
                }
            }
        }
    }
    events
}

/// A line that starts with a space or tab continues the one before.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        match (line.strip_prefix([' ', '\t']), lines.last_mut()) {
            (Some(continuation), Some(previous)) => previous.push_str(continuation),
            _ => lines.push(line.to_owned()),
        }
    }
    lines
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(escaped) => out.push(escaped),
            None => {}
        }
    }
    out.trim().to_owned()
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Someone\r\n\
BEGIN:VEVENT\r\nUID:1@example.org\r\nSTATUS:CONFIRMED\r\n\
URL:https://online.example/wbLv.wbShowLVDetail?pStpS\r\n pNr=580489\r\n\
SUMMARY:41.09000 Messtechnik und Datenanalyse VO\\, Standardgruppe\r\n\
DESCRIPTION:fix\\; Abhaltung\\; \r\n\
DTSTART:20261021T103000Z\r\nDTEND:20261021T120000Z\r\n\
LOCATION:H09 (1385|220)\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261224\r\nLOCATION:\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nSUMMARY:No start\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";

    #[test]
    fn reads_a_campusonline_event() {
        let events = parse(FEED);
        assert_eq!(events.len(), 2);
        let lecture = &events[0];
        assert_eq!(
            lecture.summary,
            "41.09000 Messtechnik und Datenanalyse VO, Standardgruppe"
        );
        assert_eq!(lecture.start, "20261021T103000Z");
        assert_eq!(lecture.end.as_deref(), Some("20261021T120000Z"));
        assert_eq!(lecture.location.as_deref(), Some("H09 (1385|220)"));
        assert_eq!(lecture.description.as_deref(), Some("fix; Abhaltung;"));
    }

    #[test]
    fn joins_folded_lines() {
        let url = parse(FEED)[0].url.clone().expect("url");
        assert_eq!(
            url,
            "https://online.example/wbLv.wbShowLVDetail?pStpSpNr=580489"
        );
    }

    #[test]
    fn keeps_date_only_events_and_drops_empty_fields() {
        let holiday = &parse(FEED)[1];
        assert_eq!(holiday.start, "20261224");
        assert_eq!(holiday.end, None);
        assert_eq!(holiday.location, None);
    }
}
