use std::io;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use super::Object;
use crate::delete::DeleteError;
use crate::process;
use crate::units::ByteSize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Failure {
    Missing,
    TimedOut,
    Failed(String),
}

pub(super) fn run(program: &str, args: &[&str], timeout: Duration) -> Result<String, Failure> {
    let output = process::run(Command::new(program).args(args), timeout).map_err(|error| {
        match error.kind() {
            io::ErrorKind::NotFound => Failure::Missing,
            io::ErrorKind::TimedOut => Failure::TimedOut,
            _ => Failure::Failed(error.to_string()),
        }
    })?;
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|error| Failure::Failed(error.to_string()))
    } else {
        Err(Failure::Failed(
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .unwrap_or("no output")
                .to_owned(),
        ))
    }
}

pub(super) fn engine_name(program: &str) -> &'static str {
    let file = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    if file.contains("podman") {
        "podman"
    } else {
        "docker"
    }
}

pub(super) fn size(text: &str) -> u64 {
    let first = text.split(" (").next().unwrap_or(text).trim();
    first.parse::<ByteSize>().map_or(0, |size| size.0)
}

pub(super) fn short(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

/// @name hours_ago
/// @description Reads how long ago a container stopped from the status Docker and Podman print,
/// such as `Exited (0) 2 hours ago` or `Exited (1) About a minute ago`. Returns `None` when the
/// status names no time, as for a container that was created but never started.
///
/// @example
/// assert_eq!(docker::hours_ago("Exited (0) 3 days ago"), Some(72));
pub fn hours_ago(status: &str) -> Option<u64> {
    let lowered = status.to_ascii_lowercase();
    let before = lowered.strip_suffix(" ago")?;
    let words: Vec<&str> = before.split_whitespace().collect();
    let unit = words.last()?.trim_end_matches('s');
    let count = match words.get(words.len().checked_sub(2)?)? {
        &"a" | &"an" | &"one" => 1,
        number => number.parse::<u64>().ok()?,
    };
    let hours = match unit {
        "second" | "minute" => 0,
        "hour" => count,
        "day" => count * 24,
        "week" => count * 24 * 7,
        "month" => count * 24 * 30,
        "year" => count * 24 * 365,
        _ => return None,
    };
    Some(hours)
}

pub(super) fn fields(line: &str) -> Vec<&str> {
    line.split('\t').map(str::trim).collect()
}

pub(super) fn describe(engine: &str, failure: &Failure) -> String {
    match failure {
        Failure::Missing => format!("{engine} is not installed"),
        Failure::TimedOut => format!("{engine} did not answer in time"),
        Failure::Failed(message) => format!("{engine} is not running: {message}"),
    }
}

pub(super) fn joined<T>(
    handle: thread::ScopedJoinHandle<'_, Result<T, Failure>>,
) -> Result<T, Failure> {
    handle.join().unwrap_or_else(|_| {
        Err(Failure::Failed(
            "the engine call stopped unexpectedly".to_owned(),
        ))
    })
}

pub(super) fn parse_path(path: &Path) -> Option<(&str, Object, &str)> {
    let text = path.to_str()?;
    let (program, rest) = text.split_once(':')?;
    let (object, id) = rest.split_once('/')?;
    let object = Object::from_name(object)?;
    (!program.is_empty() && !id.is_empty() && !id.starts_with('-')).then_some((program, object, id))
}

pub(super) fn still_untagged(program: &str, id: &str) -> Result<bool, Failure> {
    let out = run(
        program,
        &["image", "inspect", "--format", "{{len .RepoTags}}", id],
        process::QUERY_TIMEOUT,
    )?;
    Ok(out.trim() == "0")
}

pub(super) fn refusal(engine: &str, failure: Failure) -> DeleteError {
    DeleteError::Tool(match failure {
        Failure::Failed(message) => format!("{engine} refused: {message}"),
        other => describe(engine, &other),
    })
}
