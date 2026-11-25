use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, Paragraph, Row as TableRow, Table, TableState,
};

use super::app::{
    App, ConfirmStats, Group, Grouping, Item, Load, Overlay, Row, Spot, Status, Tab, project_of,
};
use super::palette::{
    self, ACCENT, CURSOR, FAINT, KEY_BG, KEY_FG, MUTED, ON_FILL, SAFE, SEARCH_FILL, SELECTED,
    WARNING,
};
use crate::cli::SortKey;
use crate::delete::Mode;
use crate::detect::{Category, Safety};
use crate::dupes::Stage;
use crate::keymap::{Action, Scope};
use crate::output::display_path;
use crate::scan::IssueKind;
use crate::source::Source;
use crate::units::{ByteSize, age, since};

const BAR: u16 = 8;
pub(super) const CURRENT_WIDTH: usize = 48;
type HelpRow = (&'static [Action], &'static str);
type HelpSection = (&'static str, &'static [HelpRow]);

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

fn explain(candidate: &crate::scan::Candidate) -> &'static str {
    match (candidate.rule.category, candidate.rule.safety) {
        (Category::Dependencies, Safety::Safe) => {
            "Installed packages. Deleting is safe, reinstall with your package manager."
        }
        (Category::Build, Safety::Safe) => {
            "Build output. Deleting is safe, the next build recreates it."
        }
        (Category::Cache, Safety::Safe) => "Cache. Deleting is safe, it is rebuilt when needed.",
        (Category::Temp, Safety::Safe) => "Temporary files. Deleting is safe.",
        (_, Safety::Review) => {
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
        Tab::Artifacts | Tab::Caches => {
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
        (Status::Failed(_), _) => "  !",
        (Status::Ready, true) => "[x]",
        (Status::Ready, false) => "[ ]",
    };
    let style = match (&item.status, candidate.rule.safety) {
        (Status::Deleted, _) => Style::new().fg(FAINT).add_modifier(Modifier::CROSSED_OUT),
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
        Cell::from(shorten(&display_path(&candidate.path), path_width)),
    ])
    .style(style)
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

fn table(frame: &mut Frame, app: &App, area: Rect) {
    match app.tab {
        Tab::Duplicates => return duplicates_table(frame, app, area),
        Tab::Issues => return issues_table(frame, app, area),
        Tab::Artifacts | Tab::Caches => {}
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
        Tab::Caches => match (&app.cache_problem, app.caches) {
            (Some(problem), _) => problem.clone(),
            (None, Load::Done) => "No global tool caches found.".to_owned(),
            (None, _) => "Measuring global tool caches, they appear as they are sized.".to_owned(),
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

fn detail(app: &App) -> Vec<Line<'static>> {
    let shown = match app.tab {
        Tab::Duplicates => duplicate_detail(app),
        Tab::Issues => issue_detail(app),
        Tab::Artifacts | Tab::Caches => app.current().map(|item| item_detail(app, item)),
    };
    shown.unwrap_or_else(|| vec![muted(empty_text(app))])
}

fn item_detail(app: &App, item: &Item) -> Vec<Line<'static>> {
    let candidate = &item.candidate;
    let mut first = vec![Span::raw(format!(" {}", display_path(&candidate.path)))];
    if candidate.usage.partial {
        first.push(Span::styled("  size is partial", Style::new().fg(MUTED)));
    }
    let second = match &item.status {
        Status::Failed(error) => format!(" failed: {error}"),
        _ if item.source == Source::Cache => format!(
            " {} · {} · {} entries{}",
            candidate.rule.name,
            candidate.rule.ecosystem,
            candidate.usage.entries,
            if item.note.is_empty() {
                String::new()
            } else {
                format!(" · {}", item.note)
            },
        ),
        _ => format!(
            " {} · {} · {} entries · project {}",
            candidate.rule.name,
            candidate.rule.ecosystem,
            candidate.usage.entries,
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
        Line::styled(format!(" {}", explain(candidate)), Style::new().fg(advice)),
        Line::from(second),
    ]
}

fn uncommitted(changes: usize) -> String {
    format!(
        "{changes} uncommitted change{}",
        if changes == 1 { "" } else { "s" }
    )
}

fn key_chip(key: &str) -> Span<'static> {
    Span::styled(
        format!(" {key} "),
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
        Overlay::Confirm { .. } | Overlay::Quit => ("Confirm", WARNING),
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

fn quit_prompt(app: &App) -> Line<'static> {
    let deleting = app.deleting();
    let mut spans = vec![
        mode_chip(app),
        Span::styled(
            format!(
                " Quit with {deleting} unfinished deletion{}? Queued ones are cancelled.  ",
                if deleting == 1 { "" } else { "s" }
            ),
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
    if app.filtering {
        let mut spans = vec![mode_chip(app), Span::raw(format!(" {}█   ", app.filter))];
        spans.extend(key_hints(
            app,
            Scope::TuiFilter,
            &[(Action::ApplyFilter, "apply"), (Action::Back, "clear")],
        ));
        return Line::from(spans);
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
    let delete_hint = if selected > 0 {
        (Action::Delete, "delete selected")
    } else {
        (Action::DeleteCursor, "delete this")
    };
    spans.extend(key_hints(
        app,
        Scope::Tui,
        &[
            (Action::Toggle, "select"),
            (Action::SelectAll, "select all safe"),
            delete_hint,
            (Action::SafeOnly, "safe only"),
            (Action::Category, "category"),
            (Action::FilterFocus, "search"),
            (Action::Help, "help"),
            (Action::Quit, "quit"),
        ],
    ));
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

fn confirm(frame: &mut Frame, app: &App, stats: &ConfirmStats, mode: crate::delete::Mode) {
    const DIRTY_SHOWN: usize = 3;
    let checking_lines = usize::from(stats.checking > 0 || stats.unknown);
    let dirty_lines = stats.dirty.len().min(DIRTY_SHOWN + 1);
    let area = centered(
        frame.area(),
        90,
        cells(stats.shown.len() + 10 + dirty_lines + checking_lines),
    );
    let inner_width = usize::from(area.width.saturating_sub(16));
    let mut lines = vec![Line::raw("")];
    for &index in &stats.shown {
        let candidate = &app.items[index].candidate;
        lines.push(Line::raw(format!(
            "  {:>10}  {}",
            ByteSize(candidate.usage.allocated).to_string(),
            shorten(&display_path(&candidate.path), inner_width)
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
        crate::delete::Mode::Trash => "Move to trash. Space is freed once the trash is emptied.",
        crate::delete::Mode::Permanent => "Delete permanently. This cannot be undone.",
        crate::delete::Mode::DryRun => "Dry run. Nothing is touched.",
    };
    lines.push(Line::styled(
        format!("  {action}"),
        Style::new().add_modifier(Modifier::BOLD),
    ));
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
                "  Still checking {} project{} for uncommitted changes.",
                stats.checking,
                if stats.checking == 1 { "" } else { "s" }
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
    lines.push(Line::from(key_hints(
        app,
        Scope::TuiConfirm,
        &[(Action::Confirm, "confirm"), (Action::Cancel, "cancel")],
    )));
    let title = format!(
        " Delete {} item{} · {} ",
        stats.count,
        if stats.count == 1 { "" } else { "s" },
        ByteSize(stats.bytes)
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::new().borders(Borders::ALL).title(title)),
        area,
    );
}

fn help(frame: &mut Frame, app: &App) {
    use Action::*;
    const SECTIONS: [HelpSection; 5] = [
        (
            "Views",
            &[
                (&[Artifacts], "project artifacts"),
                (&[Caches], "global tool caches"),
                (&[Duplicates], "duplicate files, report only"),
                (&[Issues], "folders that could not be read"),
            ],
        ),
        (
            "Navigate",
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
            &[
                (&[Toggle], "toggle and move down"),
                (&[SelectAll], "select every safe item in view"),
                (
                    &[SelectProject],
                    "project: safe, then add review, then clear",
                ),
                (&[SelectNone], "clear selection"),
            ],
        ),
        (
            "Filter",
            &[
                (&[SafeOnly], "show only items that are safe to delete"),
                (
                    &[Category],
                    "cycle category: dependencies, build, cache, temp",
                ),
                (&[FilterFocus], "search by path or rule"),
                (&[ResetFilters], "clear all filters"),
            ],
        ),
        (
            "Act",
            &[
                (
                    &[DeleteCursor],
                    "delete selection, or the item under the cursor",
                ),
                (&[Delete], "delete selection"),
                (&[CycleSort], "cycle sort: size, path, category"),
                (&[Group], "group by rule or by project"),
                (&[Back], "back out: filters, then selection"),
                (&[Quit], "quit, asks while deleting"),
            ],
        ),
    ];
    let keys_for = |actions: &'static [Action]| {
        actions
            .iter()
            .flat_map(|action| app.keymap.keys(Scope::Tui, *action))
    };
    let chips_width = |actions| {
        keys_for(actions)
            .map(|key| key.chars().count() + 3)
            .sum::<usize>()
    };
    let rows = SECTIONS.iter().flat_map(|(_, rows)| rows.iter());
    let key_width = rows
        .clone()
        .map(|(keys, _)| chips_width(keys))
        .max()
        .unwrap_or(0);
    let action_width = rows
        .clone()
        .map(|(_, action)| action.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();
    for (title, rows) in SECTIONS {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("  {}", title.to_uppercase()),
            Style::new().fg(MUTED).add_modifier(Modifier::BOLD),
        ));
        for (keys, action) in rows {
            let mut spans = vec![Span::raw("  ")];
            for key in keys_for(keys) {
                spans.push(key_chip(key));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::raw(" ".repeat(key_width - chips_width(keys) + 1)));
            spans.push(Span::raw(*action));
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("  any key to close", Style::new().fg(FAINT)));

    let width = cells(2 + key_width + 1 + action_width + 3);
    let area = centered(frame.area(), width, cells(lines.len()).saturating_add(2));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(FAINT))
                .title(Span::styled(
                    " Keys ",
                    Style::new().add_modifier(Modifier::BOLD),
                )),
        ),
        area,
    );
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
        Overlay::Help => help(frame, app),
        Overlay::None | Overlay::Quit => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
