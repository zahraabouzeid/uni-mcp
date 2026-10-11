use async_trait::async_trait;
use futures_util::future::try_join_all;
use jiff::Zoned;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use reqwest::StatusCode;
use serde::Deserialize;
use uni_core::model::{CanteenMenu, Meal};
use uni_core::profile::{CanteenEntry, CanteensProfile};
use uni_core::{Canteens, Error, Result};

use crate::SERVICE;

pub struct OpenMensaClient {
    http: reqwest::Client,
    api: String,
    canteens: Vec<CanteenEntry>,
    zone: TimeZone,
}

impl OpenMensaClient {
    pub fn new(http: reqwest::Client, profile: &CanteensProfile, timezone: &str) -> Result<Self> {
        let zone = TimeZone::get(timezone)
            .map_err(|e| Error::Config(format!("unknown time zone `{timezone}`: {e}")))?;
        Ok(Self {
            http,
            api: profile.api.trim_end_matches('/').to_owned(),
            canteens: profile.list.clone(),
            zone,
        })
    }

    async fn menu(&self, canteen: &CanteenEntry, date: Date) -> Result<CanteenMenu> {
        let url = format!("{}/canteens/{}/days/{date}/meals", self.api, canteen.id);
        let response = self.http.get(url).send().await.map_err(network)?;
        // A day the canteen has published nothing for does not exist in the API.
        let meals = if response.status() == StatusCode::NOT_FOUND {
            Vec::new()
        } else {
            let body = response
                .error_for_status()
                .map_err(network)?
                .text()
                .await
                .map_err(network)?;
            parse(&body)?
        };
        Ok(CanteenMenu {
            canteen: canteen.name.clone(),
            date: date.to_string(),
            weekday: date.strftime("%A").to_string(),
            meals,
        })
    }
}

#[async_trait]
impl Canteens for OpenMensaClient {
    async fn menus(&self, date: Option<&str>, canteen: Option<&str>) -> Result<Vec<CanteenMenu>> {
        let date = match date {
            Some(day) => parse_day(day)?,
            None => Zoned::now().with_time_zone(self.zone.clone()).date(),
        };
        let canteens = select(&self.canteens, canteen)?;
        try_join_all(canteens.into_iter().map(|canteen| self.menu(canteen, date))).await
    }
}

#[derive(Deserialize)]
struct RawMeal {
    name: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    prices: RawPrices,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Default, Deserialize)]
struct RawPrices {
    students: Option<f64>,
    others: Option<f64>,
}

fn parse(body: &str) -> Result<Vec<Meal>> {
    let meals: Vec<RawMeal> = serde_json::from_str(body).map_err(|e| Error::Protocol {
        service: SERVICE.into(),
        message: e.to_string(),
    })?;
    Ok(meals
        .into_iter()
        .map(|meal| Meal {
            name: meal.name,
            category: meal.category,
            student_price: meal.prices.students,
            guest_price: meal.prices.others,
            notes: meal.notes,
        })
        .collect())
}

fn select<'a>(canteens: &'a [CanteenEntry], search: Option<&str>) -> Result<Vec<&'a CanteenEntry>> {
    let Some(search) = search.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(canteens.iter().collect());
    };
    let wanted = search.to_lowercase();
    let found: Vec<&CanteenEntry> = canteens
        .iter()
        .filter(|canteen| canteen.name.to_lowercase().contains(&wanted))
        .collect();
    if found.is_empty() {
        let names: Vec<&str> = canteens.iter().map(|c| c.name.as_str()).collect();
        return Err(Error::InvalidInput(format!(
            "no canteen is called `{search}`; there are: {}",
            names.join(", ")
        )));
    }
    Ok(found)
}

fn parse_day(day: &str) -> Result<Date> {
    day.trim()
        .parse()
        .map_err(|_| Error::InvalidInput(format!("`{day}` is not a date like 2026-10-21")))
}

fn network(e: reqwest::Error) -> Error {
    Error::Network {
        service: SERVICE.into(),
        message: e.without_url().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEALS: &str = r#"[
        {
            "id": 1,
            "name": "Linsen-Dal mit Reis",
            "category": "Vegetarisch",
            "prices": { "students": 2.6, "employees": 4.1, "pupils": null, "others": 5.2 },
            "notes": ["vegan", "Sellerie"]
        },
        { "id": 2, "name": "Salatbuffet", "category": "Buffet", "prices": {} }
    ]"#;

    fn canteens() -> Vec<CanteenEntry> {
        ["Mensa Nord", "Mensa Süd", "Bistro"]
            .into_iter()
            .zip(1..)
            .map(|(name, id)| CanteenEntry {
                id,
                name: name.into(),
            })
            .collect()
    }

    fn names(found: Vec<&CanteenEntry>) -> Vec<&str> {
        found.into_iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn reads_meals_with_prices_and_notes() {
        let meals = parse(MEALS).expect("meals");
        assert_eq!(
            meals[0],
            Meal {
                name: "Linsen-Dal mit Reis".into(),
                category: "Vegetarisch".into(),
                student_price: Some(2.6),
                guest_price: Some(5.2),
                notes: vec!["vegan".into(), "Sellerie".into()],
            }
        );
    }

    #[test]
    fn a_meal_without_prices_or_notes_is_still_listed() {
        let meals = parse(MEALS).expect("meals");
        assert_eq!(meals[1].name, "Salatbuffet");
        assert_eq!(meals[1].student_price, None);
        assert!(meals[1].notes.is_empty());
    }

    #[test]
    fn something_else_than_a_menu_is_an_error() {
        assert!(parse("<html>").is_err());
    }

    #[test]
    fn without_a_name_every_canteen_is_asked() {
        let canteens = canteens();
        assert_eq!(select(&canteens, None).expect("all").len(), 3);
        assert_eq!(select(&canteens, Some("  ")).expect("all").len(), 3);
    }

    #[test]
    fn a_part_of_the_name_is_enough() {
        let canteens = canteens();
        let found = select(&canteens, Some("mensa")).expect("found");
        assert_eq!(names(found), ["Mensa Nord", "Mensa Süd"]);
        let found = select(&canteens, Some("SÜD")).expect("found");
        assert_eq!(names(found), ["Mensa Süd"]);
    }

    #[test]
    fn an_unknown_canteen_lists_the_ones_there_are() {
        let error = select(&canteens(), Some("West")).expect_err("unknown");
        assert!(error.to_string().contains("Mensa Nord, Mensa Süd, Bistro"));
    }

    #[test]
    fn the_day_must_be_a_date() {
        assert!(parse_day("2026-10-12").is_ok());
        assert!(parse_day("monday").is_err());
    }
}
