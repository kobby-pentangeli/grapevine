use hhmmss::Hhmmss;
use log::{Level, LevelFilter, Metadata, Record};
use std::time::Instant;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        let execution_starts_at: Option<Instant> = None;
        let elapsed = Instant::now().duration_since(execution_starts_at.expect("Failed to fetch elapsed time"));

        println!("{} {:?}", elapsed.hhmmss(), record.args());
    }

    fn flush(&self) {}
}

/// Initialize the Logger
pub fn init() {
    let execution_starts_at = Some(Instant::now());
    println!("Execution starts at {:?}", execution_starts_at);
    
    log::set_logger(&Logger)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .expect("Failed to log");
}
