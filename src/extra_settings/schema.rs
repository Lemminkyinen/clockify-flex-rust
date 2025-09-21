use crate::models::Day;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) enum DayType {
    WorkingDay,
    SickLeave,
    ParentalLeave,
    PublicHoliday,
    Vacation,
    Flex,
    SelfImprovement,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IgnoreItem {
    name: String,
    description: String,
    date_start: NaiveDate,
    date_end: NaiveDate,
    #[serde(rename = "type")]
    type_: DayType,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExpectedWorkingHours {
    name: String,
    description: String,
    date_start: NaiveDate,
    date_end: NaiveDate,
    hours_per_day: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtraSettings {
    pub email: String,
    ignore_items: Vec<IgnoreItem>,
    expected_working_hours: Vec<ExpectedWorkingHours>,
}

impl ExtraSettings {
    pub(crate) fn empty() -> Self {
        Self {
            email: String::with_capacity(0),
            ignore_items: Vec::with_capacity(0),
            expected_working_hours: Vec::with_capacity(0),
        }
    }

    pub(crate) fn is_ignored(&self, day: &Day) -> bool {
        let ignored = self.ignore_items.iter().any(|item| {
            item.date_start <= day.date()
                && item.date_end >= day.date()
                && day.type_() == item.type_
        });
        if ignored {
            log::info!("Ignore day: {:?}", day)
        }
        ignored
    }

    /// Return expected working seconds, if expectedWorkingHours is preset for the day
    pub(crate) fn expected_working_secs(&self, day: &NaiveDate) -> Option<i64> {
        if let Some(found) = self
            .expected_working_hours
            .iter()
            .find(|i| i.date_end >= *day && i.date_start <= *day)
        {
            return Some((found.hours_per_day * 3600f32) as i64);
        }

        None
    }
}
