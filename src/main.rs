mod clockify;
mod models;

use crate::clockify::{ClockifyClient, TimeOffType, Token};
use crate::models::{Day, WorkItem};
use anyhow::Error;
use chrono::{Datelike, Duration, NaiveDate, TimeDelta, Utc, Weekday};
use itertools::Itertools;
use models::{Holiday, HolidayType, SickLeaveDay, WorkDay};
use std::{env, mem};
use tokio::{fs, join};

async fn get_working_days(client: ClockifyClient) -> Result<Vec<WorkDay>, Error> {
    let since_date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
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

fn get_all_weekdays_since(date: NaiveDate) -> Vec<NaiveDate> {
    let yesterday = Utc::now().date_naive() - TimeDelta::days(1);
    DateRange(date, yesterday)
        .filter(|date| {
            [
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ]
            .contains(&date.weekday())
        })
        .collect_vec()
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
    let token = Token::new(&env::var("TOKEN")?);
    let client = ClockifyClient::new(token)?;

    let public_holidays = get_public_holidays();
    let working_days = get_working_days(client.clone());
    let days_off = get_days_off(client);

    let (public_holidays, working_days, days_off) = join!(public_holidays, working_days, days_off);

    let public_holidays = public_holidays?
        .into_iter()
        .filter_map(|day| {
            let date = day.date();
            if Utc::now().date_naive() > date {
                Some(date)
            } else {
                None
            }
        })
        .collect_vec();
    let working_days = working_days?
        .into_iter()
        .filter(|day| day.date < Utc::now().date_naive())
        .collect_vec();
    let days_off = days_off?
        .into_iter()
        .filter_map(|day| {
            let date = day.date();
            if Utc::now().date_naive() > date {
                Some(date)
            } else {
                None
            }
        })
        .collect_vec();

    let day_work_time = 7.5f32;

    let first_working_day = working_days
        .iter()
        .min_by_key(|wd| wd.date)
        .ok_or(Error::msg("Iterator is empty"))?;
    let filtered_working_days = get_all_weekdays_since(first_working_day.date)
        .into_iter()
        .filter(|day| !public_holidays.contains(day) && !days_off.contains(day))
        .collect_vec();
    let expected_work_time = filtered_working_days.len() as f32 * day_work_time;
    let total_worked_time = working_days.iter().map(|wd| wd.duration()).sum::<i64>();
    let balance = total_worked_time - ((expected_work_time * 3600f32) as i64);

    fn hours_to_hours_and_minutes(hours: f32) -> (i32, i32) {
        let whole_hours = hours.trunc() as i32;
        let minutes = ((hours - whole_hours as f32) * 60.0).round() as i32;
        (whole_hours, minutes)
    }

    let (expected_hours, expected_minutes) = hours_to_hours_and_minutes(
        get_all_weekdays_since(first_working_day.date).len() as f32 * day_work_time,
    );

    println!(
        "Expected working time: {} hours, {} minutes.",
        expected_hours, expected_minutes
    );

    let (expected_hours, expected_minutes) =
        hours_to_hours_and_minutes(filtered_working_days.len() as f32 * day_work_time);

    println!(
        "Expected working time (after filtering sick leaves, public holidays and flex hours): {} hours, {} minutes.", expected_hours, expected_minutes
    );
    // println!("days_off: {}", days_off.len() as f32 * day_work_time);

    let total_hours = total_worked_time / 3600;
    let total_minutes = (total_worked_time % 3600) / 60i64;
    println!(
        "Worked time: {} hours, {} minutes.",
        total_hours, total_minutes
    );

    let hours = balance / 3600;
    let minutes = (balance % 3600) / 60;
    println!("Work time balance: {hours} hours, {minutes} minutes.");

    Ok(())
}
