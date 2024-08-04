use clap::Parser;

#[derive(Parser, Debug, Clone)]
pub(crate) struct Args {
    #[arg(long, default_value = "false")]
    pub include_today: bool,
}
