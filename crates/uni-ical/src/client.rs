use async_trait::async_trait;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::{ToSpan, Zoned};
use uni_core::model::Event;
use uni_core::{Calendar, Error, Result};
use url::Url;

use crate::SERVICE;
use crate::parse::{self, RawEvent};

const DEFAULT_DAYS_AFTER: i64 = 6;

pub struct IcalClient {
    http: reqwest::Client,
    /// Contains the student's token: never log it.
    link: Url,
    zone: TimeZone,
}

impl IcalClient {
    /// `link` must point to `host`, so nothing is ever fetched from elsewhere.
    pub fn new(http: reqwest::Client, host: &str, timezone: &str, link: &str) -> Result<Self> {
        // The same link is often handed out as webcal://.
        let link = link.trim().replacen("webcal://", "https://", 1);
        let link = Url::parse(&link)
            .ok()
            .filter(|url| url.scheme() == "https" && url.host_str() == Some(host))
            .ok_or_else(|| {
                Error::InvalidInput(format!("the calendar link must start with https://{host}/"))
            })?;
        let zone = TimeZone::get(timezone)
            .map_err(|e| Error::Config(format!("unknown time zone `{timezone}`: {e}")))?;
        Ok(Self { http, link, zone })
    }

    async fn fetch(&self) -> Result<String> {
        let response = self
            .http
            .get(self.link.clone())
            .send()
            .await
            .map_err(|e| self.network(e))?;
        if matches!(response.status().as_u16(), 401 | 403 | 404) {
            return Err(self.rejected());
        }
        let text = response
            .error_for_status()
            .map_err(|e| self.network(e))?
            .text()
            .await
            .map_err(|e| self.network(e))?;
        // An outdated link may still answer, just not with a calendar.
        if !text.trim_start().starts_with("BEGIN:VCALENDAR") {
            return Err(self.rejected());
        }
        Ok(text)
    }

    fn rejected(&self) -> Error {
        Error::Auth {
            service: SERVICE.into(),
        }
    }

    fn network(&self, e: reqwest::Error) -> Error {
        // The link carries the token, so the URL is dropped from the message.
        Error::Network {
            service: SERVICE.into(),
            message: e.without_url().to_string(),
        }
    }
}

#[async_trait]
impl Calendar for IcalClient {
    async fn events(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        search: Option<&str>,
    ) -> Result<Vec<Event>> {
        let from = match from {
            Some(day) => parse_day(day)?,
            None => Zoned::now().with_time_zone(self.zone.clone()).date(),
        };
        let to = match to {
            Some(day) => parse_day(day)?,
            None => from.saturating_add(DEFAULT_DAYS_AFTER.days()),
        };
        if to < from {
            return Err(Error::InvalidInput("`to` is before `from`".into()));
        }
        let feed = self.fetch().await?;
        let mut events = select(parse::parse(&feed), &self.zone, from, to);
        if let Some(search) = search.map(str::trim).filter(|s| !s.is_empty()) {
            events.retain(|event| title_contains(event, search));
        }
        Ok(events)
    }
}

fn title_contains(event: &Event, search: &str) -> bool {
    event.title.to_lowercase().contains(&search.to_lowercase())
}

fn parse_day(day: &str) -> Result<Date> {
    day.trim()
        .parse()
        .map_err(|_| Error::InvalidInput(format!("`{day}` is not a date like 2026-10-21")))
}

fn select(raw: Vec<RawEvent>, zone: &TimeZone, from: Date, to: Date) -> Vec<Event> {
    let mut selected: Vec<(Zoned, Event)> = raw
        .into_iter()
        .filter_map(|event| {
            let start = to_zoned(&event.start, zone)?;
            if start.date() < from || start.date() > to {
                return None;
            }
            let end = event.end.as_deref().and_then(|end| to_zoned(end, zone));
            let entry = Event {
                title: event.summary,
                start: local_time(&start),
                end: end.as_ref().map(local_time),
                weekday: start.strftime("%A").to_string(),
                location: event.location,
                note: event.description,
                url: event.url,
            };
            Some((start, entry))
        })
        .collect();
    selected.sort_by(|a, b| a.0.cmp(&b.0));
    selected.into_iter().map(|(_, event)| event).collect()
}

/// UTC times (`...Z`) are converted to `zone`; times without a zone and
/// whole days are taken to be local already.
fn to_zoned(value: &str, zone: &TimeZone) -> Option<Zoned> {
    if let Some(utc) = value.strip_suffix('Z') {
        let utc = DateTime::strptime("%Y%m%dT%H%M%S", utc).ok()?;
        return Some(
            utc.to_zoned(TimeZone::UTC)
                .ok()?
                .with_time_zone(zone.clone()),
        );
    }
    if !value.contains('T') {
        return Date::strptime("%Y%m%d", value)
            .ok()?
            .to_zoned(zone.clone())
            .ok();
    }
    DateTime::strptime("%Y%m%dT%H%M%S", value)
        .ok()?
        .to_zoned(zone.clone())
        .ok()
}

fn local_time(time: &Zoned) -> String {
    time.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn berlin() -> TimeZone {
        TimeZone::get("Europe/Berlin").expect("time zone")
    }

    fn raw(summary: &str, start: &str) -> RawEvent {
        RawEvent {
            summary: summary.into(),
            start: start.into(),
            ..RawEvent::default()
        }
    }

    fn day(day: &str) -> Date {
        parse_day(day).expect("date")
    }

    #[test]
    fn utc_times_become_local_times() {
        let mut lecture = raw("Lecture", "20261021T103000Z");
        lecture.end = Some("20261021T120000Z".into());
        let events = select(
            vec![lecture],
            &berlin(),
            day("2026-10-21"),
            day("2026-10-21"),
        );
        assert_eq!(events[0].start, "2026-10-21T12:30:00+02:00");
        assert_eq!(events[0].end.as_deref(), Some("2026-10-21T14:00:00+02:00"));
        assert_eq!(events[0].weekday, "Wednesday");
    }

    #[test]
    fn winter_time_is_one_hour_closer_to_utc() {
        let events = select(
            vec![raw("Lecture", "20261104T113000Z")],
            &berlin(),
            day("2026-11-04"),
            day("2026-11-04"),
        );
        assert_eq!(events[0].start, "2026-11-04T12:30:00+01:00");
    }

    #[test]
    fn only_the_asked_days_are_listed_in_order() {
        let events = select(
            vec![
                raw("Late", "20261022T140000Z"),
                raw("Before", "20261020T080000Z"),
                raw("Early", "20261022T060000Z"),
                raw("After", "20261023T080000Z"),
                raw("Broken", "tomorrow"),
            ],
            &berlin(),
            day("2026-10-21"),
            day("2026-10-22"),
        );
        let titles: Vec<&str> = events.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Early", "Late"]);
    }

    #[test]
    fn the_local_day_decides_not_the_utc_day() {
        // 22:30 UTC is already half past midnight of the next day in Berlin.
        let events = select(
            vec![raw("Night", "20261021T223000Z")],
            &berlin(),
            day("2026-10-22"),
            day("2026-10-22"),
        );
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn search_ignores_upper_and_lower_case() {
        let events = select(
            vec![raw("41.05295 CES Mechanik I VO", "20261021T080000Z")],
            &berlin(),
            day("2026-10-21"),
            day("2026-10-21"),
        );
        assert!(title_contains(&events[0], "mechanik"));
        assert!(!title_contains(&events[0], "Klausur"));
    }

    #[test]
    fn whole_days_are_local() {
        let events = select(
            vec![raw("Holiday", "20261224")],
            &berlin(),
            day("2026-12-24"),
            day("2026-12-24"),
        );
        assert_eq!(events[0].start, "2026-12-24T00:00:00+01:00");
    }

    #[test]
    fn the_link_must_be_on_the_university_host() {
        let client = |link| {
            IcalClient::new(
                reqwest::Client::new(),
                "online.example",
                "Europe/Berlin",
                link,
            )
        };
        assert!(client("https://online.example/ical?pToken=abc").is_ok());
        assert!(client("webcal://online.example/ical?pToken=abc").is_ok());
        assert!(client("http://online.example/ical?pToken=abc").is_err());
        assert!(client("https://evil.example/ical?pToken=abc").is_err());
        assert!(client("not a link").is_err());
    }
}
