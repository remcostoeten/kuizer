mod json;
mod style;
mod text;

pub use json::{SCHEMA_VERSION, cache_report, duplicates_report, removal_report, scan_report};
pub use text::{
    clear_progress, display_path, largest_count, print_caches, print_candidates, print_duplicates,
    print_overview, print_progress, print_removal, print_removal_plan, print_removal_summary,
    print_scan, print_status,
};
