use crate::error::Error;
use hhmmss::Hhmmss;
use log::{Level, LevelFilter, Metadata, Record};
use std::time::Instant;

static LOGGER: Logger = Logger;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        let execution_starts_at = Instant::now();

        if self.enabled(record.metadata()) {
            println!(
                "{} {} {}",
                execution_starts_at.elapsed().hhmmss(),
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

/// Initialize the Logger
pub fn init() -> Result<(), Error> {
    let execution_starts_at = Instant::now();
    println!("Execution starts at {:?}", execution_starts_at);

    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .map_err(|e| Error::LoggingError(e.to_string()))?;
    Ok(())
}
