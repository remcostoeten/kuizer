use std::collections::HashSet;
use std::path::PathBuf;

use serde::Serialize;

use crate::caches::KnownCache;
use crate::dupes::Duplicates;
use crate::history::History;
use crate::platform;
use crate::scan::{Candidate, Issue, Summary};

const TEMPLATE: &str = include_str!("report.html");
const DATA_SLOT: &str = "\"__KUIZER_DATA__\"";
const DASHBOARD_GROUPS: usize = 2000;

pub struct Dashboard<'a> {
    pub roots: &'a [PathBuf],
    pub candidates: &'a [Candidate],
    pub issues: &'a [Issue],
    pub summary: &'a Summary,
    pub caches: &'a [(KnownCache, Candidate)],
    pub duplicates: Option<&'a Duplicates>,
    pub history: &'a History,
    pub kept: &'a HashSet<(usize, usize)>,
    pub trash_known: bool,
    pub generated: &'a str,
}

#[derive(Serialize)]
struct HistoryJson<'a> {
    #[serde(flatten)]
    history: &'a History,
    kept: Vec<(usize, usize)>,
    trash_known: bool,
}

#[derive(Serialize)]
struct DiskJson<'a> {
    path: &'a PathBuf,
    available_bytes: Option<u64>,
    total_bytes: Option<u64>,
}

fn render(data: &str) -> String {
    // `<` only occurs inside JSON strings, so escaping it keeps `</script>` in a path inert.
    TEMPLATE.replacen(DATA_SLOT, &data.replace('<', "\\u003c"), 1)
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

/// @name scan_html
/// @description Renders a scan as one self-contained HTML page with an interactive treemap,
/// category and project charts and a searchable table. The page loads nothing from the network.
///
/// @example
/// let page = output::scan_html(&roots, &candidates, &issues, &summary, "2026-10-11 14:02");
/// std::fs::write("report.html", page)?;
pub fn scan_html(
    roots: &[PathBuf],
    candidates: &[Candidate],
    issues: &[Issue],
    summary: &Summary,
    generated: &str,
) -> String {
    let home = platform::home_dir().map(|home| home.display().to_string());
    let data = format!(
        "{{\"home\":{},\"generated\":{},\"scan\":{}}}",
        json(&home),
        json(&generated),
        super::scan_report(roots, candidates, issues, summary),
    );
    render(&data)
}

/// @name dashboard_data
/// @description Combines a scan, global caches, duplicate groups, the deletion history and free
/// disk space into the JSON document the dashboard page reads. Duplicate groups are capped at
/// the largest 2000; the summary still counts all of them.
///
/// @example
/// println!("{}", output::dashboard_data(&dashboard));
pub fn dashboard_data(dashboard: &Dashboard) -> String {
    let home = platform::home_dir().map(|home| home.display().to_string());
    let duplicates = dashboard.duplicates.map_or_else(
        || "null".to_owned(),
        |found| {
            let shown = &found.groups[..found.groups.len().min(DASHBOARD_GROUPS)];
            super::duplicates_report(dashboard.roots, found, shown)
        },
    );
    let mut kept: Vec<(usize, usize)> = dashboard.kept.iter().copied().collect();
    kept.sort_unstable();
    let history = HistoryJson {
        history: dashboard.history,
        kept,
        trash_known: dashboard.trash_known,
    };
    let disk: Vec<DiskJson> = dashboard
        .roots
        .iter()
        .map(|path| DiskJson {
            path,
            available_bytes: crate::platform::available_space(path),
            total_bytes: crate::platform::total_space(path),
        })
        .collect();
    format!(
        "{{\"dashboard\":true,\"home\":{},\"generated\":{},\"scan\":{},\"caches\":{},\"duplicates\":{},\"history\":{},\"disk\":{}}}",
        json(&home),
        json(&dashboard.generated),
        super::scan_report(
            dashboard.roots,
            dashboard.candidates,
            dashboard.issues,
            dashboard.summary
        ),
        super::cache_report(dashboard.caches),
        duplicates,
        json(&history),
        json(&disk),
    )
}

/// @name dashboard_html
/// @description Renders the dashboard as one self-contained HTML page with tabs for an overview,
/// project artifacts, folders, global caches, duplicates and the deletion history.
///
/// @example
/// std::fs::write("dashboard.html", output::dashboard_html(&dashboard))?;
pub fn dashboard_html(dashboard: &Dashboard) -> String {
    render(&dashboard_data(dashboard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::Progress;
    use std::time::Duration;

    fn summary() -> Summary {
        Summary {
            progress: Progress::default(),
            duration: Duration::from_millis(5),
            cancelled: false,
        }
    }

    #[test]
    fn embeds_data_and_escapes_script_tags() {
        let roots = [PathBuf::from("/tmp/</script><b>")];
        let page = scan_html(&roots, &[], &[], &summary(), "now");
        assert!(!page.contains(DATA_SLOT));
        assert!(page.contains("/tmp/\\u003c/script>\\u003cb>"));
        assert_eq!(page.matches("</script>").count(), 1);
    }

    #[test]
    fn dashboard_data_holds_every_source() {
        let roots = [PathBuf::from("/tmp")];
        let history = History::default();
        let kept = HashSet::from([(1, 0), (0, 2)]);
        let summary = summary();
        let dashboard = Dashboard {
            roots: &roots,
            candidates: &[],
            issues: &[],
            summary: &summary,
            caches: &[],
            duplicates: None,
            history: &history,
            kept: &kept,
            trash_known: true,
            generated: "now",
        };
        let data: serde_json::Value = serde_json::from_str(&dashboard_data(&dashboard)).unwrap();
        assert_eq!(data["dashboard"], true);
        assert_eq!(data["scan"]["command"], "scan");
        assert_eq!(data["caches"]["command"], "cache");
        assert!(data["duplicates"].is_null());
        assert_eq!(data["history"]["kept"], serde_json::json!([[0, 2], [1, 0]]));
        assert_eq!(data["disk"][0]["path"], "/tmp");
    }
}
