use anyhow::Error;
use chrono::NaiveDate;
use clap::{Args as ClapArgs, Parser};

use crate::clockify::Token;

#[derive(Parser, Debug, Clone)]
pub(crate) struct Args {
    /// Include today in calculations
    #[arg(short, long, default_value = "false")]
    pub include_today: bool,

    /// Clockify API token
    #[arg(short, long)]
    pub token: Option<Token>,

    /// Start date equal or greater than 2023-01-01 in the format YYYY-MM-DD.
    #[arg(short, long, value_parser = validate_date)]
    pub start_date: Option<NaiveDate>,

    /// Optional start balance in minutes
    #[arg(short = 'b', long, requires = "start_date")]
    pub start_balance: Option<i64>,
}

fn validate_date(s: &str) -> Result<NaiveDate, Error> {
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
    if date >= NaiveDate::from_ymd_opt(2023, 1, 1).unwrap() {
        Ok(date)
    } else {
        Err(Error::msg(
            "Input date has to be equal or greater than 2023-01-01!",
        ))
    }
}
