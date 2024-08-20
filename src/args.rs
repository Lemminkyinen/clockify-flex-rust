use anyhow::Error;
use chrono::{NaiveDate, Utc};
use clap::Parser;

use crate::clockify::Token;

#[derive(Parser, Debug, Clone)]
pub(crate) struct Args {
    /// Include today in calculations
    #[arg(short, long, default_value = "false")]
    pub include_today: bool,

    /// Clockify API token
    #[arg(short, long)]
    pub token: Option<Token>,

    /// Start date equal or greater than 2022-01-01 in the format YYYY-MM-DD.
    #[arg(short, long, value_parser = validate_date)]
    pub start_date: Option<NaiveDate>,

    /// Optional start balance in minutes
    #[arg(short = 'b', long, requires = "start_date")]
    pub start_balance: Option<i64>,
}

fn validate_date(s: &str) -> Result<NaiveDate, Error> {
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
    let date2022 = NaiveDate::from_ymd_opt(2022, 1, 1).unwrap();
    let today = Utc::now().date_naive();

    let err_msg = if date > today {
        "Input cannot be greater than today!"
    } else if date < date2022 {
        "Input has to be equal or greater than 2022-01-01!"
    } else {
        return Ok(date);
    };

    Err(Error::msg(err_msg))
}

impl Args {
    pub(crate) fn validate(&self) -> Result<(), clap::Error> {
        let today = Utc::now().date_naive();
        if self.start_date == Some(today) && !self.include_today {
            println!("If start_date is today, --include-today option must be used.");
            std::process::exit(1);
        }
        Ok(())
    }
}
