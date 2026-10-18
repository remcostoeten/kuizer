use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: &'static str,
    pub args: Vec<OsString>,
    /// Whether a non-zero exit means it failed. Explorer exits with 1 even when it opened.
    pub checks_status: bool,
}

impl Launch {
    fn new(program: &'static str, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        Self {
            program,
            args: args.into_iter().map(Into::into).collect(),
            checks_status: true,
        }
    }
}

const FILE_MANAGER: &str = "org.freedesktop.FileManager1";
const FILE_MANAGER_PATH: &str = "/org/freedesktop/FileManager1";

/// @name file_uri
/// @description A `file://` URI for an absolute path, percent-encoding every byte that is not
/// unreserved, so spaces, quotes and non-ASCII names survive D-Bus and GVariant quoting.
///
/// @example
/// assert_eq!(file_uri(Path::new("/a b")), "file:///a%20b");
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(char::from(*byte));
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// @name commands
/// @description The commands that show `path` selected in the file manager, in the order to try
/// them. Linux asks the desktop's file manager over D-Bus with `gdbus` or `dbus-send`, then falls
/// back to opening the containing folder with `xdg-open`. macOS uses `open -R` and Windows
/// `explorer /select,`.
///
/// @example
/// let tries = commands(Path::new("/p/site/node_modules"), Os::Linux);
pub fn commands(path: &Path, os: Os) -> Vec<Launch> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(path);
    match os {
        Os::Mac => vec![Launch::new("open", [OsString::from("-R"), path.into()])],
        Os::Windows => {
            let mut select = OsString::from("/select,");
            select.push(path);
            vec![Launch {
                checks_status: false,
                ..Launch::new("explorer", [select])
            }]
        }
        Os::Linux => {
            let uri = file_uri(path);
            vec![
                Launch::new(
                    "gdbus",
                    [
                        "call".to_owned(),
                        "--session".to_owned(),
                        "--timeout".to_owned(),
                        "5".to_owned(),
                        "--dest".to_owned(),
                        FILE_MANAGER.to_owned(),
                        "--object-path".to_owned(),
                        FILE_MANAGER_PATH.to_owned(),
                        "--method".to_owned(),
                        format!("{FILE_MANAGER}.ShowItems"),
                        format!("['{uri}']"),
                        String::new(),
                    ],
                ),
                Launch::new(
                    "dbus-send",
                    [
                        "--session".to_owned(),
                        "--print-reply".to_owned(),
                        "--reply-timeout=5000".to_owned(),
                        format!("--dest={FILE_MANAGER}"),
                        "--type=method_call".to_owned(),
                        FILE_MANAGER_PATH.to_owned(),
                        format!("{FILE_MANAGER}.ShowItems"),
                        format!("array:string:{uri}"),
                        "string:".to_owned(),
                    ],
                ),
                Launch::new("xdg-open", [parent]),
            ]
        }
    }
}

/// @name run
/// @description Tries each command until one works and says why the last one failed otherwise.
/// It waits for the commands, so call it off the UI thread.
///
/// @example
/// run(&commands(&path, Os::current()))?;
pub fn run(tries: &[Launch]) -> Result<(), String> {
    let mut last = "no file manager command for this system".to_owned();
    for launch in tries {
        let status = Command::new(launch.program)
            .args(&launch.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(status) if status.success() || !launch.checks_status => return Ok(()),
            Ok(status) => last = format!("{} exited with {status}", launch.program),
            Err(error) => last = format!("cannot run {}: {error}", launch.program),
        }
    }
    Err(last)
}

/// @name reveal
/// @description Shows `path` in the system file manager, see `commands`.
///
/// @example
/// reveal(Path::new("/p/site/node_modules"))?;
pub fn reveal(path: &Path) -> Result<(), String> {
    run(&commands(path, Os::current()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(launch: &Launch) -> String {
        std::iter::once(launch.program.to_owned())
            .chain(
                launch
                    .args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned()),
            )
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn uris_are_percent_encoded() {
        assert_eq!(
            file_uri(Path::new("/home/me/my app's/ü")),
            "file:///home/me/my%20app%27s/%C3%BC"
        );
    }

    #[test]
    fn linux_asks_the_file_manager_then_opens_the_folder() {
        let tries = commands(Path::new("/p/my site/node_modules"), Os::Linux);
        let lines: Vec<String> = tries.iter().map(line).collect();
        assert_eq!(
            lines,
            [
                "gdbus call --session --timeout 5 --dest org.freedesktop.FileManager1 --object-path /org/freedesktop/FileManager1 --method org.freedesktop.FileManager1.ShowItems ['file:///p/my%20site/node_modules'] ",
                "dbus-send --session --print-reply --reply-timeout=5000 --dest=org.freedesktop.FileManager1 --type=method_call /org/freedesktop/FileManager1 org.freedesktop.FileManager1.ShowItems array:string:file:///p/my%20site/node_modules string:",
                "xdg-open /p/my site",
            ]
        );
        assert!(tries.iter().all(|launch| launch.checks_status));
    }

    #[test]
    fn macos_selects_the_item_in_finder() {
        let tries = commands(Path::new("/Users/me/app/dist"), Os::Mac);
        assert_eq!(tries.len(), 1);
        assert_eq!(line(&tries[0]), "open -R /Users/me/app/dist");
    }

    #[test]
    fn windows_selects_the_item_in_explorer() {
        let tries = commands(Path::new("C:/work/app/dist"), Os::Windows);
        assert_eq!(tries.len(), 1);
        assert_eq!(line(&tries[0]), "explorer /select,C:/work/app/dist");
        assert!(!tries[0].checks_status);
    }

    #[test]
    fn a_missing_program_is_reported() {
        let missing = Launch::new("kuizer-no-such-program", ["x"]);
        let error = run(&[missing]).unwrap_err();
        assert!(error.starts_with("cannot run kuizer-no-such-program"));
    }
}
