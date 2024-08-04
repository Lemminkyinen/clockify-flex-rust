use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use lazy_static::lazy_static;
use std::mem;

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

pub(crate) fn workdays_to_sec(day_count: usize) -> i64 {
    (day_count as f32 * *WORK_DAY_HOURS * 3600f32) as i64
}

pub(crate) fn today() -> NaiveDate {
    Utc::now().date_naive()
}
