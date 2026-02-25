use simplelog::{ColorChoice, LevelFilter, TermLogger, TerminalMode};

pub fn init_test_logger() {
    // Ignore error, most likely already initialized by another test
    if TermLogger::init(
        LevelFilter::Debug,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .is_err()
    {
        // NOOP
    }
}
