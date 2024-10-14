pub(crate) mod cache;
pub(crate) mod file_io;
pub(crate) mod table;

use crate::{extra_settings::schema::ExtraSettings, models::Day};
use anyhow::Error;
use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use lazy_static::lazy_static;
use serde::Serialize;
use std::{mem, path::Path};
use tokio::{fs::File, io::AsyncWriteExt};

lazy_static! {
    pub(crate) static ref WORK_DAY_HOURS: f32 = 7.5;
}
pub(crate) struct DateRange(pub(crate) NaiveDate, pub(crate) NaiveDate);

impl Iterator for DateRange {
    type Item = NaiveDate;
    fn next(&mut self) -> Option<Self::Item> {
        if self.0 <= self.1 {
            let next = self.0 + Duration::days(1);
            Some(mem::replace(&mut self.0, next))
        } else {
            None
        }
    }
}

pub(crate) fn not_in_future(date: &NaiveDate) -> bool {
    &today() >= date
}

pub(crate) fn hours_to_hours_and_minutes(hours: f32) -> (i64, i64) {
    let whole_hours = hours.trunc() as i64;
    let minutes = ((hours - whole_hours as f32) * 60.0).round() as i64;
    (whole_hours, minutes.abs())
}

pub(crate) fn seconds_to_hours_and_minutes(seconds: i64) -> (i64, i64) {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    (hours, minutes.abs())
}

pub(crate) fn is_weekday(date: &NaiveDate) -> bool {
    [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
    ]
    .contains(&date.weekday())
}

pub(crate) fn get_all_weekdays_since(date: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    DateRange(date, today()).filter(is_weekday)
}

pub(crate) fn days_to_secs(day_count: usize) -> i64 {
    (day_count as f32 * *WORK_DAY_HOURS * 3600f32) as i64
}

/// Do proper calculations with ExtraSettings
pub(crate) fn workdays_to_secs(
    days: Vec<NaiveDate>,
    extra_settings: &Option<ExtraSettings>,
) -> i64 {
    if let Some(settings) = extra_settings {
        days.into_iter()
            .map(|d| {
                settings
                    .expected_working_secs(&d)
                    .unwrap_or((*WORK_DAY_HOURS * 3600f32) as i64)
            })
            .sum()
    } else {
        days_to_secs(days.len())
    }
}

pub(crate) fn today() -> NaiveDate {
    Utc::now().date_naive()
}

pub(crate) async fn get_public_holidays(since: &NaiveDate) -> Result<Vec<Day>, Error> {
    let json_bytes = include_bytes!("../holidays.json");
    let days = serde_json::from_slice::<Vec<Day>>(json_bytes).map_err(Error::from)?;
    Ok(days
        .into_iter()
        .filter(|d| is_weekday(&d.date()) && &d.date() >= since)
        .collect())
}

pub(crate) async fn json_to_disk<T, P>(path: P, value: &T) -> Result<(), Error>
where
    T: ?Sized + Serialize,
    P: AsRef<Path>,
{
    let datat = serde_json::to_string_pretty(value)?;
    let mut file = File::create(path).await?;
    file.write_all(datat.as_bytes()).await.map_err(Error::from)
}
