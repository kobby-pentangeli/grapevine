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
        let execution_starts_at = Some(Instant::now());
        let elapsed = Instant::now()
            .duration_since(execution_starts_at.expect("Failed to fetch elapsed time"));

        if self.enabled(record.metadata()) {
            println!("{} {} {}", elapsed.hhmmss(), record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

/// Initialize the Logger
pub fn init() {
    let execution_starts_at = Some(Instant::now());
    println!("Execution starts at {:?}", execution_starts_at);

    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .expect("Failed to log");
}
