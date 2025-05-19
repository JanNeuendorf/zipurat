use clap::Parser;
use colored::*;

mod archiver;
mod cli;
mod index;
mod restore;
mod serializer;
mod utils;
fn main() {
    let result = cli::Cli::parse().run();
    if let Err(e) = result {
        eprintln!("Error: {}", format!("{}", e).red());
    }
}
