mod args;
mod clockify;
mod extra_settings;
mod models;
mod utils;

use anyhow::Error;
use args::get_settings;
use chrono::{Datelike, NaiveDate};
use clockify::{get_days_off, get_working_days};
use clockify::{ClockifyClient, Token};
use extra_settings::schema::ExtraSettings;
use extra_settings::GlobalSettings;
use itertools::Itertools;
use models::Day;
use models::{HolidayType, WorkDay};
use spinners::{Spinner, Spinners};
use std::env;
use std::time::Instant;
use tokio::join;
use utils::cache::{get_cache_first_date, set_cache_first_date};
use utils::table::build_table;
use utils::{get_public_holidays, setup_log};

async fn get_items(
    client: ClockifyClient,
    since: NaiveDate,
) -> Result<(Vec<Day>, Vec<WorkDay>, Vec<Day>), Error> {
    let public_holidays = get_public_holidays(&since);
    let working_days = get_working_days(client.clone(), &since);
    let days_off = get_days_off(client, &since);
    let (public_holidays, working_days, days_off) = join!(public_holidays, working_days, days_off);
    Ok((
        public_holidays.map_err(|e| Error::msg(format!("Failed to get public holidays: {e:?}")))?,
        working_days.map_err(|e| Error::msg(format!("Failed to get working days: {e:?}")))?,
        days_off.map_err(|e| Error::msg(format!("Failed to get fays off: {e:?}")))?,
    ))
}

struct Results {
    first_working_day: NaiveDate,
    working_day_count: usize,
    worked_time: i64,
    parental_leave_day_count: usize,
    held_vacation_day_count: usize,
    future_vacation_day_count: usize,
    filtered_expected_working_day_count: usize,
    public_holiday_count: usize,
    sick_leave_day_count: usize,
    held_flex_time_off_day_count: usize,
    future_flex_time_off_day_count: usize,
    longest_working_day: WorkDay,
    expected_working_time_sec: i64,
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
    user_settings: ExtraSettings,
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

    let public_holidays_filtered = public_holidays
        .into_iter()
        .filter_map(|day| {
            let date = day.date();
            if utils::not_in_future(&date)
                && utils::is_weekday(&date)
                && first_working_day < date
                && !user_settings.is_ignored(&day)
            {
                Some(date)
            } else {
                None
            }
        })
        .collect_vec();

    let public_holiday_count = public_holidays_filtered.len();

    let (sick_leave_days, time_off_days): (Vec<Day>, Vec<Day>) = days_off
        .into_iter()
        .partition(|day| matches!(day, Day::Sick(_)));
    let sick_leave_days = sick_leave_days
        .into_iter()
        .map(Day::into_date)
        .collect_vec();
    let sick_leave_day_count = sick_leave_days.len();

    let (parental_leave_days, time_off_days): (Vec<Day>, Vec<Day>) =
        time_off_days.into_iter().partition(|day| match day {
            Day::Holiday(hd) => matches!(hd.type_, HolidayType::ParentalLeave),
            _ => false,
        });
    let parental_leave_days = parental_leave_days
        .into_iter()
        .filter_map(|d| {
            if !utils::is_weekday(&d.date()) || user_settings.is_ignored(&d) {
                return None;
            }
            Some(Day::into_date(d))
        })
        .collect_vec();
    let parental_leave_day_count = parental_leave_days.len();

    let (vacation_days, time_off_days): (Vec<Day>, Vec<Day>) =
        time_off_days.into_iter().partition(|day| match day {
            Day::Holiday(hd) => matches!(hd.type_, HolidayType::Vacation),
            _ => false,
        });
    let vacation_days = vacation_days
        .into_iter()
        .filter_map(|d| {
            if !utils::is_weekday(&d.date()) || user_settings.is_ignored(&d) {
                return None;
            }
            Some(Day::into_date(d))
        })
        .collect_vec();
    let (held_vacation_days, future_vacation_days): (Vec<NaiveDate>, Vec<NaiveDate>) =
        vacation_days
            .into_iter()
            .partition(|day| day < &utils::today() || (include_today && day == &utils::today()));
    let held_vacation_day_count = held_vacation_days.len();
    let future_vacation_day_count = future_vacation_days.len();

    let (held_flex_time_off_days, future_flex_time_off_days): (Vec<NaiveDate>, Vec<NaiveDate>) =
        time_off_days
            .into_iter()
            .filter_map(|d| {
                if !utils::is_weekday(&d.date()) || user_settings.is_ignored(&d) {
                    return None;
                }
                Some(Day::into_date(d))
            })
            .partition(utils::not_in_future);
    let held_flex_time_off_day_count = held_flex_time_off_days.len();
    let future_flex_time_off_day_count = future_flex_time_off_days.len();

    let filtered_expected_working_days = all_weekdays
        .into_iter()
        .filter(|day| {
            !public_holidays_filtered.contains(day)
                && !sick_leave_days.contains(day)
                && !held_vacation_days.contains(day)
                && !parental_leave_days.contains(day)
        })
        .collect_vec();

    let filtered_expected_working_day_count = filtered_expected_working_days.len();
    let expected_working_time_sec =
        utils::workdays_to_secs(filtered_expected_working_days, &Some(user_settings));
    let total_worked_time_sec = working_days.iter().map(|wd| wd.duration()).sum::<i64>();
    let working_day_count = working_days.len();

    let start_balance = 60i64 * start_balance;
    let balance = start_balance + total_worked_time_sec - expected_working_time_sec;

    Ok(Results {
        first_working_day,
        working_day_count,
        public_holiday_count,
        parental_leave_day_count,
        held_vacation_day_count,
        future_vacation_day_count,
        filtered_expected_working_day_count,
        sick_leave_day_count,
        held_flex_time_off_day_count,
        expected_working_time_sec,
        future_flex_time_off_day_count,
        longest_working_day,
        worked_time: total_worked_time_sec,
        balance,
    })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();

    let args = get_settings().await;
    setup_log(&args.log_output, &args.log_level)?;

    let token = if let Some(token) = &args.token {
        token
    } else if let Ok(token) = &env::var("TOKEN") {
        &Token::new(token)
    } else {
        return Err(Error::msg("Clockify API token is missing! Please add your token to the .env file as 'TOKEN=your_token_here' or pass it using the -t argument."));
    };

    let extra_settings = GlobalSettings::create_settings().await?;

    let cache_date = get_cache_first_date(token)?;
    let since_date = args
        .start_date
        .unwrap_or(cache_date.unwrap_or(NaiveDate::from_ymd_opt(2022, 1, 1).unwrap()));

    let start_balance = args.start_balance.unwrap_or(0);

    let mut spinner = Spinner::new(Spinners::Moon, "Fetching user...".into());
    let time = Instant::now();
    let client = ClockifyClient::new(token)?;

    // Set empty options if not found.
    let user_settings = extra_settings
        .get_user_settings(&client.user.email)
        .unwrap_or(ExtraSettings::empty());
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
        user_settings,
    )?;
    spinner.stop_with_message(format!(
        "Items calculated! ({:.2} s)\n",
        time.elapsed().as_secs_f32()
    ));

    // Save first day cache, if start_date was not given
    if args.start_date.is_none() {
        set_cache_first_date(token, &results.first_working_day)?;
    }

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
