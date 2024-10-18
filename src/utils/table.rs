use crate::{utils, Results};
use tabled::builder::Builder;
use tabled::settings::themes::ColumnNames;
use tabled::settings::{Color, Style};
use tabled::Table;

pub(crate) fn build_table(r: Results, start_balance: Option<i64>) -> Table {
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

    let items = [
        (
            "Public holidays (on weekdays)",
            Some(r.public_holiday_count),
            None,
        ),
        (
            "Held parental leave weekdays",
            Some(r.parental_leave_day_count),
            None,
        ),
        (
            "Held vacation weekdays",
            Some(r.held_vacation_day_count),
            None,
        ),
        (
            "Future vacation weekdays",
            Some(r.future_vacation_day_count),
            None,
        ),
        (
            "Held flex time off",
            Some(r.held_flex_time_off_day_count),
            None,
        ),
        (
            "Future flex time off",
            Some(r.future_flex_time_off_day_count),
            None,
        ),
        ("Sick leave time", Some(r.sick_leave_day_count), None),
        (
            "Expected working time (sick leaves & public holidays deducted)",
            Some(r.filtered_expected_working_day_count),
            Some(r.expected_working_time_sec),
        ),
        (
            "Total working time",
            Some(r.working_day_count),
            Some(r.worked_time),
        ),
    ];

    for item in items {
        add_row(&mut table_builder, item.0, item.1, item.2);
    }

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
