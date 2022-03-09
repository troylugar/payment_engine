use std::env;

use engine::Engine;
use models::TxRow;

extern crate serde;
extern crate serde_derive;

mod engine;
mod models;
mod stores;

fn main() {
    // set logger
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(fern::log_file("output.log").unwrap())
        .apply()
        .unwrap();

    // read transactions
    let filepath = env::args()
        .nth(1)
        .expect("filepath is missing from arguments");

    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(filepath)
        .expect("could not read file");

    // process transactions
    let mut engine = Engine::new();
    let result_iter = reader
        .deserialize::<TxRow>()
        .map(|x| x.expect("error reading file"))
        .map(|x| engine.process_row(&x));

    for result in result_iter {
        // log errors
        if result.is_err() {
            log::error!("{}", result.unwrap_err());
        }
    }

    // write transactions to stdout
    let mut writer = csv::WriterBuilder::new().from_writer(std::io::stdout());
    writer
        .write_record(&["client", "total", "available", "held", "locked"])
        .expect("filed to write to file");

    let account_iter = engine.get_account_iter();
    for (id, data) in account_iter {
        writer
            .write_record(&[
                id.to_string(),
                (data.available + data.held).round_dp(4).to_string(),
                data.available.round_dp(4).to_string(),
                data.held.round_dp(4).to_string(),
                engine.is_account_locked(*id).to_string(),
            ])
            .expect("failed to write to file");
    }
}
