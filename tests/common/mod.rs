//! Common test utilities shared across integration tests.

/// Initialize test tracing (call once at the beginning of tests).
///
/// This sets up tracing for tests with DEBUG level output to the test writer.
/// Subsequent calls are safe and will be ignored.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
}
