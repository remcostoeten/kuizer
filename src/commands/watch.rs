use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::AppError;
use super::filter::{self, Filters};
use super::interrupt::Interrupt;
use super::scan::resolve_roots;
use crate::cli::WatchArgs;
use crate::config::Config;
use crate::output;
use crate::scan::{self, Cancel, Candidate, MountSpace, ProjectGrowth, ScanDiff};
use crate::units::{ByteSize, WatchInterval};

const SLEEP_SLICE: Duration = Duration::from_millis(100);
const DEFAULT_INTERVAL_SECS: u64 = 30;

#[derive(Serialize)]
struct WatchTickJson<'a> {
    scan: usize,
    timestamp: String,
    total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta_bytes: Option<i64>,
    mounts: &'a [MountSpace],
    grown: Vec<&'a ProjectGrowth>,
}

pub fn run(config: &Config, args: WatchArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let rules = filter::project_rules(config);
    let names: Vec<&str> = rules.iter().map(String::as_str).collect();
    let filters = Filters::new(config, &args.categories, args.min_size, &args.scope, &names)?;
    let effective = filters.config();
    let roots = scan::normalize_roots(&resolve_roots(effective, args.paths)?);
    let interval = args
        .interval
        .unwrap_or(WatchInterval(Duration::from_secs(DEFAULT_INTERVAL_SECS)));

    let interrupt = Interrupt::catch();
    if !json && !quiet {
        let root_list = roots
            .iter()
            .map(|r| output::display_path(r))
            .collect::<Vec<_>>()
            .join(", ");
        println!("Watching {root_list} every {interval} (read-only; press Ctrl-C to stop)");
    }

    let mut previous: Option<Vec<Candidate>> = None;
    let mut scan_number: usize = 0;

    loop {
        if interrupt.requested() {
            break;
        }
        scan_number += 1;

        let cancel = Cancel::new();
        let handle = scan::start(effective, &roots, cancel.clone())?;
        let collected = scan::collect_with(
            &handle,
            |_| {
                if interrupt.requested() {
                    cancel.cancel();
                }
            },
            |_| {},
        );
        if interrupt.requested() || collected.summary.cancelled {
            break;
        }

        let mut current = collected.candidates;
        current.retain(|candidate| filters.matches(candidate));

        let mounts = scan::mount_spaces(&roots, &current);
        let total: u64 = current.iter().map(|c| c.usage.allocated).sum();
        let diff = previous
            .as_deref()
            .map(|prev| scan::diff_candidates(prev, &current));
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();

        if json {
            let grown: Vec<&ProjectGrowth> = diff
                .as_ref()
                .map(|d| d.grown_projects().collect())
                .unwrap_or_default();
            let line = serde_json::to_string(&WatchTickJson {
                scan: scan_number,
                timestamp,
                total_bytes: total,
                delta_bytes: diff.as_ref().map(|d| d.delta_bytes),
                mounts: &mounts,
                grown,
            })?;
            let mut out = io::stdout().lock();
            writeln!(out, "{line}")?;
            out.flush()?;
        } else {
            print_tick(
                scan_number,
                &timestamp,
                total,
                current.len(),
                &mounts,
                diff.as_ref(),
                quiet,
            );
        }

        previous = Some(current);

        if args.count.is_some_and(|max| scan_number >= max) {
            break;
        }

        if !sleep_until(interval.0, &interrupt) {
            break;
        }
    }

    Ok(())
}

fn sleep_until(duration: Duration, interrupt: &Interrupt) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if interrupt.requested() {
            return false;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        thread::sleep(remaining.min(SLEEP_SLICE));
    }
    !interrupt.requested()
}

fn print_tick(
    scan_number: usize,
    timestamp: &str,
    total: u64,
    items: usize,
    mounts: &[MountSpace],
    diff: Option<&ScanDiff>,
    quiet: bool,
) {
    let mut out = io::stdout().lock();
    match diff {
        None => {
            let _ = writeln!(
                out,
                "[{timestamp}] baseline  {} in {items} items",
                ByteSize(total)
            );
            drop(out);
            if !quiet {
                output::print_mount_spaces(mounts);
            }
        }
        Some(diff) => {
            let grown: Vec<&ProjectGrowth> = diff.grown_projects().collect();
            if grown.is_empty() {
                if !quiet {
                    let _ = writeln!(
                        out,
                        "[{timestamp}] scan #{scan_number}  no growth  ({} total)",
                        ByteSize(total)
                    );
                }
            } else {
                let net_added: u64 = grown
                    .iter()
                    .map(|g| u64::try_from(g.delta_bytes).unwrap_or(0))
                    .sum();
                let _ = writeln!(
                    out,
                    "[{timestamp}] scan #{scan_number}  +{} grew across {} projects  ({} total)",
                    ByteSize(net_added),
                    grown.len(),
                    ByteSize(total)
                );
                for project in grown {
                    let added = u64::try_from(project.delta_bytes).unwrap_or(0);
                    let detail = project
                        .grown_paths
                        .first()
                        .and_then(|(path, delta)| {
                            let name = path.file_name()?.to_string_lossy();
                            let bytes = u64::try_from(*delta).ok()?;
                            Some(format!(" ({name} +{})", ByteSize(bytes)))
                        })
                        .unwrap_or_default();
                    let _ = writeln!(
                        out,
                        "  +{:>9}  {}{detail}",
                        ByteSize(added).to_string(),
                        output::display_path(&project.project),
                    );
                }
            }
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn watch_count_one_completes_without_deleting_anything() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("demo");
        let target = project.join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]\nname=\"demo\"\n").unwrap();
        fs::write(target.join("artifact.bin"), vec![0u8; 4096]).unwrap();

        let config = Config::default();
        let args = WatchArgs {
            paths: vec![dir.path().to_path_buf()],
            interval: Some(WatchInterval(Duration::from_secs(1))),
            count: Some(1),
            ..WatchArgs::default()
        };
        run(&config, args, true, true).expect("watch completes");
        assert!(target.join("artifact.bin").exists());
    }
}
