//! The Projects view: groups every project's artifacts, related global caches and git repository
//! together, with one action to clean that project's `safe` items.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

#[derive(Debug, Clone)]
pub(super) struct ProjectArtifact {
    pub id: u64,
    pub path: PathBuf,
    pub rule: String,
    pub safety: Safety,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub(super) struct ProjectCache {
    pub id: u64,
    pub name: String,
    pub tool: String,
    pub safety: Safety,
    pub bytes: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub(super) struct ProjectRepo {
    pub root: PathBuf,
    pub gc_gain: u64,
    pub collectable: bool,
    pub merged_count: usize,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ProjectView {
    pub root: PathBuf,
    pub ecosystems: Vec<String>,
    pub artifacts: Vec<ProjectArtifact>,
    pub caches: Vec<ProjectCache>,
    pub repo: Option<ProjectRepo>,
    pub total_bytes: u64,
    pub safe_ids: Vec<u64>,
    pub safe_bytes: u64,
    pub review_bytes: u64,
    pub growth_bytes: Option<i64>,
}

/// Maps a rule ecosystem name to the global cache tool names that store shared data for it.
fn related_cache_tools(ecosystem: &str) -> &'static [&'static str] {
    match ecosystem {
        "rust" => &["cargo", "sccache"],
        "node" | "nextjs" | "nuxt" | "sveltekit" | "turborepo" | "nx" | "parcel" | "vite"
        | "angular" => &["npm", "pnpm", "yarn", "node", "typescript", "bun"],
        "typescript" => &["typescript", "npm", "pnpm", "yarn", "node"],
        "bun" => &["bun"],
        "deno" => &["deno"],
        "python" | "jupyter" => &["python"],
        "go" => &["go"],
        "gradle" => &["gradle"],
        "maven" => &["maven"],
        "android" => &["android", "gradle"],
        "swift" | "cocoapods" => &["xcode"],
        "bazel" => &["bazel"],
        "php" => &["php"],
        _ => &[],
    }
}

/// Groups every artifact, related global cache and git repository in `report` under its project.
pub(super) fn build_projects(
    report: &Report,
    growth: Option<&scan::ScanDiff>,
    min_gain: u64,
) -> Vec<ProjectView> {
    #[derive(Default)]
    struct Builder {
        ecosystems: BTreeSet<String>,
        artifacts: Vec<ProjectArtifact>,
        repo: Option<ProjectRepo>,
        total_bytes: u64,
        safe_ids: Vec<u64>,
        safe_bytes: u64,
        review_bytes: u64,
    }

    let mut map: BTreeMap<PathBuf, Builder> = BTreeMap::new();

    for candidate in &report.candidates {
        let root = candidate
            .project
            .as_deref()
            .unwrap_or(candidate.path.as_path())
            .to_path_buf();
        let entry = map.entry(root).or_default();
        if !candidate.rule.ecosystem.is_empty() {
            entry.ecosystems.insert(candidate.rule.ecosystem.clone());
        }
        let bytes = candidate.usage.allocated;
        entry.total_bytes = entry.total_bytes.saturating_add(bytes);
        match candidate.rule.safety {
            Safety::Safe => {
                entry.safe_ids.push(candidate.id);
                entry.safe_bytes = entry.safe_bytes.saturating_add(bytes);
            }
            Safety::Review => {
                entry.review_bytes = entry.review_bytes.saturating_add(bytes);
            }
        }
        entry.artifacts.push(ProjectArtifact {
            id: candidate.id,
            path: candidate.path.clone(),
            rule: candidate.rule.name.clone(),
            safety: candidate.rule.safety,
            bytes,
        });
    }

    for repo in &report.repos {
        let summary = ProjectRepo {
            root: repo.root.clone(),
            gc_gain: repo.objects.gain(),
            collectable: repo.collectable(min_gain),
            merged_count: repo.merged.len(),
            busy: repo.busy.is_some(),
        };
        if let Some(entry) = map.get_mut(&repo.root) {
            entry.repo = Some(summary);
        } else {
            map.entry(repo.root.clone()).or_default().repo = Some(summary);
        }
    }

    let mut projects: Vec<ProjectView> = map
        .into_iter()
        .map(|(root, mut builder)| {
            builder
                .artifacts
                .sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
            let mut tools: BTreeSet<&str> = BTreeSet::new();
            for ecosystem in &builder.ecosystems {
                tools.insert(ecosystem.as_str());
                for &tool in related_cache_tools(ecosystem.as_str()) {
                    tools.insert(tool);
                }
            }
            let mut seen_caches: HashSet<u64> = HashSet::new();
            let mut caches: Vec<ProjectCache> = report
                .caches
                .iter()
                .filter(|(cache, candidate)| {
                    tools.contains(cache.tool.as_str()) && seen_caches.insert(candidate.id)
                })
                .map(|(cache, candidate)| ProjectCache {
                    id: candidate.id,
                    name: cache.name.clone(),
                    tool: cache.tool.clone(),
                    safety: candidate.rule.safety,
                    bytes: candidate.usage.allocated,
                    path: candidate.path.clone(),
                })
                .collect();
            caches.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));

            let growth_bytes = growth
                .and_then(|diff| diff.growth_for(&root))
                .map(|project| project.delta_bytes)
                .filter(|&delta| delta > 0);

            ProjectView {
                root,
                ecosystems: builder.ecosystems.into_iter().collect(),
                artifacts: builder.artifacts,
                caches,
                repo: builder.repo,
                total_bytes: builder.total_bytes,
                safe_ids: builder.safe_ids,
                safe_bytes: builder.safe_bytes,
                review_bytes: builder.review_bytes,
                growth_bytes,
            }
        })
        .collect();

    projects.sort_by(|a, b| {
        b.total_bytes
            .cmp(&a.total_bytes)
            .then_with(|| {
                let a_gc = a.repo.as_ref().map_or(0, |r| r.gc_gain);
                let b_gc = b.repo.as_ref().map_or(0, |r| r.gc_gain);
                b_gc.cmp(&a_gc)
            })
            .then_with(|| a.root.cmp(&b.root))
    });
    projects
}

impl Kuizer {
    /// Cleans every `Safety::Safe` artifact belonging to `project`, leaving review items, shared
    /// global caches and other projects untouched.
    pub(super) fn clean_project_safe(&mut self, project: &Path, cx: &mut Context<Self>) {
        let Some(report) = self.report() else {
            return;
        };
        let keys: Vec<(Source, u64)> = report
            .candidates
            .iter()
            .filter(|candidate| {
                candidate.rule.safety == Safety::Safe
                    && candidate
                        .project
                        .as_deref()
                        .unwrap_or(candidate.path.as_path())
                        == project
            })
            .map(|candidate| (Source::Artifact, candidate.id))
            .collect();
        self.delete_keys(&keys, cx);
    }

    pub(super) fn render_projects(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let growth = self.session.growth.as_ref().map(|(_, diff)| diff);
        let projects = build_projects(report, growth, self.config.git.min_gain.0);
        let safe_total: u64 = projects.iter().map(|project| project.safe_bytes).sum();
        let lock = self
            .locked(std::iter::once(Source::Artifact))
            .or_else(|| self.busy().then_some("Another action is already running"))
            .or_else(|| {
                self.saving_protected_paths()
                    .then_some("Protected paths are being saved")
            });
        let trash = self.config.delete.mode == DeleteMode::Trash;

        let header = div()
            .flex()
            .items_center()
            .gap(unit(space::L))
            .child(self.overview_switch(cx))
            .child(div().flex_1())
            .child(
                ui::tabular(div())
                    .text_size(unit(font::SMALL))
                    .text_color(theme::muted())
                    .child(format!(
                        "{} · {} safe to clean",
                        plural(projects.len(), "project"),
                        theme::bytes(safe_total),
                    )),
            );

        let body = scroll("projects")
            .debug_selector(|| "projects-view".to_owned())
            .child(header);

        if projects.is_empty() {
            return body
                .child(ui::card("Projects").child(ui::empty("No projects found in this scan")))
                .into_any_element();
        }

        let cards = projects
            .into_iter()
            .enumerate()
            .map(|(index, project)| self.project_card(index, project, report, lock, trash, cx));

        body.children(cards).into_any_element()
    }

    fn project_card(
        &self,
        index: usize,
        project: ProjectView,
        report: &Report,
        lock: Option<&'static str>,
        trash: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let title = model::short_path(&project.root, &report.roots);
        let can_clean = !project.safe_ids.is_empty() && lock.is_none();
        let clean_kind = if trash {
            ButtonKind::Secondary
        } else {
            ButtonKind::Danger
        };
        let clean_label = if project.safe_ids.is_empty() {
            "No safe items".to_owned()
        } else if trash {
            format!("Clean safe · {}", theme::bytes(project.safe_bytes))
        } else {
            format!("Delete safe · {}", theme::bytes(project.safe_bytes))
        };
        let clean_tip = if let Some(reason) = lock {
            reason.to_owned()
        } else if project.safe_ids.is_empty() {
            "This project has no safe artifacts to remove".to_owned()
        } else if trash {
            format!(
                "Move {} ({}) from {} to the trash. Needs-review items and shared caches stay untouched",
                plural(project.safe_ids.len(), "safe item"),
                theme::bytes(project.safe_bytes),
                title,
            )
        } else {
            format!(
                "Permanently delete {} ({}) from {}. Needs-review items and shared caches stay untouched",
                plural(project.safe_ids.len(), "safe item"),
                theme::bytes(project.safe_bytes),
                title,
            )
        };
        let target_root = project.root.clone();
        let clean_button = self
            .button(
                ("clean-project", index),
                clean_label,
                clean_kind,
                can_clean,
                cx,
                move |app, _, cx| app.clean_project_safe(&target_root, cx),
            )
            .debug_selector(move || format!("clean-project-{index}"))
            .h(unit(size::SEGMENT))
            .px(unit(space::M))
            .text_size(unit(font::SMALL))
            .tip(tooltip::plain(clean_tip));

        let header = div()
            .flex()
            .items_center()
            .gap(unit(space::M))
            .child(
                div()
                    .id(("project-title", index))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(unit(space::S))
                    .tip(tooltip::path(&project.root))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(title.clone()),
                    )
                    .children(project.ecosystems.iter().enumerate().map(|(eco_i, eco)| {
                        ui::tag(
                            ("project-eco", index * 32 + eco_i),
                            eco.clone(),
                            ui::Tone::Neutral,
                        )
                    }))
                    .children((project.review_bytes > 0).then(|| {
                        ui::tag(
                            ("project-review", index),
                            format!("{} review", theme::bytes(project.review_bytes)),
                            ui::Tone::Review,
                        )
                        .tip(tooltip::plain(
                            "Needs-review artifacts in this project are left untouched by Clean safe",
                        ))
                    }))
                    .children(project.growth_bytes.map(|delta| {
                        let bytes = u64::try_from(delta).unwrap_or(0);
                        ui::tag(
                            ("project-growth", index),
                            format!("+{}", theme::bytes(bytes)),
                            ui::Tone::Review,
                        )
                        .tip(tooltip::plain(
                            "Grew since the previous scan of this folder",
                        ))
                    })),
            )
            .child(ui::figure(theme::bytes(project.total_bytes)).text_color(theme::text()))
            .child(clean_button);

        let artifacts_section = (!project.artifacts.is_empty()).then(|| {
            let root = project.root.clone();
            div().flex().flex_col().gap(unit(space::XXS)).children(
                project.artifacts.iter().enumerate().map(|(item_i, item)| {
                    let rel = item
                        .path
                        .strip_prefix(&root)
                        .ok()
                        .filter(|p| !p.as_os_str().is_empty())
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| model::short_path(&item.path, &report.roots));
                    let deleting = self.session.cleaning.as_ref().is_some_and(|cleaning| {
                        cleaning
                            .paths
                            .get(&Source::Artifact)
                            .is_some_and(|paths| paths.contains(&item.path))
                    });
                    let review = item.safety == Safety::Review;
                    let key = (Source::Artifact, item.id);
                    let selected = self.list.selection.contains(&key);
                    let row = div()
                        .id(("project-artifact", index * 1024 + item_i))
                        .flex()
                        .items_center()
                        .gap(unit(space::M))
                        .h(unit(size::row()))
                        .px(unit(space::S))
                        .rounded(theme::radius_control())
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::hover_row()))
                        .when(deleting, |this| this.opacity(theme::DISABLED))
                        .tip(tooltip::path(&item.path))
                        .child(if deleting {
                            ui::deleting_mark()
                        } else {
                            ui::checkbox(selected)
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme::text())
                                .child(rel),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::faint())
                                .child(item.rule.clone()),
                        )
                        .child(ui::safety_label(
                            ("project-artifact-safety", index * 1024 + item_i),
                            review,
                        ))
                        .child(ui::figure(theme::bytes(item.bytes)).w(unit(col::M)));
                    self.control(row, cx, move |app, _, cx| {
                        app.toggle(key.0, key.1, cx);
                    })
                }),
            )
        });

        let caches_section = (!project.caches.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(unit(space::XS))
                .pt(unit(space::S))
                .border_t_1()
                .border_color(theme::border())
                .child(ui::caption("Related global caches (shared)"))
                .children(project.caches.iter().enumerate().map(|(cache_i, cache)| {
                    let review = cache.safety == Safety::Review;
                    let cache_id = cache.id;
                    let cache_path = cache.path.clone();
                    let row = div()
                        .id(("project-cache", index * 256 + cache_i))
                        .flex()
                        .items_center()
                        .gap(unit(space::M))
                        .h(unit(size::SEGMENT))
                        .px(unit(space::S))
                        .rounded(theme::radius_control())
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::hover_row()))
                        .tip(tooltip::path(&cache.path))
                        .child(icon::sized(Icon::Caches, theme::faint(), size::GLYPH))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::muted())
                                .child(format!("{} ({})", cache.name, cache.tool)),
                        )
                        .child(ui::safety_label(
                            ("project-cache-safety", index * 256 + cache_i),
                            review,
                        ))
                        .child(
                            ui::figure(theme::bytes(cache.bytes))
                                .w(unit(col::M))
                                .text_size(unit(font::SMALL))
                                .text_color(theme::muted()),
                        );
                    self.control(row, cx, move |app, _, cx| {
                        app.list.cursors.insert(
                            Tab::Caches,
                            Target::Item(Source::Cache, cache_id, cache_path.clone()),
                        );
                        app.go_to(Tab::Caches, cx);
                    })
                }))
        });

        let repo_section = project.repo.map(|repo| {
            let mut parts = Vec::new();
            if repo.collectable {
                parts.push(format!("git gc ~{}", theme::bytes(repo.gc_gain)));
            }
            if repo.merged_count > 0 {
                parts.push(plural(repo.merged_count, "merged branch"));
            }
            if parts.is_empty() {
                parts.push("clean repository".to_owned());
            }
            if repo.busy {
                parts.push("git operation in progress".to_owned());
            }
            let repo_root = repo.root.clone();
            let row = div()
                .id(("project-repo", index))
                .flex()
                .items_center()
                .gap(unit(space::M))
                .h(unit(size::SEGMENT))
                .px(unit(space::S))
                .rounded(theme::radius_control())
                .cursor_pointer()
                .hover(|style| style.bg(theme::hover_row()))
                .tip(tooltip::plain("Open in Git repositories"))
                .child(icon::sized(Icon::Repos, theme::faint(), size::GLYPH))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(unit(font::SMALL))
                        .text_color(theme::muted())
                        .child(parts.join(" · ")),
                )
                .child(
                    div()
                        .text_size(unit(font::SMALL))
                        .text_color(theme::faint())
                        .child("Git repositories →"),
                );
            div()
                .flex()
                .flex_col()
                .gap(unit(space::XS))
                .pt(unit(space::S))
                .border_t_1()
                .border_color(theme::border())
                .child(ui::caption("Git repository"))
                .child(self.control(row, cx, move |app, _, cx| {
                    app.list
                        .cursors
                        .insert(Tab::Repos, Target::Repo(repo_root.clone()));
                    app.go_to(Tab::Repos, cx);
                }))
        });

        div()
            .debug_selector(move || format!("project-card-{index}"))
            .flex()
            .flex_col()
            .min_w_0()
            .gap(unit(space::M))
            .p(unit(space::GUTTER))
            .rounded(theme::radius_panel())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border())
            .child(header)
            .children(artifacts_section)
            .children(caches_section)
            .children(repo_section)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kuizer::caches::KnownCache;
    use kuizer::detect::{Category, Kind, RuleInfo, Safety};
    use kuizer::repos::{Branch, Objects, Repository};
    use kuizer::scan::{Candidate, Usage};

    use super::*;

    fn candidate(
        id: u64,
        path: &str,
        project: &str,
        ecosystem: &str,
        safety: Safety,
        bytes: u64,
    ) -> Candidate {
        Candidate {
            id,
            path: PathBuf::from(path),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: format!("{ecosystem}-artifact"),
                category: Category::Build,
                safety,
                ecosystem: ecosystem.to_owned(),
            }),
            usage: Usage {
                apparent: bytes,
                allocated: bytes,
                entries: 1,
                partial: false,
            },
            newest: None,
            identity: None,
            project: Some(PathBuf::from(project)),
            project_last_active: None,
        }
    }

    #[test]
    fn groups_artifacts_related_caches_and_repos_under_projects() {
        let roots = vec![PathBuf::from("/work")];
        let mut report = Report::empty(&roots, &ScanOptions::from_config(&Config::default()));
        report.candidates = vec![
            candidate(
                1,
                "/work/api/target",
                "/work/api",
                "rust",
                Safety::Safe,
                400_000,
            ),
            candidate(
                2,
                "/work/api/.env-cache",
                "/work/api",
                "rust",
                Safety::Review,
                50_000,
            ),
            candidate(
                3,
                "/work/web/node_modules",
                "/work/web",
                "node",
                Safety::Safe,
                200_000,
            ),
        ];
        report.caches = vec![
            (
                KnownCache {
                    name: "cargo-registry".to_owned(),
                    tool: "cargo".to_owned(),
                    group: kuizer::catalog::Group::Developer,
                    category: Category::Cache,
                    kind: Kind::Dir,
                    path: PathBuf::from("/home/u/.cargo/registry"),
                    safety: Safety::Safe,
                    note: String::new(),
                    processes: Vec::new(),
                },
                candidate(
                    10,
                    "/home/u/.cargo/registry",
                    "/home/u/.cargo/registry",
                    "cargo",
                    Safety::Safe,
                    900_000,
                ),
            ),
            (
                KnownCache {
                    name: "npm".to_owned(),
                    tool: "npm".to_owned(),
                    group: kuizer::catalog::Group::Developer,
                    category: Category::Cache,
                    kind: Kind::Dir,
                    path: PathBuf::from("/home/u/.npm/_cacache"),
                    safety: Safety::Safe,
                    note: String::new(),
                    processes: Vec::new(),
                },
                candidate(
                    11,
                    "/home/u/.npm/_cacache",
                    "/home/u/.npm/_cacache",
                    "npm",
                    Safety::Safe,
                    300_000,
                ),
            ),
        ];
        report.repos = vec![
            Repository {
                root: PathBuf::from("/work/api"),
                git_dir: PathBuf::from("/work/api/.git"),
                objects: Objects {
                    loose: 100,
                    loose_bytes: 20 * 1024 * 1024,
                    packs: 1,
                    packed_bytes: 5 * 1024 * 1024,
                    prunable: 0,
                    garbage_bytes: 0,
                },
                default_branch: Some("main".to_owned()),
                current_branch: Some("main".to_owned()),
                merged: vec![Branch {
                    name: "old-feature".to_owned(),
                    commit: "abc123".to_owned(),
                }],
                busy: None,
            },
            Repository {
                root: PathBuf::from("/work/docs"),
                git_dir: PathBuf::from("/work/docs/.git"),
                objects: Objects {
                    loose: 10,
                    loose_bytes: 15 * 1024 * 1024,
                    packs: 0,
                    packed_bytes: 0,
                    prunable: 0,
                    garbage_bytes: 0,
                },
                default_branch: Some("main".to_owned()),
                current_branch: Some("main".to_owned()),
                merged: Vec::new(),
                busy: None,
            },
        ];

        let projects = build_projects(&report, None, 10 * 1024 * 1024);
        assert_eq!(projects.len(), 3);

        // Sorted by total artifact bytes descending: api (450k), web (200k), docs (0 + repo).
        let api = &projects[0];
        assert_eq!(api.root, PathBuf::from("/work/api"));
        assert_eq!(api.artifacts.len(), 2);
        assert_eq!(api.safe_ids, vec![1]);
        assert_eq!(api.safe_bytes, 400_000);
        assert_eq!(api.review_bytes, 50_000);
        assert_eq!(api.caches.len(), 1);
        assert_eq!(api.caches[0].name, "cargo-registry");
        let api_repo = api.repo.as_ref().expect("api repo attached");
        assert!(api_repo.collectable);
        assert_eq!(api_repo.merged_count, 1);

        let web = &projects[1];
        assert_eq!(web.root, PathBuf::from("/work/web"));
        assert_eq!(web.safe_ids, vec![3]);
        assert_eq!(web.caches.len(), 1);
        assert_eq!(web.caches[0].name, "npm");
        assert!(web.repo.is_none());

        let docs = &projects[2];
        assert_eq!(docs.root, PathBuf::from("/work/docs"));
        assert!(docs.artifacts.is_empty());
        assert!(docs.safe_ids.is_empty());
        assert!(docs.repo.as_ref().expect("docs repo attached").collectable);
    }
}
