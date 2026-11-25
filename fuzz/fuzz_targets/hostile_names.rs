#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;

use kuizer::platform;

fuzz_target!(|data: &[u8]| {
    #[cfg(unix)]
    let path = {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        Path::new(OsStr::from_bytes(data))
    };
    #[cfg(not(unix))]
    let lossy = String::from_utf8_lossy(data);
    #[cfg(not(unix))]
    let path = Path::new(lossy.as_ref());

    let shown = platform::display_path(path);
    assert!(
        !shown.chars().any(char::is_control),
        "display_path leaked a control character: {shown:?}"
    );

    let text = String::from_utf8_lossy(data);
    let sanitized = platform::sanitize_text(&text);
    assert!(
        !sanitized.chars().any(char::is_control),
        "sanitize_text leaked a control character: {sanitized:?}"
    );
});
