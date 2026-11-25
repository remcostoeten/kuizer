use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use kuizer::caches::KnownCache;
use kuizer::delete::{Mode, Removal, Status};
use kuizer::detect::{Category, Kind, RuleInfo, Safety};
use kuizer::docker::{Item, Object};
use kuizer::output;
use kuizer::scan::{Candidate, Issue, IssueKind, Progress, Summary, Usage};
use kuizer::toolchains::{Tool, Toolchain};

#[test]
fn versioned_candidates_match_the_documented_snapshot() {
    let project = PathBuf::from("/fixture/app");
    let root = PathBuf::from("/fixture");
    let artifact = Candidate {
        id: 1,
        path: project.join("node_modules"),
        kind: Kind::Dir,
        rule: Arc::new(RuleInfo {
            name: "node-modules".to_owned(),
            ecosystem: "node".to_owned(),
            category: Category::Dependencies,
            safety: Safety::Safe,
        }),
        project: Some(project.clone()),
        usage: Usage {
            allocated: 4096,
            apparent: 128,
            entries: 2,
            partial: false,
        },
        newest: Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
        identity: None,
        project_last_active: Some(UNIX_EPOCH + Duration::from_secs(1_700_010_000)),
    };
    let cache = KnownCache {
        name: "npm",
        tool: "npm",
        path: PathBuf::from("/fixture/.npm/_cacache"),
        safety: Safety::Safe,
        note: "",
    };
    let toolchain = Toolchain {
        tool: Tool::Rustup,
        runtime: "rust".to_owned(),
        version: "stable".to_owned(),
        path: PathBuf::from("/fixture/.rustup/toolchains/stable"),
        default: true,
        pins: vec![project.join("rust-toolchain.toml")],
    };
    let engine = Item {
        engine: "podman".to_owned(),
        program: "podman".to_owned(),
        object: Object::Volume,
        id: "preview-data".to_owned(),
        label: "Preview data".to_owned(),
        bytes: 8192,
        safety: Safety::Review,
        note: "unused volume".to_owned(),
    };
    let roots = std::slice::from_ref(&root);
    let issue = Issue {
        path: root.join("locked"),
        kind: IssueKind::PermissionDenied,
        message: "unreadable".to_owned(),
    };
    let summary = Summary {
        progress: Progress {
            dirs: 2,
            files: 1,
            candidates: 1,
            bytes: 4096,
            issues: 1,
        },
        duration: Duration::from_millis(10),
        cancelled: false,
    };
    let removal = Removal {
        candidate: engine.candidate(4),
        status: Status::WouldRemove,
        error: None,
    };
    let reports: Vec<serde_json::Value> = [
        output::scan_report(roots, &[artifact], &[issue], &summary),
        output::cache_report(&[(
            cache.clone(),
            candidate(2, &cache.path, "npm", "npm", Category::Cache, Safety::Safe),
        )]),
        output::toolchain_report(&[(
            toolchain.clone(),
            candidate(
                3,
                &toolchain.path,
                "rustup",
                "rust",
                Category::Dependencies,
                Safety::Review,
            ),
        )]),
        output::docker_report(
            &[(engine.clone(), engine.candidate(4))],
            &["docker is not running".to_owned()],
        ),
        output::removal_report("docker", Mode::DryRun, &[], &[removal], &[]),
    ]
    .into_iter()
    .map(|report| serde_json::from_str(&report.expect("JSON report")).expect("valid JSON"))
    .collect();
    let snapshot: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("snapshots/candidates.json")).expect("report snapshot");
    assert_eq!(reports, snapshot);

    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../docs/report.schema.json"))
            .expect("published schema");
    let required = schema["$defs"]["candidate"]["required"]
        .as_array()
        .expect("required candidate fields");
    for report in &reports {
        assert_eq!(
            report["schema_version"],
            schema["$defs"]["version"]["const"]
        );
        let candidate = report
            .get("candidates")
            .or_else(|| report.get("items"))
            .expect("candidate rows");
        for field in required {
            let name = field.as_str().expect("required field name");
            assert!(
                candidate[0].get(name).is_some(),
                "missing {name} in {report}"
            );
        }
    }
}

fn candidate(
    id: u64,
    path: &Path,
    rule: &str,
    ecosystem: &str,
    category: Category,
    safety: Safety,
) -> Candidate {
    Candidate {
        id,
        path: path.to_owned(),
        kind: Kind::Dir,
        rule: Arc::new(RuleInfo {
            name: rule.to_owned(),
            ecosystem: ecosystem.to_owned(),
            category,
            safety,
        }),
        project: None,
        usage: Usage {
            allocated: 4096,
            apparent: 128,
            entries: 2,
            partial: true,
        },
        newest: None,
        identity: None,
        project_last_active: None,
    }
}
