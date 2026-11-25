use crate::config::Excludes;
use crate::detect::{Category, Safety};
use crate::scan::Candidate;
use globset::GlobBuilder;
use std::collections::BTreeMap;
use std::path::Path;

/// Shared candidate ordering used by the CLI, TUI and GUI.
///
/// @name SortKey
/// @description Selects the ordering rule for scan candidates.
///
/// @example
/// sort_candidates(&mut candidates, SortKey::Size);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum SortKey {
    #[default]
    Size,
    Path,
    Category,
    Idle,
}

/// A case-insensitive substring query shared by the frontends.
///
/// @name Query
/// @description Matches an item when one of its searchable fields contains the query.
///
/// @example
/// let query = Query::new("cargo");
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    needle: String,
}

impl Query {
    /// @name Query::new
    /// @description Builds a case-insensitive substring query. An empty query matches every item.
    ///
    /// @example
    /// let query = Query::new("cargo");
    pub fn new(needle: &str) -> Self {
        Self {
            needle: needle.to_lowercase(),
        }
    }

    /// @name Query::matches
    /// @description Returns whether the query is empty or one of the supplied fields contains it.
    ///
    /// @example
    /// if query.matches(&[path, rule_name]) { show(candidate); }
    pub fn matches(&self, fields: &[&str]) -> bool {
        self.needle.is_empty() || fields.iter().any(|field| contains(field, &self.needle))
    }
}

/// Filters candidates by query, category and safety.
///
/// @name CandidateFilter
/// @description Holds the shared conditions applied to scan candidates.
///
/// @example
/// let filter = CandidateFilter { query: &query, category: None, safety: None };
#[derive(Debug, Clone, Copy)]
pub struct CandidateFilter<'a> {
    pub query: &'a Query,
    pub category: Option<Category>,
    pub safety: Option<Safety>,
}

impl CandidateFilter<'_> {
    /// @name CandidateFilter::matches
    /// @description Applies the shared text, category and safety filters to a scan candidate.
    ///
    /// @example
    /// candidates.retain(|candidate| filter.matches(candidate));
    pub fn matches(&self, candidate: &Candidate) -> bool {
        self.category
            .is_none_or(|category| candidate.rule.category == category)
            && self
                .safety
                .is_none_or(|safety| candidate.rule.safety == safety)
            && self.query.matches(&[
                candidate.path.to_string_lossy().as_ref(),
                candidate.rule.name.as_str(),
            ])
    }
}

/// A glob given to `--project` or `--exclude` that does not compile.
///
/// @name ScopeError
/// @description Names the flag and the pattern so the CLI can report which value to fix.
///
/// @example
/// let error = Scope::new(&[], &["[".to_owned()], &[]).unwrap_err();
#[derive(Debug, thiserror::Error)]
#[error("invalid --{flag} pattern `{pattern}`: {message}")]
pub struct ScopeError {
    flag: &'static str,
    pattern: String,
    message: String,
}

/// The `--category`, `--min-size`, `--rule`, `--project` and `--exclude` filters, shared by
/// every source so `scan` lists exactly what `clean` considers.
///
/// @name Scope
/// @description Keeps a candidate only when it passes every filter that was given. Values of one
/// filter are a union, different filters intersect, and an empty filter keeps everything. A
/// candidate without a project never matches `--project`.
///
/// @example
/// let scope = Scope::new(&["node-modules".to_owned()], &[], &[])?;
/// candidates.retain(|candidate| scope.matches(candidate));
pub struct Scope {
    categories: Vec<Category>,
    min_size: u64,
    rules: Vec<String>,
    projects: Option<Excludes>,
    excludes: Option<Excludes>,
}

impl Scope {
    /// @name Scope::new
    /// @description Compiles the rule, project and exclude filters. Globs use the `scan.exclude`
    /// syntax: a pattern with a `/` matches a whole path, one without matches a folder name, and
    /// `~` expands to the home directory.
    ///
    /// @example
    /// let scope = Scope::new(&[], &["~/apps/*".to_owned()], &["vendor".to_owned()])?;
    pub fn new(
        rules: &[String],
        projects: &[String],
        excludes: &[String],
    ) -> Result<Self, ScopeError> {
        Ok(Self {
            categories: Vec::new(),
            min_size: 0,
            rules: rules.to_vec(),
            projects: globs("project", projects)?,
            excludes: globs("exclude", excludes)?,
        })
    }

    /// @name Scope::within
    /// @description Adds the `--category` and `--min-size` filters, for sources whose lister
    /// does not read them from the config.
    ///
    /// @example
    /// let scope = Scope::new(&rules, &projects, &excludes)?.within(&[Category::Cache], 1 << 20);
    pub fn within(mut self, categories: &[Category], min_size: u64) -> Self {
        self.categories = categories.to_vec();
        self.min_size = min_size;
        self
    }

    /// @name Scope::is_empty
    /// @description Returns whether no filter was given, so every candidate matches.
    ///
    /// @example
    /// let live = scope.is_empty() && !json;
    pub fn is_empty(&self) -> bool {
        self.categories.is_empty()
            && self.min_size == 0
            && self.rules.is_empty()
            && self.projects.is_none()
            && self.excludes.is_none()
    }

    /// @name Scope::matches
    /// @description Applies every given filter to one candidate. `--project` matches the
    /// candidate's project folder or a folder above it; `--exclude` drops a candidate whose path
    /// or a folder above it matches.
    ///
    /// @example
    /// entries.retain(|(_, candidate)| scope.matches(candidate));
    pub fn matches(&self, candidate: &Candidate) -> bool {
        (self.categories.is_empty() || self.categories.contains(&candidate.rule.category))
            && candidate.usage.allocated >= self.min_size
            && (self.rules.is_empty() || self.rules.contains(&candidate.rule.name))
            && self.projects.as_ref().is_none_or(|projects| {
                candidate
                    .project
                    .as_deref()
                    .is_some_and(|project| within_any(projects, project))
            })
            && self
                .excludes
                .as_ref()
                .is_none_or(|excludes| !within_any(excludes, &candidate.path))
    }
}

fn within_any(globs: &Excludes, path: &Path) -> bool {
    path.ancestors().any(|dir| globs.path(dir))
}

fn globs(flag: &'static str, patterns: &[String]) -> Result<Option<Excludes>, ScopeError> {
    if patterns.is_empty() {
        return Ok(None);
    }
    for pattern in patterns {
        GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(true)
            .build()
            .map_err(|error| ScopeError {
                flag,
                pattern: pattern.clone(),
                message: error.kind().to_string(),
            })?;
    }
    Excludes::new(patterns, &[])
        .map(Some)
        .map_err(|error| ScopeError {
            flag,
            pattern: patterns.join(", "),
            message: error.to_string(),
        })
}

/// Positions belonging to one ordered group key.
///
/// @name Group
/// @description Keeps grouping separate from the data a frontend renders.
///
/// @example
/// let groups = group_indices(&candidates, |candidate| candidate.rule.category);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group<K> {
    pub key: K,
    pub indices: Vec<usize>,
}

/// @name sort_candidates
/// @description Orders candidates with the shared size, path, category and idle rules.
///
/// @example
/// sort_candidates(&mut candidates, SortKey::Size);
pub fn sort_candidates(candidates: &mut [Candidate], key: SortKey) {
    match key {
        SortKey::Size => candidates.sort_by(|a, b| {
            b.usage
                .allocated
                .cmp(&a.usage.allocated)
                .then_with(|| a.path.cmp(&b.path))
        }),
        SortKey::Path => candidates.sort_by(|a, b| a.path.cmp(&b.path)),
        SortKey::Category => candidates.sort_by(|a, b| {
            a.rule
                .category
                .cmp(&b.rule.category)
                .then_with(|| b.usage.allocated.cmp(&a.usage.allocated))
                .then_with(|| a.path.cmp(&b.path))
        }),
        SortKey::Idle => candidates.sort_by(|a, b| {
            a.project_last_active
                .is_none()
                .cmp(&b.project_last_active.is_none())
                .then_with(|| a.project_last_active.cmp(&b.project_last_active))
                .then_with(|| b.usage.allocated.cmp(&a.usage.allocated))
                .then_with(|| a.path.cmp(&b.path))
        }),
    }
}

/// @name group_indices
/// @description Groups input positions by an ordered key, with keys sorted and indices kept in
/// input order so frontends can render the same group structure without moving their data.
///
/// @example
/// let groups = group_indices(&candidates, |candidate| candidate.rule.category);
pub fn group_indices<T, K: Ord>(items: &[T], key: impl Fn(&T) -> K) -> Vec<Group<K>> {
    let mut groups = BTreeMap::<K, Vec<usize>>::new();
    for (index, item) in items.iter().enumerate() {
        groups.entry(key(item)).or_default().push(index);
    }
    groups
        .into_iter()
        .map(|(key, indices)| Group { key, indices })
        .collect()
}

fn contains(field: &str, needle: &str) -> bool {
    if !field.is_ascii() || !needle.is_ascii() {
        return field.to_lowercase().contains(needle);
    }
    let (field, needle) = (field.as_bytes(), needle.as_bytes());
    let Some((&first, rest)) = needle.split_first() else {
        return true;
    };
    field.len() >= needle.len()
        && (0..=field.len() - needle.len()).any(|start| {
            field[start].to_ascii_lowercase() == first
                && field[start + 1..start + needle.len()].eq_ignore_ascii_case(rest)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::detect::{Kind, RuleInfo};
    use crate::scan::Usage;

    fn candidate(id: u64, path: &str, bytes: u64, category: Category) -> Candidate {
        Candidate {
            id,
            path: PathBuf::from(path),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: format!("rule-{id}"),
                category,
                safety: if id.is_multiple_of(2) {
                    Safety::Safe
                } else {
                    Safety::Review
                },
                ecosystem: String::new(),
            }),
            project: None,
            usage: Usage {
                allocated: bytes,
                ..Usage::default()
            },
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }

    #[test]
    fn filters_match_any_field_case_insensitively_and_keep_empty_results_empty() {
        let query = Query::new("cArGo");
        assert!(query.matches(&["/work/target", "Cargo"]));
        assert!(!query.matches(&["/work/target", "npm"]));
        assert!(Query::default().matches(&[]));
        assert!(!Query::new("absent").matches(&[]));
    }

    #[test]
    fn scope_filters_intersect_and_never_match_a_missing_project() {
        let mut web = candidate(2, "/work/web/node_modules", 10, Category::Dependencies);
        web.project = Some(PathBuf::from("/work/web"));
        let mut vendored = candidate(4, "/work/api/vendor/dist", 10, Category::Build);
        vendored.project = Some(PathBuf::from("/work/api"));
        let global = candidate(6, "/home/me/.npm", 10, Category::Cache);
        let all = [&web, &vendored, &global];
        let kept = |scope: &Scope| {
            all.iter()
                .filter(|item| scope.matches(item))
                .map(|item| item.id)
                .collect::<Vec<_>>()
        };
        let strings = |values: &[&str]| -> Vec<String> {
            values.iter().map(|value| (*value).to_owned()).collect()
        };

        assert_eq!(kept(&Scope::new(&[], &[], &[]).unwrap()), [2, 4, 6]);
        assert_eq!(
            kept(&Scope::new(&[], &strings(&["/work/*"]), &[]).unwrap()),
            [2, 4]
        );
        assert_eq!(
            kept(&Scope::new(&[], &[], &strings(&["vendor"])).unwrap()),
            [2, 6]
        );
        let narrow = Scope::new(&strings(&["rule-2", "rule-6"]), &strings(&["web"]), &[]).unwrap();
        assert_eq!(kept(&narrow), [2]);
        let sized = Scope::new(&[], &[], &[])
            .unwrap()
            .within(&[Category::Cache], 11);
        assert_eq!(kept(&sized), Vec::<u64>::new());
        assert!(Scope::new(&[], &strings(&["["]), &[]).is_err());
    }

    #[test]
    fn sorting_and_grouping_are_stable_for_equal_items_and_empty_input() {
        let mut candidates = vec![
            candidate(1, "/work/b", 10, Category::Cache),
            candidate(2, "/work/a", 10, Category::Build),
            candidate(3, "/work/c", 20, Category::Cache),
        ];
        sort_candidates(&mut candidates, SortKey::Size);
        assert_eq!(
            candidates.iter().map(|item| item.id).collect::<Vec<_>>(),
            [3, 2, 1]
        );
        let groups = group_indices(&candidates, |item| item.rule.category);
        assert_eq!(groups[0].key, Category::Build);
        assert_eq!(groups[0].indices, [1]);
        assert_eq!(groups[1].indices, [0, 2]);
        assert!(group_indices::<Candidate, Category>(&[], |item| item.rule.category).is_empty());
    }
}
