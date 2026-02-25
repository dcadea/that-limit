use simplelog::{ColorChoice, LevelFilter, TermLogger, TerminalMode};

pub fn init_test_logger() {
    if TermLogger::init(
        LevelFilter::Debug,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .is_err()
    {
        // Ignore error, most likely already initialized by another test
    }
}
