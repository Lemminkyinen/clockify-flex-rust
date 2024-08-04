mod clockify;
mod models;

use crate::clockify::{ClockifyClient, TimeOffType, Token};
use crate::models::{Day, WorkItem};
use anyhow::Error;
use chrono::{Datelike, Duration, NaiveDate, TimeDelta, Utc, Weekday};
use itertools::Itertools;
use lazy_static::lazy_static;
use models::{Holiday, HolidayType, SickLeaveDay, WorkDay};
use spinners::{Spinner, Spinners};
use std::time::Instant;
use std::{env, mem};
use tabled::builder::Builder;
use tabled::settings::themes::ColumnNames;
use tabled::settings::{Color, Style};
use tabled::Table;
use tokio::{fs, join};

lazy_static! {
    static ref WORK_DAY_HOURS: f32 = 7.5;
}

async fn get_working_days(client: ClockifyClient) -> Result<Vec<WorkDay>, Error> {
    let since_date = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
    let work_items = client.get_work_items_since(since_date).await?;
    let work_days = work_items
        .into_iter()
        .chunk_by(|wi| wi.start.date_naive())
        .into_iter()
        .map(|(date, group)| {
            let work_items = group.map(WorkItem::from).collect();
            WorkDay::new(date, work_items)
        })
        .collect::<Vec<WorkDay>>();

    Ok(work_days)
}

struct DateRange(NaiveDate, NaiveDate);

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

async fn get_days_off(client: ClockifyClient) -> Result<Vec<Day>, Error> {
    let items = client.get_time_off_items().await?;
    let days_off = items
        .into_iter()
        .flat_map(|toi| {
            // Use date_naive because:
            // "start": "2024-01-30T22:00:00Z",
            // "end": "2024-01-31T21:59:59.999Z"
            let start = toi.start.date_naive();
            let end = toi.end.date_naive();
            let duration = (end - start).num_days();
            let mut days_off = Vec::new();
            for date in DateRange(start + TimeDelta::days(1), end) {
                let note = toi.note.clone();
                let day_off = if let TimeOffType::SickLeave = toi.type_ {
                    let day = SickLeaveDay::new(note, date);
                    Day::Sick(day)
                } else {
                    // Implement HolidayType
                    let day = Holiday::new(note, date, HolidayType::Unknown);
                    Day::Holiday(day)
                };
                days_off.push(day_off);
            }
            assert!(days_off.len() == duration as usize, "something wrong");
            days_off
        })
        .collect::<Vec<Day>>();
    Ok(days_off)
}

async fn get_public_holidays() -> Result<Vec<Day>, Error> {
    // Implement HolidayType
    let content = fs::read("holidays.json").await?;
    serde_json::from_slice(content.as_ref()).map_err(Error::from)
}

fn not_in_future(date: &NaiveDate) -> bool {
    &Utc::now().date_naive() >= date
}

fn hours_to_hours_and_minutes(hours: f32) -> (i64, i64) {
    let whole_hours = hours.trunc() as i64;
    let minutes = ((hours - whole_hours as f32) * 60.0).round() as i64;
    (whole_hours, minutes.abs())
}

fn seconds_to_hours_and_minutes(seconds: i64) -> (i64, i64) {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    (hours, minutes.abs())
}

fn is_weekday(date: &NaiveDate) -> bool {
    [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
    ]
    .contains(&date.weekday())
}

fn get_all_weekdays_since(date: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    let today = Utc::now().date_naive();
    DateRange(date, today).filter(is_weekday)
}

fn workdays_to_sec(day_count: usize) -> i64 {
    (day_count as f32 * *WORK_DAY_HOURS * 3600f32) as i64
}

async fn get_items(client: ClockifyClient) -> Result<(Vec<Day>, Vec<WorkDay>, Vec<Day>), Error> {
    let public_holidays = get_public_holidays();
    let working_days = get_working_days(client.clone());
    let days_off = get_days_off(client);
    let (public_holidays, working_days, days_off) = join!(public_holidays, working_days, days_off);
    Ok((public_holidays?, working_days?, days_off?))
}

struct Results {
    first_working_day: NaiveDate,
    working_day_count: usize,
    worked_time: i64,
    filtered_expected_working_day_count: usize,
    public_holiday_count: usize,
    sick_leave_day_count: usize,
    held_flex_time_off_day_count: usize,
    future_flex_time_off_day_count: usize,
    balance: i64,
}

impl Results {
    fn total_flex_time_off_day_count(&self) -> usize {
        self.held_flex_time_off_day_count + self.future_flex_time_off_day_count
    }

    fn unfiltered_expected_working_day_count(&self) -> usize {
        self.filtered_expected_working_day_count
            + self.public_holiday_count
            + self.sick_leave_day_count
    }

    fn total_weekdays_since_start(&self) -> usize {
        self.public_holiday_count + self.sick_leave_day_count + self.working_day_count
    }

    fn weekdays_sick_leaves_filtered_since_start(&self) -> usize {
        self.working_day_count + self.public_holiday_count
    }

    fn weekdays_public_holidays_filtered_since_start(&self) -> usize {
        self.working_day_count + self.sick_leave_day_count
    }

    fn filtered_worked_time(&self) -> i64 {
        self.worked_time - workdays_to_sec(self.held_flex_time_off_day_count)
    }

    fn balance_days(&self) -> i64 {
        let denominator_seconds = (*WORK_DAY_HOURS * 3600.0f32) as i64;
        self.balance / denominator_seconds
    }
}

fn calculate_results(
    public_holidays: Vec<Day>,
    working_days: Vec<WorkDay>,
    days_off: Vec<Day>,
) -> Result<Results, Error> {
    let first_working_day = working_days
        .iter()
        .min_by_key(|wd| wd.date)
        .ok_or(Error::msg("Iterator is empty"))?
        .date;

    let public_holidays = public_holidays
        .into_iter()
        .filter_map(|day| {
            let date = day.date();
            if not_in_future(&date) && is_weekday(&date) && first_working_day < date {
                Some(date)
            } else {
                None
            }
        })
        .collect_vec();
    let public_holiday_count = public_holidays.len();

    let (sick_leave_days, flex_time_off_days): (Vec<Day>, Vec<Day>) = days_off
        .into_iter()
        .partition(|day| matches!(day, Day::Sick(_)));
    let sick_leave_days = sick_leave_days
        .into_iter()
        .map(Day::into_date)
        .collect_vec();
    let sick_leave_day_count = sick_leave_days.len();
    let (held_flex_time_off_days, future_flex_time_off_days): (Vec<NaiveDate>, Vec<NaiveDate>) =
        flex_time_off_days
            .into_iter()
            .map(Day::into_date)
            .partition(not_in_future);
    let held_flex_time_off_day_count = held_flex_time_off_days.len();
    let future_flex_time_off_day_count = future_flex_time_off_days.len();

    let all_weekdays = get_all_weekdays_since(first_working_day).collect_vec();

    let filtered_expected_working_days = all_weekdays
        .into_iter()
        .filter(|day| !public_holidays.contains(day) && !sick_leave_days.contains(day))
        .collect_vec();
    let filtered_expected_working_day_count = filtered_expected_working_days.len();

    let expected_work_time_sec = workdays_to_sec(filtered_expected_working_day_count);
    let flex_time_off_sec = workdays_to_sec(held_flex_time_off_day_count);
    let total_worked_time_sec = working_days.iter().map(|wd| wd.duration()).sum::<i64>();
    let working_day_count = working_days.len();

    let balance = total_worked_time_sec - expected_work_time_sec - flex_time_off_sec;

    Ok(Results {
        first_working_day,
        working_day_count,
        public_holiday_count,
        filtered_expected_working_day_count,
        sick_leave_day_count,
        held_flex_time_off_day_count,
        future_flex_time_off_day_count,
        worked_time: total_worked_time_sec,
        balance,
    })
}

fn build_table(r: Results) -> Table {
    fn add_row(builder: &mut Builder, text: &str, days: Option<usize>, seconds: Option<i64>) {
        let hours_and_minutes = if let Some(seconds) = seconds {
            Some(seconds_to_hours_and_minutes(seconds))
        } else {
            days.map(|days| hours_to_hours_and_minutes(days as f32 * *WORK_DAY_HOURS))
        };

        let hours_and_minutes_str = if let Some((hours, minutes)) = hours_and_minutes {
            let minutes = if minutes != 0 {
                format!(", {} minutes", minutes)
            } else {
                "".into()
            };
            &format!("{} hours{}", hours, minutes)
        } else {
            ""
        };

        let days_str = if let Some(days) = days {
            &days.to_string()
        } else {
            ""
        };

        builder.push_record([text, days_str, hours_and_minutes_str])
    }

    let mut table_builder = Builder::default();

    table_builder.push_record(["Item", "Days", "Hours & minutes"]);
    add_row(
        &mut table_builder,
        "Public holidays (on weekdays)",
        Some(r.public_holiday_count),
        None,
    );
    add_row(
        &mut table_builder,
        "Held flex time off",
        Some(r.held_flex_time_off_day_count),
        None,
    );
    add_row(
        &mut table_builder,
        "Future flex time off",
        Some(r.future_flex_time_off_day_count),
        None,
    );
    add_row(
        &mut table_builder,
        "Sick leave time",
        Some(r.sick_leave_day_count),
        None,
    );
    add_row(
        &mut table_builder,
        "Expected working time (sick leaves & public holidays deducted)",
        Some(r.filtered_expected_working_day_count),
        None,
    );
    add_row(
        &mut table_builder,
        "Total working time",
        Some(r.working_day_count),
        Some(r.worked_time),
    );
    add_row(
        &mut table_builder,
        "Total working time (held flex hours deducted)",
        Some(r.working_day_count),
        Some(r.filtered_worked_time()),
    );

    let (balance_hours, balance_minutes) = seconds_to_hours_and_minutes(r.balance);
    table_builder.push_record([
        "Work time balance",
        &format!("{}+", r.balance_days()),
        &format!("{balance_hours} hours, {balance_minutes} minutes"),
    ]);

    let mut table = table_builder.build();
    table
        .with(Style::modern_rounded())
        .with(ColumnNames::default().color(Color::FG_GREEN));
    table
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
    let token = Token::new(&env::var("TOKEN")?);
    let client = ClockifyClient::new(token)?;

    let mut spinner = Spinner::new(Spinners::Moon, "Fetching data...".into());
    let time = Instant::now();
    let (public_holidays, working_days, days_off) = get_items(client).await?;

    spinner.stop_with_message(format!(
        "{} items fetched from Clockify API! ({:.2} s)",
        working_days.iter().map(WorkDay::item_count).sum::<usize>() + days_off.len(),
        time.elapsed().as_secs_f32()
    ));

    let mut spinner = Spinner::new(Spinners::Moon, "Calculating results...".into());
    let time = Instant::now();
    let results = calculate_results(public_holidays, working_days, days_off)?;
    spinner.stop_with_message(format!(
        "Items calculated! ({:.2} s)\n",
        time.elapsed().as_secs_f32()
    ));

    println!(
        "You have been grinding since: {:?}",
        results.first_working_day
    );

    let table = build_table(results);
    println!("{table}");

    Ok(())
}
