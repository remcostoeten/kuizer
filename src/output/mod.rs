mod git;
mod html;
mod json;
mod style;
mod text;

pub use git::{GitOutcome, git_report, print_git_outcome, print_repositories};
pub use html::{Dashboard, dashboard_data, dashboard_html, scan_html};
pub use json::{
    SCHEMA_VERSION, cache_lines, cache_report, docker_lines, docker_report, duplicates_link_report,
    duplicates_report, removal_line, removal_report, removal_summary_line, scan_line, scan_report,
    scan_summary_line, toolchain_lines, toolchain_report,
};
pub use text::{
    clear_progress, display_path, largest_count, print_caches, print_docker, print_duplicates,
    print_duplicates_link, print_issues, print_mount_spaces, print_overview, print_progress,
    print_removal, print_removal_plan, print_removal_summary, print_scan, print_status,
    print_toolchains,
};
