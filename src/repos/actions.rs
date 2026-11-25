use std::fs;
use std::path::Path;
use std::process::Output;
use std::time::SystemTime;

use super::refs::Refs;
use super::{Branch, Busy, Collected, KIB, Objects, Pruned, RepoError, Repository, STALE_GC};
use crate::config::GitConfig;
use crate::process;
use crate::scan::{self, Cancel};

/// @name busy
/// @description Returns the git operation in progress in `git_dir`, if any: a rebase, merge,
/// cherry-pick, revert or bisect, a held index or ref lock, or a running `git gc`.
///
/// @example
/// if let Some(reason) = repos::busy(Path::new("/home/me/apps/kearn/.git")) {
///     println!("skipped: {}", reason.describe());
/// }
pub fn busy(git_dir: &Path) -> Option<Busy> {
    let has = |name: &str| git_dir.join(name).exists();
    if has("rebase-merge") || has("rebase-apply") {
        return Some(Busy::Rebase);
    }
    if has("MERGE_HEAD") {
        return Some(Busy::Merge);
    }
    if has("CHERRY_PICK_HEAD") {
        return Some(Busy::CherryPick);
    }
    if has("REVERT_HEAD") {
        return Some(Busy::Revert);
    }
    if has("BISECT_LOG") {
        return Some(Busy::Bisect);
    }
    if ["index.lock", "HEAD.lock", "packed-refs.lock", "config.lock"]
        .iter()
        .any(|name| has(name))
    {
        return Some(Busy::Locked);
    }
    let collecting = fs::metadata(git_dir.join("gc.pid"))
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .is_ok_and(|age| age < STALE_GC)
        });
    collecting.then_some(Busy::Collecting)
}

/// @name collect_garbage
/// @description Runs `git gc` in the repository, with `--aggressive` when `git.aggressive` is
/// set, after checking again that no other git operation is in progress. Measures the git
/// folder before and after so the result reports what was actually freed. Nothing goes to the
/// trash: unreachable objects older than git's prune expiry are gone for good.
///
/// @example
/// let collected = repos::collect_garbage(&repository, &config.git)?;
/// println!("freed {}", ByteSize(collected.freed()));
pub fn collect_garbage(
    repository: &Repository,
    config: &GitConfig,
) -> Result<Collected, RepoError> {
    if !repository.git_dir.is_dir() {
        return Err(RepoError::NotRepository(repository.root.clone()));
    }
    if let Some(reason) = busy(&repository.git_dir) {
        return Err(RepoError::Busy(reason));
    }
    let cancel = Cancel::new();
    let before = scan::measure(&repository.git_dir, &cancel).usage.allocated;
    let mut args = vec!["gc", "--quiet"];
    if config.aggressive {
        args.push("--aggressive");
    }
    let output = process::git(&repository.root, &args, process::MUTATION_TIMEOUT)
        .map_err(|error| RepoError::Git(error.to_string()))?;
    if !output.status.success() {
        return Err(RepoError::Git(stderr(&output)));
    }
    let after = scan::measure(&repository.git_dir, &cancel).usage.allocated;
    Ok(Collected { before, after })
}

/// @name delete_branches
/// @description Deletes the named local branches of the repository. Each one is checked again
/// first: it must still point at the commit that was listed, still be merged into the default
/// branch, and not be kept, the default branch or checked out. Remote branches are never
/// touched. Restore one with `git branch NAME COMMIT`.
///
/// @example
/// for pruned in repos::delete_branches(&repository, &repository.merged, &config.git) {
///     println!("{}: {:?}", pruned.branch.name, pruned.result);
/// }
pub fn delete_branches(
    repository: &Repository,
    branches: &[Branch],
    config: &GitConfig,
) -> Vec<Pruned> {
    if let Some(reason) = busy(&repository.git_dir) {
        return branches
            .iter()
            .map(|branch| Pruned {
                branch: branch.clone(),
                result: Err(RepoError::Busy(reason)),
            })
            .collect();
    }
    let root = &repository.root;
    let current = read(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let base = default_branch(root, None, config);
    let merge_bases = base
        .as_deref()
        .map(|base| bases(root, None, base))
        .unwrap_or_default();
    let checked_out = worktree_branches(root);
    branches
        .iter()
        .map(|branch| Pruned {
            branch: branch.clone(),
            result: delete_branch(
                root,
                branch,
                (base.as_deref(), &merge_bases),
                current.as_deref(),
                &checked_out,
                config,
            ),
        })
        .collect()
}

fn delete_branch(
    root: &Path,
    branch: &Branch,
    (base, merge_bases): (Option<&str>, &[String]),
    current: Option<&str>,
    checked_out: &[String],
    config: &GitConfig,
) -> Result<(), RepoError> {
    let name = branch.name.as_str();
    if kept(name, base, current, checked_out, config) {
        return Err(RepoError::Kept(branch.name.clone()));
    }
    let reference = format!("refs/heads/{name}");
    let at = read(root, &["rev-parse", "--verify", "--quiet", &reference]);
    let merged = merge_bases
        .iter()
        .any(|base| succeeds(root, &["merge-base", "--is-ancestor", &branch.commit, base]));
    if at.as_deref() != Some(branch.commit.as_str()) || !merged {
        return Err(RepoError::Changed(branch.name.clone()));
    }
    delete_ref_at(root, name, &branch.commit)
}

pub(super) fn delete_ref_at(root: &Path, name: &str, commit: &str) -> Result<(), RepoError> {
    let reference = format!("refs/heads/{name}");
    let output = git(root, &["update-ref", "-d", &reference, commit]).map_err(RepoError::Git)?;
    if !output.status.success() {
        return Err(RepoError::Changed(name.to_owned()));
    }
    let section = format!("branch.{name}");
    let _ = git(root, &["config", "--remove-section", &section]);
    Ok(())
}

fn kept(
    name: &str,
    base: Option<&str>,
    current: Option<&str>,
    checked_out: &[String],
    config: &GitConfig,
) -> bool {
    Some(name) == base
        || Some(name) == current
        || checked_out.iter().any(|branch| branch == name)
        || config.keep_branches.iter().any(|kept| kept == name)
}

pub(super) fn count_objects(root: &Path) -> Result<Objects, RepoError> {
    let output = git(root, &["count-objects", "-v"]).map_err(RepoError::Git)?;
    if !output.status.success() {
        return Err(RepoError::NotRepository(root.to_path_buf()));
    }
    Ok(parse_count(&String::from_utf8_lossy(&output.stdout)))
}

pub(super) fn parse_count(text: &str) -> Objects {
    let mut objects = Objects::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<u64>() else {
            continue;
        };
        match key.trim() {
            "count" => objects.loose = value,
            "size" => objects.loose_bytes = value * KIB,
            "packs" => objects.packs = value,
            "size-pack" => objects.packed_bytes = value * KIB,
            "prune-packable" => objects.prunable = value,
            "size-garbage" => objects.garbage_bytes = value * KIB,
            _ => {}
        }
    }
    objects
}

pub(super) fn default_branch(
    root: &Path,
    refs: Option<&Refs>,
    config: &GitConfig,
) -> Option<String> {
    let remote = match refs {
        Some(refs) => refs.origin_head.clone(),
        None => read(
            root,
            &[
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
        ),
    };
    if let Some(name) = remote
        .as_deref()
        .and_then(|remote| remote.strip_prefix("origin/"))
    {
        return Some(name.to_owned());
    }
    ["main", "master"]
        .into_iter()
        .chain(config.keep_branches.iter().map(String::as_str))
        .find(|name| has_ref(root, refs, &format!("refs/heads/{name}")))
        .map(str::to_owned)
}

fn bases(root: &Path, refs: Option<&Refs>, base: &str) -> Vec<String> {
    [
        format!("refs/heads/{base}"),
        format!("refs/remotes/origin/{base}"),
    ]
    .into_iter()
    .filter(|reference| has_ref(root, refs, reference))
    .collect()
}

pub(super) fn merged_branches(
    root: &Path,
    refs: Option<&Refs>,
    base: &str,
    current: Option<&str>,
    config: &GitConfig,
) -> Vec<Branch> {
    let checked_out = match refs {
        Some(refs) if !refs.worktrees => current.map(str::to_owned).into_iter().collect(),
        _ => worktree_branches(root),
    };
    if let Some(refs) = refs
        && refs
            .heads
            .iter()
            .all(|name| kept(name, Some(base), current, &checked_out, config))
    {
        return Vec::new();
    }
    let bases = bases(root, refs, base);
    if bases.is_empty() {
        return Vec::new();
    }
    let mut args = vec![
        "for-each-ref".to_owned(),
        "--format=%(refname:lstrip=2)%09%(objectname)".to_owned(),
    ];
    args.extend(
        bases
            .iter()
            .map(|reference| format!("--merged={reference}")),
    );
    args.push("refs/heads/".to_owned());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(listed) = read(root, &args) else {
        return Vec::new();
    };
    let mut merged: Vec<Branch> = Vec::new();
    for line in listed.lines() {
        let Some((name, commit)) = line.split_once('\t') else {
            continue;
        };
        if kept(name, Some(base), current, &checked_out, config)
            || merged.iter().any(|branch| branch.name == name)
        {
            continue;
        }
        merged.push(Branch {
            name: name.to_owned(),
            commit: commit.to_owned(),
        });
    }
    merged.sort_by(|a, b| a.name.cmp(&b.name));
    merged
}

fn worktree_branches(root: &Path) -> Vec<String> {
    read(root, &["worktree", "list", "--porcelain"])
        .map(|text| {
            text.lines()
                .filter_map(|line| line.strip_prefix("branch refs/heads/"))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn has_ref(root: &Path, refs: Option<&Refs>, reference: &str) -> bool {
    match refs {
        Some(refs) => refs.names.contains(reference),
        None => succeeds(root, &["show-ref", "--verify", "--quiet", reference]),
    }
}

pub(super) fn succeeds(root: &Path, args: &[&str]) -> bool {
    git(root, args).is_ok_and(|output| output.status.success())
}

pub(super) fn read(root: &Path, args: &[&str]) -> Option<String> {
    let output = git(root, args).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim_end_matches(['\n', '\r']);
    (!text.is_empty()).then(|| text.to_owned())
}

pub(super) fn stderr(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let text = text.trim();
    if text.is_empty() {
        format!("exited with {}", output.status)
    } else {
        text.lines().last().unwrap_or(text).to_owned()
    }
}

pub(super) fn git(root: &Path, args: &[&str]) -> Result<Output, String> {
    process::git(root, args, process::QUERY_TIMEOUT).map_err(|error| error.to_string())
}
