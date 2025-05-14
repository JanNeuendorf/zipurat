use std::path::Path;

use clap::Parser;

mod archiver;
mod cli;
mod index;
mod utils;
fn main() {
    let result = cli::Cli::parse().run();
    result.unwrap();
}
