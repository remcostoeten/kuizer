#![no_main]

use libfuzzer_sys::fuzz_target;

use kuizer::config::{self, Config, DeleteConfig};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    let (config, _warnings) = Config::parse_lenient(text);
    match config::fix_text(text) {
        Err(_) => {
            // Broken TOML syntax must fall back to the cautious delete config.
            assert_eq!(config.delete, DeleteConfig::cautious());
        }
        Ok(report) => {
            let (_fixed_config, _) = Config::parse_lenient(&report.text);
            if let Ok(second) = config::fix_text(&report.text) {
                assert_eq!(
                    second.text, report.text,
                    "config::fix_text must be idempotent"
                );
            }
        }
    }
});
