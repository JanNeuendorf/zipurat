use clap::Parser;

mod archiver;
mod cli;
mod index;
mod restore;
mod serializer;
mod utils;
fn main() {
    let result = cli::Cli::parse().run();
    result.unwrap();
}
