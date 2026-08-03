use clap::Parser;

#[derive(Parser)]
#[command(name = "dpawns")]
#[command(version = "0.1.0")]
#[command(author = "mustafaelrasheid")]
#[command(
	about = "An init system meant to be simple and reliable.",
	long_about = None
)]
pub struct Cli {}
