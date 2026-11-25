use std::path::Path;

use super::AppError;
use crate::config::Config;
use crate::detect::{Kind, explain_path};
use crate::platform::display_path;

pub fn run(config: &Config, path: &Path, json: bool) -> Result<(), AppError> {
    let info = explain_path(config, path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }

    println!("Path:      {}", display_path(&info.path));
    if let Some(kind) = info.kind {
        let label = match kind {
            Kind::Dir => "dir",
            Kind::File => "file",
            Kind::Symlink => "symlink",
            Kind::External => "external",
        };
        println!("Kind:      {label}");
    } else {
        println!("Kind:      missing");
    }
    println!(
        "Candidate: {}",
        if info.is_candidate { "yes" } else { "no" }
    );

    if let Some(matched) = &info.matched {
        println!(
            "Rule:      {} ({}, {}, {})",
            matched.name,
            matched.category.as_str(),
            matched.safety.as_str(),
            matched.source
        );
        println!("Ecosystem: {}", matched.ecosystem);
        if !matched.markers_found.is_empty() {
            println!("Markers:   {}", matched.markers_found.join(", "));
        }
        if !matched.contents_found.is_empty() {
            println!("Contents:  {}", matched.contents_found.join(", "));
        }
        for note in &matched.notes {
            println!("Note:      {note}");
        }
    } else if !info.near_misses.is_empty() {
        println!("Near misses:");
        for miss in &info.near_misses {
            let mut reasons = Vec::new();
            if !miss.missing_markers.is_empty() {
                reasons.push(format!(
                    "missing marker ({})",
                    miss.missing_markers.join(" or ")
                ));
            }
            if !miss.missing_contents.is_empty() {
                reasons.push(format!(
                    "missing contents ({})",
                    miss.missing_contents.join(" or ")
                ));
            }
            if let Some(note) = &miss.note {
                reasons.push(note.clone());
            }
            println!("  {}: {}", miss.rule, reasons.join("; "));
        }
    } else if info.exists {
        println!("Rule:      none matched");
    }

    if !info.blockers.is_empty() {
        println!("Blocked by:");
        for blocker in &info.blockers {
            println!("  - {blocker}");
        }
    }

    if let Some(guard) = &info.delete_guard {
        println!("Guard:     would refuse deletion ({guard})");
    } else if info.exists {
        println!("Guard:     allows deletion");
    }

    Ok(())
}
