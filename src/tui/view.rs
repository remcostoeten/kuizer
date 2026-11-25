use std::borrow::Cow;
use std::path::Path;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, Paragraph, Row as TableRow, Table, TableState,
};

use super::app::{
    App, ConfirmStats, GitScope, GitTask, Group, Grouping, HistorySpot, Item, Load, Overlay, Row,
    Spot, Status, Tab, counted, project_of,
};
use super::palette::{
    self, ACCENT, CURSOR, DANGER, FAINT, KEY_BG, KEY_FG, MUTED, ON_FILL, SAFE, SEARCH_FILL,
    SELECTED, WARNING,
};
use crate::cli::SortKey;
use crate::delete::{Confirmation, Mode};
use crate::detect::{Category, Safety};
use crate::dupes::Stage;
use crate::history::{self, Action as HistoryAction, ItemState, TrashState};
use crate::keymap::{Action, Scope};
use crate::output::display_path;
use crate::scan::IssueKind;
use crate::source::Source;
use crate::units::{ByteSize, age, since};

const BAR: u16 = 8;
const SHOWN_REPOS: usize = 5;
pub(super) const CURRENT_WIDTH: usize = 48;
type HelpRow = (&'static [Action], &'static str);
type HelpSection = (&'static str, Scope, &'static [HelpRow]);

const FIXED_COLUMNS: u16 = 3 + 10 + 9 + 8 + 12 + 20 + 6 + 8;

pub(super) fn shorten(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width || width < 2 {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count - (width - 1)).collect();
    format!("…{tail}")
}

fn size_bar(bytes: u64, largest: u64) -> String {
    let width = usize::from(BAR);
    let filled = match largest {
        0 => 0,
        largest => (u128::from(bytes) * u128::from(BAR))
            .div_ceil(u128::from(largest))
            .try_into()
            .unwrap_or(width),
    };
    format!(
        "{}{}",
        "█".repeat(filled.min(width)),
        "░".repeat(width - filled.min(width))
    )
}

fn explain(item: &Item) -> &'static str {
    let rule = &item.candidate.rule;
    match (item.source, rule.category, rule.safety) {
        (Source::Toolchain, _, Safety::Safe) => {
            "A version nothing uses. Deleting is permanent, the tool installs it again on demand."
        }
        (Source::Toolchain, _, Safety::Review) => {
            "Check first: this version is the default or a project pins it."
        }
        (Source::Docker, _, Safety::Safe) => {
            "Removed through the engine for good. Pull or build it again when needed."
        }
        (Source::Docker, _, Safety::Review) => {
            "Check first: this object is recent or may still be in use."
        }
        (_, Category::Dependencies, Safety::Safe) => {
            "Installed packages. Deleting is safe, reinstall with your package manager."
        }
        (_, Category::Build, Safety::Safe) => {
            "Build output. Deleting is safe, the next build recreates it."
        }
        (_, Category::Cache, Safety::Safe) => "Cache. Deleting is safe, it is rebuilt when needed.",
        (_, Category::Temp, Safety::Safe) => "Temporary files. Deleting is safe.",
        (_, Category::Logs, Safety::Safe) => {
            "Log files. Deleting is safe, programs start new ones."
        }
        (_, Category::Trash, Safety::Safe) => {
            "Items already in the trash. Emptying it cannot be undone."
        }
        (_, Category::Files, Safety::Safe) => {
            "Large file. Look before deleting, it cannot be regenerated."
        }
        (_, _, Safety::Review) => {
            "Check first: this may hold data that cannot be regenerated. Look inside before deleting."
        }
    }
}

fn filters(app: &App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if app.tab.source().is_none() {
        if !app.filter.is_empty() {
            spans.push(Span::styled(
                format!(" \"{}\" ", app.filter),
                Style::new().fg(ON_FILL).bg(SEARCH_FILL),
            ));
        }
        return spans;
    }
    if let Some(category) = app.category {
        spans.push(Span::styled(
            format!(" {} ", category.as_str()),
            Style::new().fg(ON_FILL).bg(palette::category(category)),
        ));
        spans.push(Span::raw(" "));
    }
    if app.safe_only {
        spans.push(Span::styled(
            " safe only ",
            Style::new().fg(ON_FILL).bg(SAFE),
        ));
        spans.push(Span::raw(" "));
    }
    if !app.filter.is_empty() {
        spans.push(Span::styled(
            format!(" \"{}\" ", app.filter),
            Style::new().fg(ON_FILL).bg(SEARCH_FILL),
        ));
        spans.push(Span::raw(" "));
    }
    spans
}

fn sort_label(sort: SortKey) -> &'static str {
    match sort {
        SortKey::Size => "size",
        SortKey::Path => "path",
        SortKey::Category => "category",
        SortKey::Idle => "idle",
    }
}

fn header(app: &App) -> Line<'static> {
    let state = match &app.finished {
        Some(summary) => format!("done in {:.1}s", summary.duration.as_secs_f64()),
        None => "scanning".to_owned(),
    };
    let mut spans = vec![
        Span::styled(" kuizer ", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(
                " {state}  {} dirs  {} files  {} issues ",
                app.progress.dirs,
                app.progress.files,
                app.issues.entries.len()
            ),
            Style::new().fg(MUTED),
        ),
    ];
    spans.extend(summary(app));
    spans.extend(filters(app));
    spans.extend(disk(app));
    if app.finished.is_none()
        && let Some(folder) = &app.current
    {
        spans.push(Span::styled(
            format!(" in {} ", shorten(&display_path(folder), CURRENT_WIDTH)),
            Style::new().fg(MUTED),
        ));
    }
    Line::from(spans)
}

fn summary(app: &App) -> Vec<Span<'static>> {
    match app.tab {
        Tab::Artifacts | Tab::Caches | Tab::Toolchains | Tab::Docker => {
            let mut spans = vec![
                Span::raw(format!(
                    " {} found  {} ",
                    app.total_items(),
                    ByteSize(app.total_bytes())
                )),
                Span::styled(
                    format!(" sort {} ", sort_label(app.sort)),
                    Style::new().fg(MUTED),
                ),
            ];
            if app.tab == Tab::Artifacts && app.grouping == Grouping::Project {
                spans.push(Span::styled(" by project ", Style::new().fg(MUTED)));
            }
            spans
        }
        Tab::Duplicates if app.duplicates.load == Load::Done => {
            let found = &app.duplicates.found;
            vec![Span::raw(format!(
                " {} groups  {} reclaimable ",
                found.groups.len(),
                ByteSize(found.reclaimable())
            ))]
        }
        Tab::Duplicates => vec![Span::styled(
            format!(" {} ", duplicate_stage(app.duplicates.stage)),
            Style::new().fg(MUTED),
        )],
        Tab::Issues => vec![Span::raw(format!(
            " {} unreadable or skipped ",
            app.issues.entries.len()
        ))],
        Tab::Repos if app.repos.load == Load::Done => {
            let found = &app.repos.found;
            let gain: u64 = found
                .iter()
                .map(|repository| repository.objects.gain())
                .sum();
            let merged: usize = found.iter().map(|repository| repository.merged.len()).sum();
            vec![Span::raw(format!(
                " {}  {} to pack  {} ",
                counted(found.len(), "repository", "repositories"),
                ByteSize(gain),
                counted(merged, "merged branch", "merged branches")
            ))]
        }
        Tab::Repos => vec![Span::styled(
            " reading repositories ",
            Style::new().fg(MUTED),
        )],
        Tab::History if app.history.load == Load::Done => {
            let history = &app.history.history;
            let trash = &app.history.trash;
            let mut text = format!(
                " {}  {} removed ",
                counted(history.runs.len(), "run", "runs"),
                ByteSize(history.cumulative.bytes),
            );
            if trash.known {
                let kept = trash.in_trash(history);
                if kept.items > 0 {
                    text.push_str(&format!(" {} in trash ", ByteSize(kept.bytes)));
                }
            }
            vec![Span::raw(text)]
        }
        Tab::History => vec![Span::styled(" loading history ", Style::new().fg(MUTED))],
    }
}

fn duplicate_stage(stage: Option<Stage>) -> String {
    match stage {
        None => "looking for duplicates".to_owned(),
        Some(Stage::Walking { files }) => format!("walking {files} files"),
        Some(Stage::Hashing { files }) => format!("hashing {files} files"),
        Some(Stage::Verifying { groups }) => format!("comparing bytes in {groups} groups"),
    }
}

fn tabs(app: &App) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for tab in Tab::ALL {
        let key = app.keymap.keys(Scope::Tui, tab.action()).next();
        let count = match tab {
            Tab::Issues if !app.issues.entries.is_empty() => {
                format!(" {}", app.issues.entries.len())
            }
            _ => String::new(),
        };
        let style = if tab == app.tab {
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(MUTED)
        };
        if let Some(key) = key {
            spans.push(key_chip(key));
        }
        spans.push(Span::styled(format!(" {}{count}  ", tab.label()), style));
    }
    Line::from(spans)
}

fn disk(app: &App) -> Vec<Span<'static>> {
    let Some(free) = app.free else {
        return Vec::new();
    };
    let mut spans = vec![Span::raw(format!(" {} free ", ByteSize(free)))];
    let selected = app.selected_bytes();
    if selected == 0 {
        return spans;
    }
    let note = match (app.mode, app.free_after(selected, app.mode)) {
        (Mode::Permanent, Some(after)) => format!("→ {} after delete ", ByteSize(after)),
        (Mode::Trash, _) => "selection stays on disk in the trash ".to_owned(),
        _ => return spans,
    };
    spans.push(Span::styled(note, Style::new().fg(MUTED)));
    spans
}

fn group_row(app: &App, group: &Group, path_width: usize) -> TableRow<'static> {
    let count = group.count;
    let summary = format!(
        "  · {count} item{}  · {}",
        if count == 1 { "" } else { "s" },
        ByteSize(group.bytes),
    );
    let candidate = &app.items[group.first].candidate;
    let warning = app
        .changes_in(candidate)
        .filter(|_| app.grouping == Grouping::Project)
        .map(|changes| format!("  · {}", uncommitted(changes)));
    let reserved = summary.chars().count() + warning.as_ref().map_or(0, |w| w.chars().count());
    let name = match group.category {
        Some(category) => format!("{} / {}", category.as_str(), group.name),
        None => shorten(
            &display_path(project_of(candidate)),
            path_width.saturating_sub(reserved),
        ),
    };
    let mut title = vec![Span::raw(format!("{name}{summary}"))];
    if let Some(warning) = warning {
        title.push(Span::styled(warning, Style::new().fg(WARNING)));
    }
    let mut cells = vec![Cell::from(""); 7];
    cells.push(Cell::from(Line::from(title)));
    TableRow::new(cells).style(Style::new().fg(MUTED).add_modifier(Modifier::BOLD))
}

fn item_row(item: &Item, largest: u64, path_width: usize) -> TableRow<'static> {
    let candidate = &item.candidate;
    let mark = match (&item.status, item.selected) {
        (Status::Deleted, _) => "  ✓",
        (Status::Deleting, _) => "  …",
        (Status::Excluded, _) => "  ×",
        (Status::Failed(_), _) => "  !",
        (Status::Ready, true) => "[x]",
        (Status::Ready, false) => "[ ]",
    };
    let style = match (&item.status, candidate.rule.safety) {
        (Status::Deleted | Status::Excluded, _) => {
            Style::new().fg(FAINT).add_modifier(Modifier::CROSSED_OUT)
        }
        (Status::Deleting | Status::Failed(_), _) => Style::new().fg(MUTED),
        (Status::Ready, _) if item.selected => Style::new().bg(SELECTED),
        (Status::Ready, _) => Style::new(),
    };
    let ready = item.status == Status::Ready;
    let dim_if = |color: Color| if ready { color } else { MUTED };
    let (badge, badge_color) = match candidate.rule.safety {
        Safety::Safe => ("● safe", SAFE),
        Safety::Review => ("▲ check", WARNING),
    };
    let bytes = candidate.usage.allocated;
    TableRow::new(vec![
        Cell::from(mark),
        Cell::from(format!("{:>10}", ByteSize(bytes).to_string()))
            .style(Style::new().fg(dim_if(palette::size(bytes)))),
        Cell::from(size_bar(bytes, largest)).style(Style::new().fg(FAINT)),
        Cell::from(badge).style(Style::new().fg(dim_if(badge_color))),
        Cell::from(candidate.rule.category.as_str())
            .style(Style::new().fg(dim_if(palette::category(candidate.rule.category)))),
        Cell::from(candidate.rule.name.clone()),
        Cell::from(age(candidate.newest)).style(Style::new().fg(MUTED)),
        Cell::from(shorten(&shown_path(item), path_width)),
    ])
    .style(style)
}

fn shown_path(item: &Item) -> String {
    if item.label.is_empty() {
        display_path(&item.candidate.path)
    } else {
        item.label.clone()
    }
}

fn window(cursor: usize, rows: usize, height: usize) -> (usize, usize) {
    let start = (cursor + 1).saturating_sub(height.max(1));
    (start, (start + height).min(rows))
}

fn report_table(frame: &mut Frame, rows: Vec<TableRow<'static>>, cursor: usize, area: Rect) {
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Fill(1),
        ],
    )
    .row_highlight_style(Style::new().bg(CURSOR))
    .column_spacing(1);
    let mut state = TableState::default().with_selected(Some(cursor));
    frame.render_stateful_widget(table, area, &mut state);
}

fn duplicates_table(frame: &mut Frame, app: &App, area: Rect) {
    let list = &app.duplicates;
    let height = usize::from(area.height);
    let path_width = usize::from(area.width.saturating_sub(10 + 16 + 2));
    let (start, stop) = window(list.cursor, list.rows.len(), height);
    let rows = list.rows[start.min(stop)..stop]
        .iter()
        .map(|spot| match *spot {
            Spot::Group(group) => {
                let group = &list.found.groups[group];
                TableRow::new(vec![
                    Cell::from(format!("{:>10}", ByteSize(group.size).to_string())),
                    Cell::from(format!("{} copies", group.members.len())),
                    Cell::from(format!("{} reclaimable", ByteSize(group.reclaimable()))),
                ])
                .style(Style::new().fg(MUTED).add_modifier(Modifier::BOLD))
            }
            Spot::Member(group, member) => {
                let member = &list.found.groups[group].members[member];
                TableRow::new(vec![
                    Cell::from(""),
                    Cell::from(age(member.modified)).style(Style::new().fg(MUTED)),
                    Cell::from(shorten(&display_path(&member.path), path_width)),
                ])
            }
        })
        .collect();
    report_table(frame, rows, list.cursor - start, area);
}

fn issue_label(kind: IssueKind) -> &'static str {
    match kind {
        IssueKind::PermissionDenied => "permission denied",
        IssueKind::Vanished => "vanished",
        IssueKind::Io => "read error",
        IssueKind::SkippedMount => "skipped mount",
        IssueKind::InvalidRoot => "invalid root",
        IssueKind::Protected => "protected",
    }
}

fn issues_table(frame: &mut Frame, app: &App, area: Rect) {
    let list = &app.issues;
    let height = usize::from(area.height);
    let path_width = usize::from(area.width.saturating_sub(10 + 16 + 2));
    let (start, stop) = window(list.cursor, list.rows.len(), height);
    let rows = list.rows[start.min(stop)..stop]
        .iter()
        .map(|&index| {
            let issue = &list.entries[index];
            TableRow::new(vec![
                Cell::from(""),
                Cell::from(issue_label(issue.kind)).style(Style::new().fg(WARNING)),
                Cell::from(shorten(&display_path(&issue.path), path_width)),
            ])
        })
        .collect();
    report_table(frame, rows, list.cursor - start, area);
}

fn repos_table(frame: &mut Frame, app: &App, area: Rect) {
    let list = &app.repos;
    let height = usize::from(area.height);
    let path_width = usize::from(area.width.saturating_sub(10 + 16 + 2));
    let (start, stop) = window(list.cursor, list.rows.len(), height);
    let rows = list.rows[start.min(stop)..stop]
        .iter()
        .map(|&index| {
            let repository = &list.found[index];
            let gain = repository.objects.gain();
            let state = if list.running.contains(&repository.root) {
                Cell::from("running git").style(Style::new().fg(MUTED))
            } else if repository.busy.is_some() {
                Cell::from("busy").style(Style::new().fg(WARNING))
            } else if repository.merged.is_empty() {
                Cell::from("")
            } else {
                Cell::from(format!("{} merged", repository.merged.len()))
                    .style(Style::new().fg(ACCENT))
            };
            TableRow::new(vec![
                Cell::from(format!("{:>10}", ByteSize(gain).to_string()))
                    .style(Style::new().fg(palette::size(gain))),
                state,
                Cell::from(shorten(&display_path(&repository.root), path_width)),
            ])
        })
        .collect();
    report_table(frame, rows, list.cursor - start, area);
}

fn short_timestamp(ts: &str) -> String {
    ts.get(..19).unwrap_or(ts).replace('T', " ")
}

fn history_run_state(
    history: &history::History,
    trash: &history::TrashSnapshot,
    run_index: usize,
) -> (String, Color) {
    if history.runs[run_index].action == HistoryAction::Restore {
        return ("restore".to_owned(), ACCENT);
    }
    match trash.state(history, run_index) {
        TrashState::Restorable { items, .. } => (format!("{items} in trash"), SAFE),
        TrashState::Restored => ("restored".to_owned(), MUTED),
        TrashState::Emptied => ("emptied".to_owned(), MUTED),
        TrashState::Unknown => ("unknown".to_owned(), MUTED),
        TrashState::NotTrashed => ("permanent".to_owned(), MUTED),
    }
}

fn history_item_color(state: ItemState) -> Color {
    match state {
        ItemState::InTrash => SAFE,
        ItemState::RestoredHere => ACCENT,
        ItemState::Restored
        | ItemState::Deleted
        | ItemState::DryRun
        | ItemState::Trashed
        | ItemState::LeftTrash => MUTED,
    }
}

fn history_table(frame: &mut Frame, app: &App, area: Rect) {
    let list = &app.history;
    let height = usize::from(area.height.saturating_sub(1));
    let info_width = usize::from(area.width.saturating_sub(10 + 10 + 14 + 3));
    let (start, stop) = window(list.cursor, list.rows.len(), height);
    let rows: Vec<TableRow<'static>> = list.rows[start.min(stop)..stop]
        .iter()
        .map(|spot| match *spot {
            HistorySpot::Run(run_index) => {
                let run = &list.history.runs[run_index];
                let expanded = list.expanded.contains(&run.id) || !app.filter.is_empty();
                let marker = if run.deleted.is_empty() {
                    " "
                } else if expanded {
                    "▾"
                } else {
                    "▸"
                };
                let (state_text, state_color) =
                    history_run_state(&list.history, &list.trash, run_index);
                let detail = format!(
                    "{}  · {}  · {}",
                    short_timestamp(&run.started_at),
                    counted(run.total.items, "item", "items"),
                    run.command
                );
                TableRow::new(vec![
                    Cell::from(format!("{marker} {}", run.session())),
                    Cell::from(format!("{:>10}", ByteSize(run.total.bytes).to_string()))
                        .style(Style::new().fg(palette::size(run.total.bytes))),
                    Cell::from(state_text).style(Style::new().fg(state_color)),
                    Cell::from(shorten(&detail, info_width)),
                ])
                .style(Style::new().add_modifier(Modifier::BOLD))
            }
            HistorySpot::Item(run_index, entry_index) => {
                let entry = &list.history.runs[run_index].deleted[entry_index];
                let id = list.history.item_id(run_index, entry_index);
                let state = history::item_state(&list.history, &list.trash, run_index, entry_index);
                let color = history_item_color(state);
                let label = format!("{}  {}", entry.rule, display_path(&entry.path));
                TableRow::new(vec![
                    Cell::from(format!("  {id}")).style(Style::new().fg(MUTED)),
                    Cell::from(format!("{:>10}", ByteSize(entry.bytes).to_string()))
                        .style(Style::new().fg(palette::size(entry.bytes))),
                    Cell::from(state.as_str()).style(Style::new().fg(color)),
                    Cell::from(shorten(&label, info_width)),
                ])
            }
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Fill(1),
        ],
    )
    .header(
        TableRow::new(["SESSION", "      SIZE", "STATE", "DETAILS"]).style(Style::new().fg(MUTED)),
    )
    .row_highlight_style(Style::new().bg(CURSOR))
    .column_spacing(1);
    let mut state =
        TableState::default().with_selected((!list.rows.is_empty()).then(|| list.cursor - start));
    frame.render_stateful_widget(table, area, &mut state);
}

fn table(frame: &mut Frame, app: &App, area: Rect) {
    match app.tab {
        Tab::Duplicates => return duplicates_table(frame, app, area),
        Tab::Issues => return issues_table(frame, app, area),
        Tab::Repos => return repos_table(frame, app, area),
        Tab::History => return history_table(frame, app, area),
        Tab::Artifacts | Tab::Caches | Tab::Toolchains | Tab::Docker => {}
    }
    let path_width = usize::from(area.width.saturating_sub(FIXED_COLUMNS + 8));
    let height = usize::from(area.height.saturating_sub(1));
    let cursor_row = app.view_row.get(app.cursor).copied();
    let start = cursor_row.map_or(0, |row| (row + 1).saturating_sub(height));
    let stop = (start + height).min(app.rows.len());
    let rows: Vec<TableRow> = app.rows[start.min(stop)..stop]
        .iter()
        .map(|row| match *row {
            Row::Group(group) => group_row(app, &app.groups[group], path_width),
            Row::Item(position) => {
                item_row(&app.items[app.view[position]], app.largest, path_width)
            }
        })
        .collect();
    let widths = [
        Constraint::Length(3),
        Constraint::Length(10),
        Constraint::Length(BAR),
        Constraint::Length(7),
        Constraint::Length(12),
        Constraint::Length(20),
        Constraint::Length(6),
        Constraint::Fill(1),
    ];
    let table = Table::new(rows, widths)
        .header(
            TableRow::new([
                "",
                "      SIZE",
                "",
                "SAFE?",
                "CATEGORY",
                "RULE",
                "AGE",
                "PATH",
            ])
            .style(Style::new().fg(MUTED)),
        )
        .row_highlight_style(Style::new().bg(CURSOR))
        .column_spacing(1);
    let mut state = TableState::default().with_selected(cursor_row.map(|row| row - start));
    frame.render_stateful_widget(table, area, &mut state);
}

fn muted(text: impl Into<String>) -> Line<'static> {
    Line::styled(format!(" {}", text.into()), Style::new().fg(MUTED))
}

fn empty_text(app: &App) -> String {
    match app.tab {
        Tab::Artifacts if app.finished.is_some() => "Nothing found.".to_owned(),
        Tab::Artifacts => "Scanning, results appear as they are found.".to_owned(),
        Tab::Caches | Tab::Toolchains | Tab::Docker => {
            let Some(source) = app.tab.source() else {
                return String::new();
            };
            let feed = app.feed(source);
            match (&feed.problem, feed.load, app.tab) {
                (Some(problem), _, _) => problem.clone(),
                (None, Load::Done, Tab::Caches) => "No global tool caches found.".to_owned(),
                (None, Load::Done, Tab::Toolchains) => "No toolchain versions found.".to_owned(),
                (None, Load::Done, _) => "No Docker or Podman objects found.".to_owned(),
                (None, _, Tab::Caches) => {
                    "Measuring global tool caches, they appear as they are sized.".to_owned()
                }
                (None, _, Tab::Toolchains) => {
                    "Finding toolchain versions and the projects that pin them, they appear as they are sized."
                        .to_owned()
                }
                (None, _, _) => {
                    "Asking Docker and Podman for images, containers, volumes and build cache."
                        .to_owned()
                }
            }
        }
        Tab::Repos => match (&app.repos.problem, app.repos.load) {
            (Some(problem), _) => problem.clone(),
            (None, Load::Done) if app.repos.found.is_empty() => {
                "No repository has anything to pack or merged branches to remove.".to_owned()
            }
            (None, Load::Done) => "No repository matches the search.".to_owned(),
            (None, Load::Loading) => format!(
                "Reading {} for git gc gain and merged branches.",
                counted(app.repos.checking, "repository", "repositories")
            ),
            (None, Load::Idle) if app.finished.is_none() => {
                "Repositories are read once the scan finishes.".to_owned()
            }
            (None, Load::Idle) => "Reading repositories.".to_owned(),
        },
        Tab::Duplicates => match (&app.duplicates.problem, app.duplicates.load) {
            (Some(problem), _) => problem.clone(),
            (None, Load::Done) => "No duplicate files found.".to_owned(),
            (None, _) => format!(
                "{}, results appear when done.",
                duplicate_stage(app.duplicates.stage)
            ),
        },
        Tab::Issues if app.finished.is_some() => "Every folder was read.".to_owned(),
        Tab::Issues => "Scanning, unreadable folders appear here.".to_owned(),
        Tab::History => match (&app.history.problem, app.history.load) {
            (Some(problem), _) => problem.clone(),
            (None, Load::Done) if app.history.history.runs.is_empty() => {
                "Nothing deleted yet. Past runs appear here.".to_owned()
            }
            (None, Load::Done) => "No history entry matches the search.".to_owned(),
            (None, _) => "Reading deletion history and trash.".to_owned(),
        },
    }
}

fn duplicate_detail(app: &App) -> Option<Vec<Line<'static>>> {
    let list = &app.duplicates;
    let report = "Report only: kuizer lists duplicates and never deletes them.";
    Some(match *list.rows.get(list.cursor)? {
        Spot::Group(group) => {
            let group = &list.found.groups[group];
            let check = if group.verified {
                "compared byte for byte"
            } else {
                "matched by content hash"
            };
            vec![
                Line::raw(format!(
                    " {} identical files of {}, {} reclaimable",
                    group.members.len(),
                    ByteSize(group.size),
                    ByteSize(group.reclaimable())
                )),
                muted(format!("{check} · blake3 {}", group.hash)),
                muted(report),
            ]
        }
        Spot::Member(group, member) => {
            let member = &list.found.groups[group].members[member];
            let links = match member.links.len() {
                0 => String::new(),
                count => format!(" · {count} hard links"),
            };
            vec![
                Line::raw(format!(" {}", display_path(&member.path))),
                muted(format!("modified {}{links}", since(member.modified))),
                muted(report),
            ]
        }
    })
}

fn issue_detail(app: &App) -> Option<Vec<Line<'static>>> {
    let list = &app.issues;
    let issue = &list.entries[*list.rows.get(list.cursor)?];
    Some(vec![
        Line::raw(format!(" {}", display_path(&issue.path))),
        Line::styled(
            format!(" {}: {}", issue_label(issue.kind), issue.message),
            Style::new().fg(WARNING),
        ),
        muted("Nothing below this path was counted."),
    ])
}

fn repo_detail(app: &App) -> Option<Vec<Line<'static>>> {
    let repository = app.current_repo()?;
    let objects = &repository.objects;
    let branches = |name: &Option<String>| name.clone().unwrap_or_else(|| "-".to_owned());
    let first = vec![
        Span::raw(format!(" {}", display_path(&repository.root))),
        Span::styled(
            format!(
                "  default {} · on {}",
                branches(&repository.default_branch),
                branches(&repository.current_branch)
            ),
            Style::new().fg(MUTED),
        ),
    ];
    let second = match repository.busy {
        Some(busy) => Line::styled(
            format!(" Skipped while {}.", busy.describe()),
            Style::new().fg(WARNING),
        ),
        None => muted(format!(
            "git gc frees up to {} · {} · {}",
            ByteSize(objects.gain()),
            counted(objects.loose as usize, "loose object", "loose objects"),
            counted(objects.packs as usize, "pack", "packs")
        )),
    };
    let names: Vec<&str> = repository
        .merged
        .iter()
        .map(|branch| branch.name.as_str())
        .collect();
    let third = if names.is_empty() {
        muted("No merged branches.")
    } else {
        Line::styled(
            format!(" merged into the default branch: {}", names.join(", ")),
            Style::new().fg(ACCENT),
        )
    };
    Some(vec![Line::from(first), second, third])
}

fn history_detail(app: &App) -> Option<Vec<Line<'static>>> {
    let list = &app.history;
    let spot = *list.rows.get(list.cursor)?;
    match spot {
        HistorySpot::Run(run_index) => {
            let run = list.history.runs.get(run_index)?;
            let (state_text, state_color) =
                history_run_state(&list.history, &list.trash, run_index);
            let first = Line::from(vec![
                Span::raw(format!(
                    " {}  · {}  · {}  · {}",
                    run.session(),
                    short_timestamp(&run.started_at),
                    counted(run.total.items, "item", "items"),
                    ByteSize(run.total.bytes)
                )),
                Span::styled(format!("  {state_text}"), Style::new().fg(state_color)),
            ]);
            let second = muted(run.command.clone());
            let third = if run.action == HistoryAction::Restore {
                muted("Restore session. Press Enter to expand or collapse items.")
            } else if !list.trash.known {
                muted(
                    "Trash cannot be read on this platform; on macOS use Put Back in Finder. Enter expands.",
                )
            } else {
                match list.trash.state(&list.history, run_index) {
                    TrashState::Restorable { items, bytes } => Line::styled(
                        format!(
                            " Press r to restore {} ({}) from trash · Enter expands or collapses",
                            counted(items, "item", "items"),
                            ByteSize(bytes)
                        ),
                        Style::new().fg(SAFE),
                    ),
                    TrashState::Restored => {
                        muted("Everything trashed in this session was already restored.")
                    }
                    TrashState::Emptied => {
                        muted("Items from this session are no longer in the trash.")
                    }
                    TrashState::NotTrashed => {
                        muted("Deleted permanently; nothing in this session can be restored.")
                    }
                    TrashState::Unknown => {
                        muted("Trash status is unknown on this platform. Enter expands.")
                    }
                }
            };
            Some(vec![first, second, third])
        }
        HistorySpot::Item(run_index, entry_index) => {
            let entry = list.history.runs.get(run_index)?.deleted.get(entry_index)?;
            let id = list.history.item_id(run_index, entry_index);
            let state = history::item_state(&list.history, &list.trash, run_index, entry_index);
            let first = Line::raw(format!(" {id}  {}", display_path(&entry.path)));
            let second = Line::from(vec![
                Span::styled(
                    format!(" {}", state.as_str()),
                    Style::new().fg(history_item_color(state)),
                ),
                Span::styled(
                    format!(
                        "  · {}  · {}  · {}  · {}",
                        entry.rule,
                        entry.category,
                        ByteSize(entry.bytes),
                        short_timestamp(&entry.deleted_at)
                    ),
                    Style::new().fg(MUTED),
                ),
            ]);
            let third = match state {
                ItemState::InTrash => Line::styled(
                    " Still in trash. Press r to restore to its original path.",
                    Style::new().fg(SAFE),
                ),
                ItemState::RestoredHere => match entry.restores.as_deref() {
                    Some(origin) => muted(format!("Restored from {origin}.")),
                    None => muted("Restored in this session."),
                },
                ItemState::Restored => muted("Already restored by a later session."),
                ItemState::Deleted => muted("Deleted permanently; cannot be restored."),
                ItemState::DryRun => muted("Dry run; nothing was removed."),
                ItemState::LeftTrash => muted("No longer in the trash; cannot be restored."),
                ItemState::Trashed => {
                    muted("Trash cannot be read on this platform; on macOS use Put Back in Finder.")
                }
            };
            Some(vec![first, second, third])
        }
    }
}

fn detail(app: &App) -> Vec<Line<'static>> {
    let shown = match app.tab {
        Tab::Duplicates => duplicate_detail(app),
        Tab::Issues => issue_detail(app),
        Tab::Repos => repo_detail(app),
        Tab::History => history_detail(app),
        Tab::Artifacts | Tab::Caches | Tab::Toolchains | Tab::Docker => {
            app.current().map(|item| item_detail(app, item))
        }
    };
    shown.unwrap_or_else(|| vec![muted(empty_text(app))])
}

fn item_detail(app: &App, item: &Item) -> Vec<Line<'static>> {
    let candidate = &item.candidate;
    let mut first = vec![Span::raw(format!(" {}", display_path(&candidate.path)))];
    if !item.label.is_empty() {
        first.push(Span::styled(
            format!("  {}", item.label),
            Style::new().fg(MUTED),
        ));
    }
    if candidate.usage.partial {
        first.push(Span::styled("  size is partial", Style::new().fg(MUTED)));
    }
    let entries = counted(
        usize::try_from(candidate.usage.entries).unwrap_or(usize::MAX),
        "entry",
        "entries",
    );
    let second = match &item.status {
        Status::Failed(error) => format!(" failed: {error}"),
        _ if item.source == Source::Docker => format!(
            " {} {}{}",
            candidate.rule.ecosystem,
            candidate.rule.name,
            if item.note.is_empty() {
                String::new()
            } else {
                format!(" · {}", item.note)
            },
        ),
        _ if item.source != Source::Artifact => format!(
            " {} · {} · {}{}",
            candidate.rule.name,
            candidate.rule.ecosystem,
            entries,
            if item.note.is_empty() {
                String::new()
            } else {
                format!(" · {}", item.note)
            },
        ),
        _ => format!(
            " {} · {} · {} · project {}",
            candidate.rule.name,
            candidate.rule.ecosystem,
            entries,
            candidate
                .project
                .as_deref()
                .map(display_path)
                .unwrap_or_else(|| "-".to_owned()),
        ),
    };
    let mut second = vec![Span::styled(second, Style::new().fg(MUTED))];
    if let Some(changes) = app.changes_in(candidate) {
        second.push(Span::styled(
            format!(" · {}", uncommitted(changes)),
            Style::new().fg(WARNING),
        ));
    }
    let advice = match candidate.rule.safety {
        Safety::Safe => SAFE,
        Safety::Review => WARNING,
    };
    vec![
        Line::from(first),
        Line::styled(format!(" {}", explain(item)), Style::new().fg(advice)),
        Line::from(second),
    ]
}

fn uncommitted(changes: usize) -> String {
    format!(
        "{changes} uncommitted change{}",
        if changes == 1 { "" } else { "s" }
    )
}

fn format_key(key: &str) -> Cow<'_, str> {
    match key {
        "escape" => Cow::Borrowed("Esc"),
        "enter" => Cow::Borrowed("Enter"),
        "space" => Cow::Borrowed("Space"),
        "tab" => Cow::Borrowed("Tab"),
        "shift-tab" => Cow::Borrowed("S-Tab"),
        "backspace" => Cow::Borrowed("Bksp"),
        "delete" => Cow::Borrowed("Del"),
        "home" => Cow::Borrowed("Home"),
        "end" => Cow::Borrowed("End"),
        "pageup" => Cow::Borrowed("PgUp"),
        "pagedown" => Cow::Borrowed("PgDn"),
        "up" => Cow::Borrowed("↑"),
        "down" => Cow::Borrowed("↓"),
        "left" => Cow::Borrowed("←"),
        "right" => Cow::Borrowed("→"),
        "shift-up" => Cow::Borrowed("S-↑"),
        "shift-down" => Cow::Borrowed("S-↓"),
        "shift-left" => Cow::Borrowed("S-←"),
        "shift-right" => Cow::Borrowed("S-→"),
        _ => {
            if let Some(rest) = key.strip_prefix("ctrl-") {
                Cow::Owned(format!("Ctrl+{}", rest.to_ascii_uppercase()))
            } else if let Some(rest) = key.strip_prefix("shift-") {
                Cow::Owned(format!("S-{}", rest.to_ascii_uppercase()))
            } else {
                Cow::Borrowed(key)
            }
        }
    }
}

fn key_chip(key: &str) -> Span<'static> {
    Span::styled(
        format!(" {} ", format_key(key)),
        Style::new()
            .fg(KEY_FG)
            .bg(KEY_BG)
            .add_modifier(Modifier::BOLD),
    )
}

fn key_hints(app: &App, scope: Scope, hints: &[(Action, &str)]) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(hints.len() * 2);
    for (key, label) in hints {
        if let Some(key) = app.keymap.keys(scope, *key).next() {
            spans.push(key_chip(key));
        }
        spans.push(Span::styled(format!(" {label}  "), Style::new().fg(MUTED)));
    }
    spans
}

fn mode_chip(app: &App) -> Span<'static> {
    let (label, fill) = match app.overlay {
        Overlay::Confirm { .. }
        | Overlay::Branches { .. }
        | Overlay::Exclude { .. }
        | Overlay::Restore { .. }
        | Overlay::Quit => ("Confirm", WARNING),
        Overlay::Help => ("Help", MUTED),
        Overlay::None if app.filtering => ("Filter", MUTED),
        Overlay::None => ("Normal", MUTED),
    };
    Span::styled(
        format!(" {label} "),
        Style::new()
            .fg(ON_FILL)
            .bg(fill)
            .add_modifier(Modifier::BOLD),
    )
}

fn exclude_prompt(app: &App, path: &Path, pattern: &str, confirm: bool) -> Line<'static> {
    let cancel_style = if !confirm {
        Style::new()
            .fg(ON_FILL)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(MUTED)
    };
    let exclude_style = if confirm {
        Style::new()
            .fg(ON_FILL)
            .bg(WARNING)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(MUTED)
    };
    let shown = display_path(path);
    let mut spans = vec![
        mode_chip(app),
        Span::styled(
            format!(" Exclude {shown} (adds {pattern} to scan.exclude)? "),
            Style::new().fg(WARNING).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Cancel ", cancel_style),
        Span::raw(" "),
        Span::styled(" Exclude ", exclude_style),
        Span::raw("  "),
    ];
    spans.extend(key_hints(
        app,
        Scope::TuiConfirm,
        &[
            (Action::DialogToggle, "switch"),
            (Action::Confirm, "accept"),
            (Action::Cancel, "cancel"),
        ],
    ));
    Line::from(spans)
}

fn restore_prompt(
    app: &App,
    label: &str,
    count: usize,
    bytes: u64,
    confirm: bool,
) -> Line<'static> {
    let cancel_style = if !confirm {
        Style::new()
            .fg(ON_FILL)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(MUTED)
    };
    let restore_style = if confirm {
        Style::new()
            .fg(ON_FILL)
            .bg(SAFE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(MUTED)
    };
    let mut spans = vec![
        mode_chip(app),
        Span::styled(
            format!(
                " Restore {} ({}) from {label}? ",
                counted(count, "item", "items"),
                ByteSize(bytes)
            ),
            Style::new().fg(WARNING).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Cancel ", cancel_style),
        Span::raw(" "),
        Span::styled(" Restore ", restore_style),
        Span::raw("  "),
    ];
    spans.extend(key_hints(
        app,
        Scope::TuiConfirm,
        &[
            (Action::DialogToggle, "switch"),
            (Action::Confirm, "accept"),
            (Action::Cancel, "cancel"),
        ],
    ));
    Line::from(spans)
}

fn quit_prompt(app: &App) -> Line<'static> {
    let deleting = app.deleting();
    let running = app.repos.running.len();
    let mut parts = Vec::new();
    if deleting > 0 {
        parts.push(counted(
            deleting,
            "unfinished deletion",
            "unfinished deletions",
        ));
    }
    if running > 0 {
        parts.push(format!(
            "git running in {}",
            counted(running, "repository", "repositories")
        ));
    }
    if app.history.restoring {
        parts.push("a restore running".to_owned());
    }
    let unfinished = if parts.is_empty() {
        "unfinished work".to_owned()
    } else {
        parts.join(" and ")
    };
    let mut spans = vec![
        mode_chip(app),
        Span::styled(
            format!(" Quit with {unfinished}? Queued ones are cancelled.  "),
            Style::new().fg(WARNING).add_modifier(Modifier::BOLD),
        ),
    ];
    spans.extend(key_hints(
        app,
        Scope::TuiQuit,
        &[
            (Action::QuitAccept, "quit"),
            (Action::QuitCancel, "keep deleting"),
        ],
    ));
    Line::from(spans)
}

fn footer(app: &App) -> Line<'static> {
    if app.overlay == Overlay::Quit {
        return quit_prompt(app);
    }
    if let Overlay::Exclude {
        path,
        pattern,
        confirm,
    } = &app.overlay
    {
        return exclude_prompt(app, path, pattern, *confirm);
    }
    if let Overlay::Restore {
        label,
        targets,
        bytes,
        confirm,
    } = &app.overlay
    {
        return restore_prompt(app, label, targets.len(), *bytes, *confirm);
    }
    if app.filtering {
        let mut spans = vec![mode_chip(app), Span::raw(format!(" {}█   ", app.filter))];
        spans.extend(key_hints(
            app,
            Scope::TuiFilter,
            &[(Action::ApplyFilter, "apply"), (Action::Back, "clear")],
        ));
        return Line::from(spans);
    }
    if app.tab == Tab::Repos {
        return repos_footer(app);
    }
    if app.tab == Tab::History {
        return history_footer(app);
    }
    if app.tab.source().is_none() {
        return report_footer(app);
    }
    let selected = app.selected_count();
    let mut spans = vec![mode_chip(app), Span::raw(" ")];
    let review = app.selected_review();
    if review > 0 {
        spans.push(Span::styled(
            format!(" {review} review selected  "),
            Style::new().fg(WARNING).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!(" {message}  ")));
    }
    if app.rows.is_empty() {
        spans.extend(key_hints(
            app,
            Scope::Tui,
            &[
                (Action::FilterFocus, "search"),
                (Action::Help, "help"),
                (Action::Quit, "quit"),
            ],
        ));
    } else {
        let delete_hint = if selected > 0 {
            (Action::Delete, "delete")
        } else {
            (Action::DeleteCursor, "delete")
        };
        spans.extend(key_hints(
            app,
            Scope::Tui,
            &[
                (Action::Toggle, "select"),
                (Action::SelectAll, "all safe"),
                delete_hint,
                (Action::SafeOnly, "safe"),
                (Action::Category, "category"),
                (Action::FilterFocus, "search"),
                (Action::Help, "help"),
                (Action::Quit, "quit"),
            ],
        ));
    }
    if app.filtered() || selected > 0 {
        spans.extend(key_hints(app, Scope::Tui, &[(Action::Back, "back")]));
    }
    if app.filtered() {
        spans.extend(key_hints(
            app,
            Scope::Tui,
            &[(Action::ResetFilters, "clear filters")],
        ));
    }
    if selected > 0 {
        spans.push(Span::styled(
            format!(" {selected} selected  {} ", ByteSize(app.selected_bytes())),
            Style::new().add_modifier(Modifier::BOLD),
        ));
    }
    if app.freed > 0 {
        spans.push(Span::raw(format!(" freed {} ", ByteSize(app.freed))));
    }
    if app.trashed > 0 {
        spans.push(Span::raw(format!(" trashed {} ", ByteSize(app.trashed))));
    }
    let deleting = app.deleting();
    if deleting > 0 {
        spans.push(Span::styled(
            format!(" deleting {deleting} "),
            Style::new().fg(MUTED),
        ));
    }
    Line::from(spans)
}

fn report_footer(app: &App) -> Line<'static> {
    let mut spans = vec![mode_chip(app), Span::raw(" ")];
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!(" {message}  ")));
    }
    spans.extend(key_hints(
        app,
        Scope::Tui,
        &[
            (Action::Down, "move"),
            (Action::FilterFocus, "search"),
            (Action::Help, "help"),
            (Action::Quit, "quit"),
        ],
    ));
    if !app.filter.is_empty() {
        spans.extend(key_hints(
            app,
            Scope::Tui,
            &[(Action::Back, "clear search")],
        ));
    }
    Line::from(spans)
}

fn repos_footer(app: &App) -> Line<'static> {
    let mut spans = vec![mode_chip(app), Span::raw(" ")];
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!(" {message}  ")));
    }
    spans.extend(key_hints(
        app,
        Scope::TuiRepos,
        &[
            (Action::Gc, "git gc"),
            (Action::Branches, "merged branches"),
            (Action::GcAll, "gc all"),
            (Action::BranchesAll, "branches all"),
        ],
    ));
    spans.extend(key_hints(
        app,
        Scope::Tui,
        &[
            (Action::FilterFocus, "search"),
            (Action::Help, "help"),
            (Action::Quit, "quit"),
        ],
    ));
    if !app.filter.is_empty() {
        spans.extend(key_hints(
            app,
            Scope::Tui,
            &[(Action::Back, "clear search")],
        ));
    }
    let running = app.repos.running.len();
    if running > 0 {
        spans.push(Span::styled(
            format!(
                " git running in {} ",
                counted(running, "repository", "repositories")
            ),
            Style::new().fg(MUTED),
        ));
    }
    Line::from(spans)
}

fn history_footer(app: &App) -> Line<'static> {
    let mut spans = vec![mode_chip(app), Span::raw(" ")];
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!(" {message}  ")));
    }
    spans.push(key_chip("enter"));
    spans.push(Span::styled(" expand  ", Style::new().fg(MUTED)));
    spans.extend(key_hints(
        app,
        Scope::TuiHistory,
        &[(Action::Restore, "restore")],
    ));
    spans.extend(key_hints(
        app,
        Scope::Tui,
        &[
            (Action::FilterFocus, "search"),
            (Action::Help, "help"),
            (Action::Quit, "quit"),
        ],
    ));
    if !app.filter.is_empty() {
        spans.extend(key_hints(
            app,
            Scope::Tui,
            &[(Action::Back, "clear search")],
        ));
    }
    if app.history.restoring {
        spans.push(Span::styled(" restoring… ", Style::new().fg(MUTED)));
    }
    Line::from(spans)
}

pub(super) fn cells(count: usize) -> u16 {
    u16::try_from(count).unwrap_or(u16::MAX)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

const DIALOG_WIDTH: u16 = 90;

fn dialog(frame: &mut Frame, lines: Vec<Line<'static>>, title: Line<'static>, border: Color) {
    let area = centered(
        frame.area(),
        DIALOG_WIDTH,
        cells(lines.len()).saturating_add(2),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(border))
                .title(title),
        ),
        area,
    );
}

fn dialog_width(frame: &Frame) -> usize {
    usize::from(DIALOG_WIDTH.min(frame.area().width).saturating_sub(16))
}

fn confirm(frame: &mut Frame, app: &App, stats: &ConfirmStats, mode: Mode) {
    const DIRTY_SHOWN: usize = 3;
    let typing = app.pending_confirmation() == Confirmation::TypeCount;
    let inner_width = dialog_width(frame);
    let mut lines = vec![Line::raw("")];
    for &index in &stats.shown {
        let item = &app.items[index];
        lines.push(Line::raw(format!(
            "  {:>10}  {}",
            ByteSize(item.candidate.usage.allocated).to_string(),
            shorten(&shown_path(item), inner_width)
        )));
    }
    if stats.count > stats.shown.len() {
        lines.push(Line::styled(
            format!("  and {} more", stats.count - stats.shown.len()),
            Style::new().fg(MUTED),
        ));
    }
    lines.push(Line::raw(""));
    let action = match mode {
        Mode::Trash => "Move to trash. Space is freed once the trash is emptied.",
        Mode::Permanent => "Delete permanently. This cannot be undone.",
        Mode::DryRun => "Dry run. Nothing is touched.",
    };
    lines.push(Line::styled(
        format!("  {action}"),
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let permanent_source = stats
        .shown
        .first()
        .map(|&index| app.items[index].source)
        .filter(|source| source.permanent());
    if app.mode == Mode::Trash
        && let Some(source) = permanent_source
    {
        let what = match source {
            Source::Docker => "Docker objects",
            Source::Trash => "Trash items",
            _ => "Toolchains",
        };
        lines.push(muted(format!(
            " {what} never go to the trash, they are removed for good."
        )));
    }
    if let (Some(free), Some(after)) = (app.free, app.free_after(stats.bytes, mode)) {
        lines.push(Line::raw(format!(
            "  {} free now, {} free afterwards.",
            ByteSize(free),
            ByteSize(after)
        )));
    }
    if stats.review > 0 {
        lines.push(Line::raw(format!(
            "  {} of these are marked review: check they are generated before deleting.",
            stats.review
        )));
    }
    for (project, changes) in stats.dirty.iter().take(DIRTY_SHOWN) {
        let note = format!(" has {}.", uncommitted(*changes));
        let width = inner_width.saturating_sub(note.chars().count());
        lines.push(Line::styled(
            format!("  {}{note}", shorten(&display_path(project), width)),
            Style::new().fg(WARNING),
        ));
    }
    if stats.dirty.len() > DIRTY_SHOWN {
        lines.push(Line::styled(
            format!(
                "  and {} more projects with uncommitted changes.",
                stats.dirty.len() - DIRTY_SHOWN
            ),
            Style::new().fg(WARNING),
        ));
    }
    if stats.checking > 0 {
        lines.push(Line::styled(
            format!(
                "  Still checking {} for uncommitted changes.",
                counted(stats.checking, "project", "projects")
            ),
            Style::new().fg(WARNING),
        ));
    }
    if stats.unknown {
        lines.push(Line::styled(
            "  Could not check these projects for uncommitted changes.",
            Style::new().fg(WARNING),
        ));
    }
    lines.push(Line::raw(""));
    if typing {
        let count = app.confirm_count();
        lines.push(Line::styled(
            format!("  Review items are selected. Type {count} to delete them, Esc cancels."),
            Style::new().fg(WARNING).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!(" {}█ ", app.typed),
                Style::new().fg(ON_FILL).bg(SEARCH_FILL),
            ),
            if app.typed_wrong {
                Span::styled(format!("  That is not {count}."), Style::new().fg(DANGER))
            } else {
                Span::raw("")
            },
        ]));
        lines.push(Line::raw(""));
    }
    if typing {
        let confirm_key = app
            .keymap
            .keys(Scope::TuiConfirm, Action::Confirm)
            .find(|&key| key == "enter")
            .or_else(|| app.keymap.keys(Scope::TuiConfirm, Action::Confirm).next());
        let cancel_key = app
            .keymap
            .keys(Scope::TuiConfirm, Action::Cancel)
            .find(|&key| key == "escape")
            .or_else(|| app.keymap.keys(Scope::TuiConfirm, Action::Cancel).next());
        let mut hints = Vec::with_capacity(4);
        if let Some(key) = confirm_key {
            hints.push(key_chip(key));
        }
        hints.push(Span::styled(" delete  ", Style::new().fg(MUTED)));
        if let Some(key) = cancel_key {
            hints.push(key_chip(key));
        }
        hints.push(Span::styled(" cancel  ", Style::new().fg(MUTED)));
        lines.push(Line::from(hints));
    } else {
        lines.push(Line::from(key_hints(
            app,
            Scope::TuiConfirm,
            &[(Action::Confirm, "confirm"), (Action::Cancel, "cancel")],
        )));
    }
    let title = Line::styled(
        format!(
            " Delete {} · {} ",
            counted(stats.count, "item", "items"),
            ByteSize(stats.bytes)
        ),
        Style::new().add_modifier(Modifier::BOLD),
    );
    dialog(frame, lines, title, if typing { WARNING } else { FAINT });
}

fn button(label: String, focused: bool, fill: Color) -> Span<'static> {
    let style = if focused {
        Style::new()
            .fg(ON_FILL)
            .bg(fill)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(MUTED)
    };
    Span::styled(format!(" {label} "), style)
}

fn branches(frame: &mut Frame, app: &App, scope: &GitScope, delete: bool) {
    let targets = app.git_targets(GitTask::Branches, scope);
    let total: usize = targets
        .iter()
        .map(|repository| repository.merged.len())
        .sum();
    let inner_width = dialog_width(frame);
    let mut lines = vec![Line::raw("")];
    for repository in targets.iter().take(SHOWN_REPOS) {
        let names: Vec<&str> = repository
            .merged
            .iter()
            .map(|branch| branch.name.as_str())
            .collect();
        lines.push(Line::raw(format!(
            "  {}",
            shorten(&display_path(&repository.root), inner_width)
        )));
        lines.push(muted(format!(
            "   {}",
            shorten(&names.join(", "), inner_width)
        )));
    }
    if targets.len() > SHOWN_REPOS {
        lines.push(muted(format!(
            " and {} more",
            counted(targets.len() - SHOWN_REPOS, "repository", "repositories")
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  Deletes local branches already merged into the default branch. Remote branches stay.",
        Style::new().add_modifier(Modifier::BOLD),
    ));
    lines.push(muted(
        " Each branch is checked again first. Restore one with git branch NAME COMMIT.",
    ));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        button("Cancel".to_owned(), !delete, ACCENT),
        Span::raw("  "),
        button(
            format!("Delete {}", counted(total, "branch", "branches")),
            delete,
            DANGER,
        ),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(key_hints(
        app,
        Scope::TuiConfirm,
        &[
            (Action::DialogToggle, "switch"),
            (Action::Confirm, "choose"),
            (Action::Cancel, "cancel"),
        ],
    )));
    let title = Line::styled(
        format!(
            " Delete merged branches in {} ",
            counted(targets.len(), "repository", "repositories")
        ),
        Style::new().add_modifier(Modifier::BOLD),
    );
    dialog(frame, lines, title, if delete { DANGER } else { FAINT });
}

const HELP_LEFT: [HelpSection; 3] = {
    use Action::*;
    [
        (
            "Views",
            Scope::Tui,
            &[
                (&[Artifacts], "project artifacts"),
                (&[Caches], "global tool caches"),
                (&[Duplicates], "duplicate files, report only"),
                (&[Issues], "folders that could not be read"),
                (&[Toolchains], "toolchains, deleted for good"),
                (&[Docker], "Docker/Podman, deleted for good"),
                (&[Repos], "git repos: gc & merged branches"),
                (&[History], "history and restore"),
            ],
        ),
        (
            "Navigate",
            Scope::Tui,
            &[
                (&[Down, Up], "move"),
                (&[ExtendDown, ExtendUp], "select while moving"),
                (&[First], "first"),
                (&[Last], "last"),
                (&[PageUp, PageDown], "page"),
                (&[HalfPageDown, HalfPageUp], "half page"),
            ],
        ),
        (
            "Select",
            Scope::Tui,
            &[
                (&[Toggle], "toggle and move down"),
                (&[SelectAll], "select all safe items in view"),
                (&[SelectProject], "project: safe, +review, clear"),
                (&[SelectNone], "clear selection"),
            ],
        ),
    ]
};

const HELP_RIGHT: [HelpSection; 5] = {
    use Action::*;
    [
        (
            "Filter",
            Scope::Tui,
            &[
                (&[SafeOnly], "show only safe items"),
                (&[Category], "cycle category filter"),
                (&[FilterFocus], "search by path or rule"),
                (&[ResetFilters], "clear all filters"),
            ],
        ),
        (
            "Act",
            Scope::Tui,
            &[
                (&[DeleteCursor], "delete selection or cursor item"),
                (&[Delete], "delete selection"),
                (&[Exclude], "always exclude row in config"),
                (&[CycleSort], "cycle sort: size, path, category"),
                (&[Group], "group by rule or project"),
                (&[Back], "back: search, filter, selection"),
                (&[Quit], "quit (asks while deleting)"),
            ],
        ),
        (
            "Git",
            Scope::TuiRepos,
            &[
                (&[Gc], "git gc in this repository"),
                (&[GcAll], "git gc in every shown repo"),
                (&[Branches], "delete merged branches here"),
                (&[BranchesAll], "delete all merged branches"),
            ],
        ),
        (
            "History",
            Scope::TuiHistory,
            &[(&[Restore], "restore trashed session or item")],
        ),
        (
            "Confirm",
            Scope::TuiConfirm,
            &[
                (&[Confirm], "confirm, or activate button"),
                (&[DialogToggle], "switch Cancel / Delete"),
                (&[FieldBackspace], "erase typed count digit"),
                (&[Cancel], "cancel"),
            ],
        ),
    ]
};

const HELP_MAX_CHIPS: usize = 4;

fn help_column(app: &App, sections: &[HelpSection]) -> (Vec<Line<'static>>, usize) {
    let keys_for = |scope: Scope, actions: &'static [Action]| {
        actions
            .iter()
            .flat_map(move |action| app.keymap.keys(scope, *action))
            .take(HELP_MAX_CHIPS)
    };
    let chips_width = |scope, actions| {
        keys_for(scope, actions)
            .map(|key| format_key(key).chars().count() + 3)
            .sum::<usize>()
    };
    let rows = sections
        .iter()
        .flat_map(|&(_, scope, rows)| rows.iter().map(move |row| (scope, row)));
    let key_width = rows
        .clone()
        .map(|(scope, (keys, _))| chips_width(scope, keys))
        .max()
        .unwrap_or(0);
    let action_width = rows
        .map(|(_, (_, action))| action.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for &(title, scope, rows) in sections {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("  {}", title.to_uppercase()),
            Style::new().fg(MUTED).add_modifier(Modifier::BOLD),
        ));
        for (keys, action) in rows {
            let mut spans = vec![Span::raw("  ")];
            for key in keys_for(scope, keys) {
                spans.push(key_chip(key));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::raw(
                " ".repeat(key_width - chips_width(scope, keys) + 1),
            ));
            spans.push(Span::raw(*action));
            lines.push(Line::from(spans));
        }
    }
    (lines, 2 + key_width + 1 + action_width + 2)
}

fn help(frame: &mut Frame, app: &App) {
    let (mut left, left_width) = help_column(app, &HELP_LEFT);
    let (right, right_width) = help_column(app, &HELP_RIGHT);
    left.push(Line::raw(""));
    left.push(Line::styled("  any key to close", Style::new().fg(FAINT)));
    let height = cells(left.len().max(right.len())).saturating_add(2);
    let area = centered(
        frame.area(),
        cells(left_width + right_width).saturating_add(2),
        height,
    );
    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(FAINT))
        .title(Span::styled(
            " Keys ",
            Style::new().add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let first_width = if left_width + right_width > usize::from(inner.width) {
        left_width.min(usize::from(inner.width) / 2)
    } else {
        left_width
    };
    let [first, second] =
        Layout::horizontal([Constraint::Length(cells(first_width)), Constraint::Fill(1)])
            .areas(inner);
    frame.render_widget(Paragraph::new(left), first);
    frame.render_widget(Paragraph::new(right), second);
}

pub fn draw(frame: &mut Frame, app: &App) {
    let [top, bar, main, info, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(Paragraph::new(header(app)), top);
    frame.render_widget(Paragraph::new(tabs(app)), bar);
    table(frame, app, main);
    frame.render_widget(Paragraph::new(detail(app)), info);
    frame.render_widget(Paragraph::new(footer(app)), bottom);
    match &app.overlay {
        Overlay::Confirm { mode, .. } => {
            if let Some(stats) = &app.confirm_stats {
                confirm(frame, app, stats, *mode);
            }
        }
        Overlay::Branches { scope, delete } => branches(frame, app, scope, *delete),
        Overlay::Help => help(frame, app),
        Overlay::None | Overlay::Quit | Overlay::Exclude { .. } | Overlay::Restore { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render_lines(app: &App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn shortens_from_the_left() {
        assert_eq!(shorten("~/projects/web/node_modules", 14), "…/node_modules");
        assert_eq!(shorten("short", 10), "short");
    }

    #[test]
    fn size_bar_rounds_up_and_stays_in_bounds() {
        assert_eq!(size_bar(0, 0), "░".repeat(8));
        assert_eq!(size_bar(1, 1000), format!("█{}", "░".repeat(7)));
        assert_eq!(
            size_bar(500, 1000),
            format!("{}{}", "█".repeat(4), "░".repeat(4))
        );
        assert_eq!(size_bar(u64::MAX, 1), "█".repeat(8));
    }

    #[test]
    fn cells_saturates_at_the_terminal_limit() {
        assert_eq!(cells(12), 12);
        assert_eq!(cells(usize::MAX), u16::MAX);
    }

    #[test]
    fn help_overlay_fits_both_columns_at_120x36() {
        let mut app = App::new(Mode::Trash, true);
        app.overlay = Overlay::Help;
        let rendered = render_lines(&app, 120, 36).join("\n");
        for expected in [
            "VIEWS",
            "NAVIGATE",
            "SELECT",
            "FILTER",
            "ACT",
            "GIT",
            "HISTORY",
            "CONFIRM",
            "history and restore",
            "restore trashed session or item",
            "cycle sort: size, path, category",
            "switch Cancel / Delete",
            "erase typed count digit",
            "any key to close",
        ] {
            assert!(
                rendered.contains(expected),
                "expected {expected:?} in 120x36 help overlay:\n{rendered}"
            );
        }
    }

    #[test]
    fn history_tab_and_restore_prompt_render() {
        use crate::history::{Entry, History, Run, Total, TrashSnapshot};
        use std::collections::HashSet;

        let mut app = App::new(Mode::Trash, true);
        app.switch(Tab::History);
        let history = History {
            next_session: 2,
            runs: vec![Run {
                id: 1,
                action: HistoryAction::Delete,
                started_at: "2026-04-01T10:00:00+00:00".to_owned(),
                finished_at: "2026-04-01T10:00:02+00:00".to_owned(),
                command: "kuizer clean /p/web".to_owned(),
                policy: None,
                roots: vec![std::path::PathBuf::from("/p/web")],
                total: Total {
                    items: 1,
                    bytes: 4096,
                    size: "4.0 KB".to_owned(),
                },
                deleted: vec![Entry {
                    deleted_at: "2026-04-01T10:00:00+00:00".to_owned(),
                    path: std::path::PathBuf::from("/p/web/node_modules"),
                    rule: "node-modules".to_owned(),
                    category: "dependencies".to_owned(),
                    mode: Mode::Trash,
                    bytes: 4096,
                    size: "4.0 KB".to_owned(),
                    restores: None,
                }],
            }],
            ..History::default()
        };
        let trash = TrashSnapshot::new(&history, Some(HashSet::from([(0usize, 0usize)])));
        app.set_history(history, trash);

        let rendered = render_lines(&app, 120, 24).join("\n");
        assert!(rendered.contains("s1"));
        assert!(rendered.contains("1 in trash"));
        assert!(rendered.contains("restore"));

        app.ask_restore();
        let prompt = render_lines(&app, 120, 24).pop().unwrap();
        assert!(prompt.contains("Restore 1 item"));
        assert!(prompt.contains("Cancel"));
    }

    #[test]
    fn footer_fits_at_120_columns() {
        let mut app = App::new(Mode::Trash, true);
        let candidate = crate::scan::Candidate {
            id: 1,
            path: std::path::PathBuf::from("/p/a/node_modules"),
            kind: crate::detect::Kind::Dir,
            project: Some(std::path::PathBuf::from("/p/a")),
            rule: std::sync::Arc::new(crate::detect::RuleInfo {
                name: "node_modules".to_owned(),
                category: crate::detect::Category::Dependencies,
                safety: Safety::Safe,
                ecosystem: "node".to_owned(),
            }),
            usage: crate::scan::Usage {
                apparent: 1024,
                allocated: 1024,
                entries: 1,
                partial: false,
            },
            newest: None,
            identity: None,
            project_last_active: None,
        };
        app.push(candidate);
        app.refresh();
        let lines = render_lines(&app, 120, 24);
        let bottom = lines.last().unwrap();
        assert!(
            bottom.contains("quit"),
            "footer clipped at 120 cols: {bottom:?}"
        );
    }
}
