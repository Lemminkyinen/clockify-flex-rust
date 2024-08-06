use clap::Parser;

use crate::clockify::Token;

#[derive(Parser, Debug, Clone)]
pub(crate) struct Args {
    #[arg(long, default_value = "false")]
    pub include_today: bool,
    #[arg(short, long)]
    pub token: Option<Token>,
}
