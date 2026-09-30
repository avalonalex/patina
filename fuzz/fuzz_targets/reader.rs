#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../crates/patina-frontend/tests/support/reader_checks.rs"]
mod reader_checks;

fuzz_target!(|data: &[u8]| {
    // Parser/Reader accept &str. Invalid UTF-8 has no representation at this
    // boundary; port byte-decoding has its own tests in patina-core.
    if let Ok(text) = std::str::from_utf8(data) {
        // Mutations affect both syntax and the varying character boundaries.
        reader_checks::check_reader(text, data);
        reader_checks::check_reader(text, &[0]);
    }
});
