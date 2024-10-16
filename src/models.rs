use crate::{clockify::TimeEntry, extra_settings::schema::DayType};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub(crate) enum HolidayType {
    Vacation,
    PublicHoliday,
    Flex,
    ParentalLeave,
    Unknown,
}

#[derive(Deserialize, Debug)]
pub(crate) struct Holiday {
    pub type_: HolidayType,
    pub title: String,
    pub date: NaiveDate,
}

impl Holiday {
    pub(crate) fn new(title: String, date: NaiveDate, type_: HolidayType) -> Self {
        Self { title, date, type_ }
    }
}

#[derive(Deserialize, Debug)]
pub(crate) struct SickLeaveDay {
    title: String,
    date: NaiveDate,
}

impl SickLeaveDay {
    pub(crate) fn new(title: String, date: NaiveDate) -> Self {
        SickLeaveDay { title, date }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct WorkDay {
    pub date: NaiveDate,
    pub items: Vec<WorkItem>,
}

impl WorkDay {
    pub(crate) fn new(date: NaiveDate, items: Vec<WorkItem>) -> Self {
        WorkDay { date, items }
    }

    pub(crate) fn duration(&self) -> i64 {
        self.items.iter().map(|wi| wi.duration()).sum()
    }

    pub(crate) fn item_count(&self) -> usize {
        self.items.len()
    }
}

#[derive(Deserialize, Clone, Debug)]
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

#[derive(Deserialize, Debug)]
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

    pub(crate) fn into_date(self) -> NaiveDate {
        match self {
            Self::Holiday(d) => d.date,
            Self::Sick(d) => d.date,
            Self::Work(d) => d.date,
        }
    }

    pub(crate) fn type_(&self) -> DayType {
        match self {
            Self::Holiday(d) => match d.type_ {
                HolidayType::Flex => DayType::Flex,
                HolidayType::ParentalLeave => DayType::ParentalLeave,
                HolidayType::PublicHoliday => DayType::PublicHoliday,
                HolidayType::Vacation => DayType::Vacation,
                HolidayType::Unknown => DayType::Unknown,
            },
            Self::Sick(_) => DayType::SickLeave,
            Self::Work(_) => DayType::WorkingDay,
        }
    }
}
