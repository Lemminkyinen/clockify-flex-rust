mod args;
mod clockify;
mod models;
mod utils;

use crate::clockify::{ClockifyClient, TimeOffType, Token};
use crate::models::{Day, WorkItem};
use anyhow::Error;
use args::Args;
use chrono::{Datelike, NaiveDate, TimeDelta};
use clap::Parser;
use itertools::Itertools;

use models::{Holiday, HolidayType, SickLeaveDay, WorkDay};
use spinners::{Spinner, Spinners};
use std::env;
use std::time::Instant;
use tabled::builder::Builder;
use tabled::settings::themes::ColumnNames;
use tabled::settings::{Color, Style};
use tabled::Table;
use tokio::{fs, join};

async fn get_working_days(
    client: ClockifyClient,
    since: &NaiveDate,
) -> Result<Vec<WorkDay>, Error> {
    let work_items = client.get_work_items_since(since).await?;
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

async fn get_days_off(client: ClockifyClient, since: &NaiveDate) -> Result<Vec<Day>, Error> {
    let items = client.get_time_off_items().await?;
    let days_off = items
        .into_iter()
        .flat_map(|toi| {
            // TODO support users datetime
            // Use date_naive because:
            // "start": "2024-01-30T22:00:00Z",
            // "end": "2024-01-31T21:59:59.999Z"
            let start = toi.start.date_naive();
            let end = toi.end.date_naive();
            let mut days_off = Vec::new();
            for date in utils::DateRange(start + TimeDelta::days(1), end).filter(|d| d >= since) {
                let note = toi.note.clone();
                let day_off = if let TimeOffType::SickLeave = toi.type_ {
                    let day = SickLeaveDay::new(note, date);
                    Day::Sick(day)
                } else {
                    // TODO Implement HolidayType
                    let day = Holiday::new(note, date, HolidayType::Unknown);
                    Day::Holiday(day)
                };
                days_off.push(day_off);
            }
            days_off
        })
        .collect::<Vec<Day>>();
    Ok(days_off)
}

async fn get_public_holidays(since: &NaiveDate) -> Result<Vec<Day>, Error> {
    // Implement HolidayType
    let content = fs::read("holidays.json").await?;
    let days = serde_json::from_slice::<Vec<Day>>(content.as_ref()).map_err(Error::from)?;
    Ok(days.into_iter().filter(|d| &d.date() >= since).collect())
}

async fn get_items(
    client: ClockifyClient,
    since: NaiveDate,
) -> Result<(Vec<Day>, Vec<WorkDay>, Vec<Day>), Error> {
    let public_holidays = get_public_holidays(&since);
    let working_days = get_working_days(client.clone(), &since);
    let days_off = get_days_off(client, &since);
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
    longest_working_day: WorkDay,
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
        self.worked_time - utils::workdays_to_sec(self.held_flex_time_off_day_count)
    }

    fn balance_days(&self) -> i64 {
        let denominator_seconds = (*utils::WORK_DAY_HOURS * 3600.0f32) as i64;
        self.balance / denominator_seconds
    }
}

fn calculate_results(
    mut public_holidays: Vec<Day>,
    mut working_days: Vec<WorkDay>,
    mut days_off: Vec<Day>,
    include_today: bool,
    start_balance: i64,
) -> Result<Results, Error> {
    let first_working_day = working_days
        .iter()
        .min_by_key(|wd| wd.date)
        .ok_or(Error::msg("Working days is empty"))?
        .date;
    let mut all_weekdays = utils::get_all_weekdays_since(first_working_day).collect_vec();

    if !include_today {
        let today = utils::today();
        working_days.retain(|wd| wd.date < today);
        public_holidays.retain(|phd| phd.date() < today);
        days_off.retain(|do_| {
            matches!(do_, Day::Holiday(_)) || matches!(do_, Day::Sick(_)) && do_.date() < today
        });
        all_weekdays.retain(|d| d < &today)
    }

    let longest_working_day = working_days
        .iter()
        .max_by_key(|wd| wd.duration())
        .ok_or(Error::msg("Days iterator is empty!"))?
        .to_owned();

    let public_holidays = public_holidays
        .into_iter()
        .filter_map(|day| {
            let date = day.date();
            if utils::not_in_future(&date) && utils::is_weekday(&date) && first_working_day < date {
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
            .partition(utils::not_in_future);
    let held_flex_time_off_day_count = held_flex_time_off_days.len();
    let future_flex_time_off_day_count = future_flex_time_off_days.len();

    let filtered_expected_working_days = all_weekdays
        .into_iter()
        .filter(|day| !public_holidays.contains(day) && !sick_leave_days.contains(day))
        .collect_vec();
    let filtered_expected_working_day_count = filtered_expected_working_days.len();

    let expected_work_time_sec = utils::workdays_to_sec(filtered_expected_working_day_count);
    let flex_time_off_sec = utils::workdays_to_sec(held_flex_time_off_day_count);
    let total_worked_time_sec = working_days.iter().map(|wd| wd.duration()).sum::<i64>();
    let working_day_count = working_days.len();

    let start_balance = 60i64 * start_balance;
    let balance =
        start_balance + total_worked_time_sec - expected_work_time_sec - flex_time_off_sec;

    Ok(Results {
        first_working_day,
        working_day_count,
        public_holiday_count,
        filtered_expected_working_day_count,
        sick_leave_day_count,
        held_flex_time_off_day_count,
        future_flex_time_off_day_count,
        longest_working_day,
        worked_time: total_worked_time_sec,
        balance,
    })
}

fn build_table(r: Results, start_balance: Option<i64>) -> Table {
    fn add_row(builder: &mut Builder, text: &str, days: Option<usize>, seconds: Option<i64>) {
        let hours_and_minutes = if let Some(seconds) = seconds {
            Some(utils::seconds_to_hours_and_minutes(seconds))
        } else {
            days.map(|days| utils::hours_to_hours_and_minutes(days as f32 * *utils::WORK_DAY_HOURS))
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
    if let Some(start_balance) = start_balance {
        add_row(
            &mut table_builder,
            "Start balance",
            None,
            Some(start_balance * 60),
        );
    }

    let (balance_hours, balance_minutes) = utils::seconds_to_hours_and_minutes(r.balance);
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

    let args = Args::parse();

    let token = if let Some(token) = args.token {
        token
    } else {
        Token::new(&env::var("TOKEN")?)
    };

    let since_date = args
        .start_date
        .unwrap_or(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap());

    let start_balance = args.start_balance.unwrap_or(0);

    let mut spinner = Spinner::new(Spinners::Moon, "Fetching user...".into());
    let time = Instant::now();
    let client = ClockifyClient::new(token)?;
    spinner.stop_with_message(format!(
        "User fetched from Clockify API! ({:.2} s)",
        time.elapsed().as_secs_f32()
    ));

    let mut spinner = Spinner::new(Spinners::Moon, "Fetching data...".into());
    let time = Instant::now();
    let (public_holidays, working_days, days_off) = get_items(client, since_date).await?;

    spinner.stop_with_message(format!(
        "{} items fetched from Clockify API! ({:.2} s)",
        working_days.iter().map(WorkDay::item_count).sum::<usize>() + days_off.len(),
        time.elapsed().as_secs_f32()
    ));

    let mut spinner = Spinner::new(Spinners::Moon, "Calculating results...".into());
    let time = Instant::now();
    let results = calculate_results(
        public_holidays,
        working_days,
        days_off,
        args.include_today,
        start_balance,
    )?;
    spinner.stop_with_message(format!(
        "Items calculated! ({:.2} s)\n",
        time.elapsed().as_secs_f32()
    ));

    // TODO Support for first day even when the start_date is given
    let grinding_text = if args.start_date.is_none() {
        format!(
            "You have been grinding since: {:?}",
            results.first_working_day
        )
    } else {
        format!(
            "You have been grinding at least since: {:?}",
            results.first_working_day
        )
    };
    println!("{grinding_text}");

    // TODO Support for longest working day even when the start_date is given
    let longest_day = results.longest_working_day.clone();
    let (hours, minutes) = utils::seconds_to_hours_and_minutes(longest_day.duration());
    println!(
        "Your longest grind is {hours} hours, {minutes} minutes. You did it on {}, {:?}",
        longest_day.date.weekday(),
        longest_day.date
    );

    let table = build_table(results, args.start_balance);
    println!("{table}");

    Ok(())
}
