use log::Log;

struct SimpleLogger;

impl SimpleLogger {
    fn init() {
        let _ = log::set_boxed_logger(Box::new(SimpleLogger))
            .map(|_| log::set_max_level(log::LevelFilter::Debug));
    }
}

impl Log for SimpleLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        let _ = eprintln!(
            "[pw-capture-gl] {:>5} [{}:{}] [{}] {}",
            record.level(),
            record.file().unwrap_or_default(),
            record.line().unwrap_or_default(),
            record.target(),
            record.args()
        );
    }
    fn flush(&self) {}
}

pub fn init_logger() {
    SimpleLogger::init()
}

#[macro_export]
macro_rules! trace {
    (target: $target:expr, $($arg:tt)+) => (::log::log!(target: $target, ::log::Level::Trace, $($arg)+));
    ($($arg:tt)+) => (::log::log!(target: function_name!(), ::log::Level::Trace, $($arg)+));
}

#[macro_export]
macro_rules! debug {
    (target: $target:expr, $($arg:tt)+) => (::log::log!(target: $target, ::log::Level::Debug, $($arg)+));
    ($($arg:tt)+) => (::log::log!(target: function_name!(), ::log::Level::Debug, $($arg)+));
}

#[macro_export]
macro_rules! info {
    (target: $target:expr, $($arg:tt)+) => (::log::log!(target: $target, ::log::Level::Info, $($arg)+));
    ($($arg:tt)+) => (::log::log!(target: function_name!(), ::log::Level::Info, $($arg)+));
}

#[macro_export]
macro_rules! warn {
    (target: $target:expr, $($arg:tt)+) => (::log::log!(target: $target, ::log::Level::Warn, $($arg)+));
    ($($arg:tt)+) => (::log::log!(target: function_name!(), ::log::Level::Warn, $($arg)+));
}

#[macro_export]
macro_rules! error {
    (target: $target:expr, $($arg:tt)+) => (::log::log!(target: $target, ::log::Level::Error, $($arg)+));
    ($($arg:tt)+) => (::log::log!(target: function_name!(), ::log::Level::Error, $($arg)+));
}

pub use crate::warn;
pub use debug;
pub use error;
pub use info;
pub use trace;
