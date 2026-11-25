use std::io::{self, IsTerminal};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use super::AppError;
use super::filter;
use super::removal::{self, Request};
use super::scan::{keep_idle, narrow, resolve_roots};
use crate::caches::{self, Locations as CacheLocations};
use crate::cli::{CleanArgs, Format, PolicyAction, PolicyArgs, Selection};
use crate::config::{Config, Policy, PolicySelect, SourceKind, known_rules};
use crate::delete::Mode;
use crate::detect::{Category, Safety};
use crate::docker::{self, Object};
use crate::output;
use crate::protect::Protected;
use crate::scan::{self, Cancel, Candidate, Issue, IssueKind, Scope};
use crate::source::Source;
use crate::toolchains::{self, Locations as ToolchainLocations, pins};
use crate::units::{ByteSize, Idle};

#[derive(Debug, Clone, Serialize)]
struct PolicyListEntry<'a> {
    name: &'a str,
    source: SourceKind,
    select: &'a PolicySelect,
    permanent: bool,
    paths: &'a [PathBuf],
    category: &'a [Category],
    rule: &'a [String],
    project: &'a [String],
    idle: Option<Idle>,
    min_size: Option<ByteSize>,
    exclude: &'a [String],
    summary: String,
}

#[derive(Debug, Clone, Serialize)]
struct EffectivePolicy {
    name: String,
    source: SourceKind,
    roots: Vec<PathBuf>,
    categories: Vec<Category>,
    rules: Vec<String>,
    projects: Vec<String>,
    idle: Option<Idle>,
    min_size: ByteSize,
    exclude: Vec<String>,
    select: PolicySelect,
    mode: Mode,
    permanent: bool,
}

/// @name run
/// @description Dispatches `kuizer policy list` (the default) and `kuizer policy show NAME`.
///
/// @example
/// policy::run(&loaded.config, args, cli.json)?;
pub fn run(config: &Config, args: PolicyArgs, json: bool) -> Result<(), AppError> {
    match args.command.unwrap_or(PolicyAction::List) {
        PolicyAction::List => list(config, json),
        PolicyAction::Show { name } => show(config, &name, json),
        PolicyAction::Help => Ok(()),
    }
}

fn lookup<'a>(config: &'a Config, name: &str) -> Result<&'a Policy, AppError> {
    config.policy.get(name).ok_or_else(|| {
        if config.policy.is_empty() {
            AppError::Usage(format!(
                "unknown policy `{name}`, no policies are configured"
            ))
        } else {
            let names: Vec<&str> = config.policy.keys().map(String::as_str).collect();
            AppError::Usage(format!(
                "unknown policy `{name}`, configured policies: {}",
                names.join(", ")
            ))
        }
    })
}

fn filter_summary(policy: &Policy) -> String {
    let mut parts = Vec::new();
    if !policy.paths.is_empty() {
        let paths: Vec<String> = policy
            .paths
            .iter()
            .map(|p| output::display_path(p))
            .collect();
        parts.push(format!("paths={}", paths.join(",")));
    }
    if !policy.category.is_empty() {
        let cats: Vec<&str> = policy.category.iter().map(|c| c.as_str()).collect();
        parts.push(format!("category={}", cats.join(",")));
    }
    if !policy.rule.is_empty() {
        parts.push(format!("rule={}", policy.rule.join(",")));
    }
    if !policy.project.is_empty() {
        parts.push(format!("project={}", policy.project.join(",")));
    }
    if let Some(idle) = policy.idle {
        parts.push(format!("idle={idle}"));
    }
    if let Some(min_size) = policy.min_size {
        parts.push(format!("min_size={min_size}"));
    }
    if !policy.exclude.is_empty() {
        parts.push(format!("exclude={}", policy.exclude.join(",")));
    }
    if policy.permanent {
        parts.push("permanent".to_owned());
    }
    if parts.is_empty() {
        "-".to_owned()
    } else {
        parts.join(", ")
    }
}

fn list(config: &Config, json: bool) -> Result<(), AppError> {
    let entries: Vec<PolicyListEntry<'_>> = config
        .policy
        .iter()
        .map(|(name, policy)| PolicyListEntry {
            name: name.as_str(),
            source: policy.source,
            select: &policy.select,
            permanent: policy.permanent,
            paths: &policy.paths,
            category: &policy.category,
            rule: &policy.rule,
            project: &policy.project,
            idle: policy.idle,
            min_size: policy.min_size,
            exclude: &policy.exclude,
            summary: filter_summary(policy),
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    if entries.is_empty() {
        println!("no policies configured, add a [policy.NAME] table to the config file");
        return Ok(());
    }
    let name_w = entries
        .iter()
        .map(|entry| entry.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let source_w = entries
        .iter()
        .map(|entry| entry.source.as_str().len())
        .max()
        .unwrap_or(6)
        .max(6);
    let select_w = entries
        .iter()
        .map(|entry| entry.select.to_string().len())
        .max()
        .unwrap_or(6)
        .max(6);
    println!(
        "{:<name_w$}  {:<source_w$}  {:<select_w$}  FILTERS",
        "NAME", "SOURCE", "SELECT"
    );
    for entry in entries {
        println!(
            "{:<name_w$}  {:<source_w$}  {:<select_w$}  {}",
            entry.name,
            entry.source.as_str(),
            entry.select,
            entry.summary
        );
    }
    Ok(())
}

fn effective_mode(config: &Config, policy: &Policy, dry_run: bool) -> Mode {
    if dry_run {
        return Mode::DryRun;
    }
    if policy.permanent {
        return Mode::Permanent;
    }
    source_of(policy.source).mode(config.delete.mode.into())
}

fn source_of(kind: SourceKind) -> Source {
    match kind {
        SourceKind::Projects => Source::Artifact,
        SourceKind::Cache => Source::Cache,
        SourceKind::Toolchain => Source::Toolchain,
        SourceKind::Docker => Source::Docker,
    }
}

fn union_strings(base: &[String], extra: &[String]) -> Vec<String> {
    let mut out = base.to_vec();
    for item in extra {
        if !out.contains(item) {
            out.push(item.clone());
        }
    }
    out
}

fn resolve_effective(
    config: &Config,
    name: &str,
    policy: &Policy,
) -> Result<EffectivePolicy, AppError> {
    let roots = match policy.source {
        SourceKind::Projects => {
            scan::normalize_roots(&resolve_roots(config, policy.paths.clone())?)
        }
        SourceKind::Cache | SourceKind::Toolchain | SourceKind::Docker => Vec::new(),
    };
    let categories = match policy.source {
        SourceKind::Projects => {
            let mut categories = config.detect.categories.clone();
            if !policy.category.is_empty() {
                categories.retain(|category| policy.category.contains(category));
            }
            categories
        }
        SourceKind::Cache | SourceKind::Toolchain | SourceKind::Docker => policy.category.clone(),
    };
    let min_size = config
        .scan
        .min_size
        .max(policy.min_size.unwrap_or(ByteSize(0)));
    let exclude = union_strings(&config.scan.exclude, &policy.exclude);
    let mode = effective_mode(config, policy, false);
    Ok(EffectivePolicy {
        name: name.to_owned(),
        source: policy.source,
        roots,
        categories,
        rules: policy.rule.clone(),
        projects: policy.project.clone(),
        idle: policy.idle,
        min_size,
        exclude,
        select: policy.select.clone(),
        mode,
        permanent: policy.permanent,
    })
}

fn show(config: &Config, name: &str, json: bool) -> Result<(), AppError> {
    let policy = lookup(config, name)?;
    let effective = resolve_effective(config, name, policy)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&effective)?);
        return Ok(());
    }
    println!("{:<10} {}", "policy:", effective.name);
    println!("{:<10} {}", "source:", effective.source);
    if effective.source == SourceKind::Projects {
        let roots: Vec<String> = effective
            .roots
            .iter()
            .map(|p| output::display_path(p))
            .collect();
        println!("{:<10} {}", "roots:", roots.join(", "));
    }
    let categories = if effective.categories.is_empty() {
        if effective.source == SourceKind::Projects {
            "none".to_owned()
        } else {
            "all".to_owned()
        }
    } else {
        effective
            .categories
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("{:<10} {}", "category:", categories);
    println!(
        "{:<10} {}",
        "rule:",
        if effective.rules.is_empty() {
            "all".to_owned()
        } else {
            effective.rules.join(", ")
        }
    );
    if !effective.projects.is_empty() {
        println!("{:<10} {}", "project:", effective.projects.join(", "));
    }
    if effective.source == SourceKind::Projects {
        println!(
            "{:<10} {}",
            "idle:",
            effective
                .idle
                .map_or_else(|| "none".to_owned(), |idle| idle.to_string())
        );
    }
    println!("{:<10} {}", "min_size:", effective.min_size);
    println!(
        "{:<10} {}",
        "exclude:",
        if effective.exclude.is_empty() {
            "none".to_owned()
        } else {
            effective.exclude.join(", ")
        }
    );
    let mode = match effective.mode {
        Mode::Trash => "trash",
        Mode::Permanent => "permanent",
        Mode::DryRun => "dry-run",
    };
    println!("{:<10} {}", "select:", effective.select);
    println!("{:<10} {}", "mode:", mode);
    println!("{:<10} {}", "permanent:", effective.permanent);
    Ok(())
}

fn clean_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn canonical_or_abs(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(std::fs::canonicalize(&absolute).unwrap_or_else(|_| clean_components(&absolute)))
}

fn narrow_roots(
    name: &str,
    policy_roots: &[PathBuf],
    cli_paths: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, AppError> {
    if cli_paths.is_empty() {
        return Ok(policy_roots.to_vec());
    }
    let normalized_policy: Vec<PathBuf> = policy_roots
        .iter()
        .map(|root| canonical_or_abs(root))
        .collect::<io::Result<Vec<_>>>()?;
    for cli_path in &cli_paths {
        let candidate = canonical_or_abs(cli_path)?;
        if !normalized_policy
            .iter()
            .any(|root| candidate.starts_with(root))
        {
            return Err(AppError::Usage(format!(
                "path `{}` is outside policy `{name}` roots",
                cli_path.display()
            )));
        }
    }
    Ok(scan::normalize_roots(&cli_paths))
}

fn rule_is_always_safe(config: &Config, source: SourceKind, rule: &str) -> Result<bool, AppError> {
    match source {
        SourceKind::Projects => {
            let specs = config.rule_specs()?;
            let mut matched = false;
            for spec in specs.iter().filter(|spec| spec.name == rule) {
                matched = true;
                if spec.safety != Safety::Safe {
                    return Ok(false);
                }
            }
            Ok(matched)
        }
        SourceKind::Cache => Ok(caches::safety(rule, &config.catalog) == Some(Safety::Safe)),
        SourceKind::Docker => Ok(matches!(rule, "image" | "build-cache")),
        SourceKind::Toolchain => {
            // Any toolchain manager can have default or project-pinned versions marked `review`.
            // Selecting by manager name would bypass the `safe` restriction.
            Ok(false)
        }
    }
}

fn narrow_selection(
    config: &Config,
    name: &str,
    policy: &Policy,
    cli_select: &[Selection],
    known: &[&str],
) -> Result<Vec<Selection>, AppError> {
    removal::check_selection(cli_select, known)?;
    if cli_select.is_empty() {
        return Ok(match &policy.select {
            PolicySelect::Safe => vec![Selection::Safe],
            PolicySelect::All => vec![Selection::All],
            PolicySelect::Rules(rules) => rules.iter().cloned().map(Selection::Rule).collect(),
        });
    }
    for selection in cli_select {
        match (&policy.select, selection) {
            (PolicySelect::All, _) => {}
            (PolicySelect::Safe, Selection::Safe) => {}
            (PolicySelect::Safe, Selection::All) => {
                return Err(AppError::Usage(format!(
                    "cannot pass --select all to policy `{name}` because its selection is `safe`"
                )));
            }
            (PolicySelect::Safe, Selection::Rule(rule)) => {
                if !rule_is_always_safe(config, policy.source, rule)? {
                    return Err(AppError::Usage(format!(
                        "cannot select `{rule}` with policy `{name}` because its selection is `safe`"
                    )));
                }
            }
            (PolicySelect::Rules(allowed), Selection::Rule(rule)) => {
                if !allowed.iter().any(|entry| entry == rule) {
                    return Err(AppError::Usage(format!(
                        "cannot select `{rule}` with policy `{name}`, allowed rules: {}",
                        allowed.join(", ")
                    )));
                }
            }
            (PolicySelect::Rules(allowed), Selection::All) => {
                return Err(AppError::Usage(format!(
                    "cannot pass --select all to policy `{name}`, allowed rules: {}",
                    allowed.join(", ")
                )));
            }
            (PolicySelect::Rules(allowed), Selection::Safe) => {
                return Err(AppError::Usage(format!(
                    "cannot pass --select safe to policy `{name}`, allowed rules: {}",
                    allowed.join(", ")
                )));
            }
        }
    }
    Ok(cli_select.to_vec())
}

struct PolicyFilters {
    config: Config,
    evidence: Config,
    policy_scope: Scope,
    cli_scope: Scope,
}

impl PolicyFilters {
    fn build(
        config: &Config,
        policy: &Policy,
        effective: &EffectivePolicy,
        args: &CleanArgs,
        known_filter_rules: &[&str],
    ) -> Result<Self, AppError> {
        if let Some(rule) = args
            .scope
            .rules
            .iter()
            .find(|name| !known_filter_rules.contains(&name.as_str()))
        {
            return Err(AppError::Usage(format!(
                "unknown rule `{rule}`, expected one of: {}",
                known_filter_rules.join(", ")
            )));
        }
        let combined_min_size = effective.min_size.max(args.min_size.unwrap_or(ByteSize(0)));
        let policy_scope = Scope::new(&policy.rule, &policy.project, &policy.exclude)?
            .within(&policy.category, effective.min_size.0);
        let cli_scope = Scope::new(
            &args.scope.rules,
            &args.scope.projects,
            &args.scope.excludes,
        )?
        .within(&args.categories, combined_min_size.0);

        let mut evidence = config.clone();
        narrow(&mut evidence, &policy.category, policy.min_size);
        narrow(&mut evidence, &args.categories, Some(combined_min_size));
        let mut scan_config = evidence.clone();
        scan_config
            .scan
            .exclude
            .extend(policy.exclude.iter().cloned());
        scan_config
            .scan
            .exclude
            .extend(args.scope.excludes.iter().cloned());

        Ok(Self {
            config: scan_config,
            evidence,
            policy_scope,
            cli_scope,
        })
    }

    fn matches(&self, candidate: &Candidate) -> bool {
        self.policy_scope.matches(candidate) && self.cli_scope.matches(candidate)
    }

    fn keep<T>(&self, entries: &mut Vec<(T, Candidate)>) {
        entries.retain(|(_, candidate)| self.matches(candidate));
    }
}

/// @name execute
/// @description Runs `kuizer clean --policy NAME`, combining CLI flags strictly by narrowing
/// and dispatching to the policy's source.
///
/// @example
/// policy::execute(config, &name, args, format, quiet)?;
pub fn execute(
    config: &Config,
    name: &str,
    args: CleanArgs,
    format: Format,
    quiet: bool,
) -> Result<(), AppError> {
    let policy = lookup(config, name)?;
    if args.permanent && !policy.permanent {
        return Err(AppError::Usage(format!(
            "cannot pass --permanent to policy `{name}` because it does not set permanent = true"
        )));
    }
    if policy.source != SourceKind::Projects && !args.paths.is_empty() {
        return Err(AppError::Usage(format!(
            "paths cannot be passed with `--policy {name}` because its source is `{}`",
            policy.source
        )));
    }
    if policy.source != SourceKind::Projects && args.idle.is_some() {
        return Err(AppError::Usage(format!(
            "--idle cannot be passed with `--policy {name}` because its source is `{}`",
            policy.source
        )));
    }

    let effective = resolve_effective(config, name, policy)?;
    let filter_rules = match policy.source {
        SourceKind::Projects => filter::project_rules(config),
        other => known_rules(config, other)?,
    };
    let filter_rule_refs: Vec<&str> = filter_rules.iter().map(String::as_str).collect();
    let filters = PolicyFilters::build(config, policy, &effective, &args, &filter_rule_refs)?;

    let selection_rules = known_rules(config, policy.source)?;
    let selection_rule_refs: Vec<&str> = selection_rules.iter().map(String::as_str).collect();
    let selections = narrow_selection(
        config,
        name,
        policy,
        &args.removal.select,
        &selection_rule_refs,
    )?;

    let mode = effective_mode(&filters.config, policy, args.removal.dry_run);
    let json = format != Format::Text;
    let show_progress = format == Format::Text && !quiet && io::stderr().is_terminal();

    match policy.source {
        SourceKind::Projects => {
            let roots = narrow_roots(name, &effective.roots, args.paths)?;
            let effective_idle = match (policy.idle, args.idle) {
                (Some(a), Some(b)) => Some(Idle(a.0.max(b.0))),
                (a, b) => a.or(b),
            };
            let handle = scan::start(&filters.config, &roots, Cancel::new())?;
            let collected = scan::collect(&handle, |progress| {
                if show_progress {
                    output::print_progress(progress);
                }
            });
            if show_progress {
                output::clear_progress();
            }
            let mut candidates = collected.candidates;
            candidates.retain(|candidate| filters.matches(candidate));
            keep_idle(&filters.evidence, &mut candidates, effective_idle, false)?;
            let selected = removal::select(candidates, &selections);
            let request = Request {
                command: "clean",
                policy: Some(name),
                roots: &roots,
                mode,
                yes: args.removal.yes,
                confirm: filters.config.delete.confirm,
                format,
                source: Source::Artifact,
                engines: &filters.config.docker.engines,
                delete: &filters.config.delete,
            };
            removal::run(&request, selected, &collected.issues)
        }
        SourceKind::Cache => {
            let locations = CacheLocations::for_user().ok_or(AppError::NoHome)?;
            let mut entries = caches::measure_threaded(
                caches::find_excluding(
                    &locations,
                    &Protected::for_user(),
                    &filters.config.catalog,
                    &filters.config.excludes()?,
                ),
                &Cancel::new(),
                scan::size_threads(&filters.config),
                |_| {},
            );
            filters.keep(&mut entries);
            let candidates: Vec<Candidate> = entries
                .into_iter()
                .map(|(_, candidate)| candidate)
                .collect();
            let selected = removal::select(candidates, &selections);
            let roots = caches::guard_roots(&selected);
            let request = Request {
                command: "clean",
                policy: Some(name),
                roots: &roots,
                mode,
                yes: args.removal.yes,
                confirm: filters.config.delete.confirm,
                format,
                source: Source::Cache,
                engines: &filters.config.docker.engines,
                delete: &filters.config.delete,
            };
            removal::run(&request, selected, &[])
        }
        SourceKind::Toolchain => {
            let locations = ToolchainLocations::for_user().ok_or(AppError::NoHome)?;
            let mut found = toolchains::find_excluding(
                &locations,
                &filters.config.toolchains.tools,
                &filters.config.excludes()?,
            );
            if filters.config.toolchains.pins && !found.is_empty() {
                let pin_roots = resolve_roots(&filters.config, Vec::new())?;
                if !quiet && !json {
                    output::print_status("looking for pinned versions");
                }
                let pins = pins::find(&filters.evidence, &pin_roots, &Cancel::new())?;
                if !quiet && !json {
                    output::clear_progress();
                }
                toolchains::apply_pins(&mut found, &pins);
            }
            let mut entries = toolchains::measure_with_threads(
                found,
                &Cancel::new(),
                scan::size_threads(&filters.config),
                |_| {},
            );
            filters.keep(&mut entries);
            let candidates: Vec<Candidate> = entries
                .into_iter()
                .map(|(_, candidate)| candidate)
                .collect();
            let selected = removal::select(candidates, &selections);
            let roots = toolchains::roots(&selected);
            let request = Request {
                command: "clean",
                policy: Some(name),
                roots: &roots,
                mode: Source::Toolchain.mode(mode),
                yes: args.removal.yes,
                confirm: filters.config.delete.confirm,
                format,
                source: Source::Toolchain,
                engines: &filters.config.docker.engines,
                delete: &filters.config.delete,
            };
            removal::run(&request, selected, &[])
        }
        SourceKind::Docker => {
            let _ = Object::ALL;
            if !quiet && !json {
                output::print_status("asking docker and podman");
            }
            let listing = docker::list(
                &filters.config.docker.engines,
                filters.config.docker.timeout(),
                filters.config.docker.recent_hours,
            );
            if !quiet && !json {
                output::clear_progress();
            }
            let mut entries = docker::entries(listing.items);
            filters.keep(&mut entries);
            let issues: Vec<Issue> = listing
                .issues
                .iter()
                .map(|message| Issue {
                    path: "docker".into(),
                    kind: IssueKind::Io,
                    message: message.clone(),
                })
                .collect();
            if !json {
                for issue in &issues {
                    eprintln!("kuizer: {}", issue.message);
                }
            }
            let candidates: Vec<Candidate> = entries
                .into_iter()
                .map(|(_, candidate)| candidate)
                .collect();
            let selected = removal::select(candidates, &selections);
            let request = Request {
                command: "clean",
                policy: Some(name),
                roots: &[],
                mode: Source::Docker.mode(mode),
                yes: args.removal.yes,
                confirm: filters.config.delete.confirm,
                format,
                source: Source::Docker,
                engines: &filters.config.docker.engines,
                delete: &filters.config.delete,
            };
            removal::run(&request, selected, &issues)
        }
    }
}
