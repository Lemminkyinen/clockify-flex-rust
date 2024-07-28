use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use crate::clockify::TimeEntry;

#[derive(Deserialize)]
pub(crate) enum HolidayType {
    Vacation,
    PublicHoliday,
    Flex,
    Unknown,
}

#[derive(Deserialize)]
pub(crate) struct Holiday {
    type_: HolidayType,
    title: String,
    date: NaiveDate,
}

impl Holiday {
    pub(crate) fn new(title: String, date: NaiveDate, type_: HolidayType) -> Self {
        Self { title, date, type_ }
    }
}

#[derive(Deserialize)]
pub(crate) struct SickLeaveDay {
    title: String,
    date: NaiveDate,
}

impl SickLeaveDay {
    pub(crate) fn new(title: String, date: NaiveDate) -> Self {
        SickLeaveDay { title, date }
    }
}

#[derive(Deserialize)]
pub(crate) struct WorkDay {
    pub date: NaiveDate,
    items: Vec<WorkItem>,
}

impl WorkDay {
    pub(crate) fn new(date: NaiveDate, items: Vec<WorkItem>) -> Self {
        WorkDay { date, items }
    }

    pub(crate) fn duration(&self) -> i64 {
        self.items.iter().map(|wi| wi.duration()).sum()
    }
}

#[derive(Deserialize)]
pub(crate) struct WorkItem {
    description: String,
    project: String,
    start: DateTime<Utc>,
    stop: DateTime<Utc>,
}

impl From<TimeEntry> for WorkItem {
    fn from(value: TimeEntry) -> Self {
        WorkItem {
            description: value.description,
            project: value.project_name,
            start: value.start,
            stop: value.end,
        }
    }
}

impl WorkItem {
    fn duration(&self) -> i64 {
        (self.stop - self.start).num_seconds()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Day {
    Holiday(Holiday),
    Sick(SickLeaveDay),
    Work(WorkDay),
}

impl Day {
    pub(crate) fn date(&self) -> NaiveDate {
        match self {
            Self::Holiday(d) => d.date,
            Self::Sick(d) => d.date,
            Self::Work(d) => d.date,
        }
    }
}
