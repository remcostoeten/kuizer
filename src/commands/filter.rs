use super::AppError;
use super::scan::narrow;
use crate::cli::{ScopeArgs, SourceFilterArgs};
use crate::config::Config;
use crate::detect::{Category, builtin_rules};
use crate::scan::{Candidate, Scope};
use crate::units::ByteSize;

pub struct Filters {
    config: Config,
    evidence: Config,
    scope: Scope,
}

impl Filters {
    /// @name Filters::new
    /// @description Checks the `--rule` names against `known` and narrows `config` with the
    /// filter flags: categories intersect, the larger minimum size wins and `--exclude` globs are
    /// added to `scan.exclude`. Flags never widen the config. Evidence that protects an item,
    /// project activity and toolchain pins, is gathered without the added excludes, so `--idle`
    /// cannot match more and no pinned version looks unused because of `--exclude`.
    ///
    /// @example
    /// let filters = Filters::new(config, &args.categories, args.min_size, &args.scope, &names)?;
    pub fn new(
        config: &Config,
        categories: &[Category],
        min_size: Option<ByteSize>,
        args: &ScopeArgs,
        known: &[&str],
    ) -> Result<Self, AppError> {
        if let Some(name) = args
            .rules
            .iter()
            .find(|name| !known.contains(&name.as_str()))
        {
            return Err(AppError::UnknownRule {
                name: name.clone(),
                known: known.join(", "),
            });
        }
        let scope = Scope::new(&args.rules, &args.projects, &args.excludes)?
            .within(categories, min_size.map_or(0, |ByteSize(bytes)| bytes));
        let mut evidence = config.clone();
        narrow(&mut evidence, categories, min_size);
        let mut config = evidence.clone();
        config.scan.exclude.extend(args.excludes.iter().cloned());
        Ok(Self {
            config,
            evidence,
            scope,
        })
    }

    /// @name Filters::source
    /// @description Builds the filters of a `cache`, `toolchain` or `docker` listing.
    ///
    /// @example
    /// let filters = Filters::source(config, &args.filters, &names)?;
    pub fn source(
        config: &Config,
        args: &SourceFilterArgs,
        known: &[&str],
    ) -> Result<Self, AppError> {
        Self::new(config, &args.categories, args.min_size, &args.scope, known)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn evidence(&self) -> &Config {
        &self.evidence
    }

    pub fn matches(&self, candidate: &Candidate) -> bool {
        self.scope.matches(candidate)
    }

    pub fn keep<T>(&self, entries: &mut Vec<(T, Candidate)>) {
        entries.retain(|(_, candidate)| self.scope.matches(candidate));
    }
}

/// @name project_rules
/// @description Lists every project rule name the config knows, built-in and custom, whatever
/// the categories, so `--rule` next to `--category` narrows to nothing instead of failing.
///
/// @example
/// let names = project_rules(&config);
pub fn project_rules(config: &Config) -> Vec<String> {
    let mut names: Vec<String> = config
        .rules
        .iter()
        .map(|rule| rule.name.clone())
        .chain(
            config
                .detect
                .builtin_rules
                .then(builtin_rules)
                .into_iter()
                .flatten()
                .map(|rule| rule.name),
        )
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}
