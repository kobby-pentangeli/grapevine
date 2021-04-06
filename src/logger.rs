use hhmmss::Hhmmss;
use log::{Level, LevelFilter, Metadata, Record};
use std::time::Instant;

static mut PROGRAM_STARTED_AT: Option<Instant> = None;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        let elapsed = unsafe { Instant::now().duration_since(PROGRAM_STARTED_AT.unwrap()) };

        println!("{} {:?}", elapsed.hhmmss(), record.args());
    }

    fn flush(&self) {}
}

pub fn init() {
    unsafe {
        PROGRAM_STARTED_AT = Some(Instant::now());
    }

    // static logger: *const SimpleLogger = logger;
    log::set_logger(&Logger)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .unwrap();
}
