use std::collections::BTreeMap;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, TableState,
};

use super::app::{App, GroupKey, Grouping, Overlay, Status, project_of};
use crate::cli::SortKey;
use crate::delete::Mode;
use crate::detect::{Category, Safety};
use crate::output::display_path;
use crate::units::{ByteSize, age};

pub(super) const MUTED: Color = Color::Indexed(244);
const FAINT: Color = Color::Indexed(240);
const CURSOR: Color = Color::Indexed(237);
const WARNING: Color = Color::Yellow;
const SAFE: Color = Color::Green;
const SELECTED: Color = Color::Indexed(22);
const BAR: u16 = 8;
pub(super) const CURRENT_WIDTH: usize = 48;
type HelpRow = (&'static [&'static str], &'static str);
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

fn category_color(category: Category) -> Color {
    match category {
        Category::Dependencies => Color::Cyan,
        Category::Build => Color::Blue,
        Category::Cache => Color::Magenta,
        Category::Temp => Color::Indexed(180),
    }
}

fn size_color(bytes: u64) -> Color {
    match bytes {
        b if b >= 1 << 30 => Color::Red,
        b if b >= 100 << 20 => Color::Yellow,
        _ => Color::Reset,
    }
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
    if let Some(category) = app.category {
        spans.push(Span::styled(
            format!(" {} ", category.as_str()),
            Style::new().fg(Color::Black).bg(category_color(category)),
        ));
        spans.push(Span::raw(" "));
    }
    if app.safe_only {
        spans.push(Span::styled(
            " safe only ",
            Style::new().fg(Color::Black).bg(SAFE),
        ));
        spans.push(Span::raw(" "));
    }
    if !app.filter.is_empty() {
        spans.push(Span::styled(
            format!(" \"{}\" ", app.filter),
            Style::new().fg(Color::Black).bg(Color::White),
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
    let total: u64 = app
        .items
        .iter()
        .filter(|item| item.status != Status::Deleted)
        .map(|item| item.candidate.usage.allocated)
        .sum();
    let mut spans = vec![
        Span::styled(" kuizer ", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(
                " {state}  {} dirs  {} files  {} issues ",
                app.progress.dirs, app.progress.files, app.issues
            ),
            Style::new().fg(MUTED),
        ),
        Span::raw(format!(" {} found  {} ", app.items.len(), ByteSize(total))),
        Span::styled(
            format!(" sort {} ", sort_label(app.sort)),
            Style::new().fg(MUTED),
        ),
    ];
    if app.grouping == Grouping::Project {
        spans.push(Span::styled(" by project ", Style::new().fg(MUTED)));
    }
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

fn table(frame: &mut Frame, app: &App, area: Rect) {
    let path_width = usize::from(area.width.saturating_sub(FIXED_COLUMNS + 8));
    let keys: Vec<GroupKey> = app
        .view
        .iter()
        .map(|&index| app.group_key(&app.items[index].candidate))
        .collect();
    let mut groups: BTreeMap<&GroupKey, (usize, u64)> = BTreeMap::new();
    for (key, &index) in keys.iter().zip(&app.view) {
        let entry = groups.entry(key).or_default();
        entry.0 += 1;
        entry.1 += app.items[index].candidate.usage.allocated;
    }
    let largest = app
        .view
        .iter()
        .map(|&index| app.items[index].candidate.usage.allocated)
        .max()
        .unwrap_or(0);
    let mut rows = Vec::with_capacity(app.view.len() + groups.len());
    let mut previous_group = None;
    let mut selected_row = None;
    for (position, (group, &index)) in keys.iter().zip(&app.view).enumerate() {
        let item = &app.items[index];
        let candidate = &item.candidate;
        if previous_group != Some(group) {
            let (count, bytes) = groups[group];
            let summary = format!(
                "  · {count} item{}  · {}",
                if count == 1 { "" } else { "s" },
                ByteSize(bytes),
            );
            let warning = app
                .changes_in(candidate)
                .filter(|_| app.grouping == Grouping::Project)
                .map(|changes| format!("  · {}", uncommitted(changes)));
            let reserved =
                summary.chars().count() + warning.as_ref().map_or(0, |w| w.chars().count());
            let name = match group {
                (Some(category), rule) => format!("{} / {rule}", category.as_str()),
                (None, _) => shorten(
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
            rows.push(Row::new(cells).style(Style::new().fg(MUTED).add_modifier(Modifier::BOLD)));
            previous_group = Some(group);
        }
        if position == app.cursor {
            selected_row = Some(rows.len());
        }
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
        rows.push(
            Row::new(vec![
                Cell::from(mark.to_owned()),
                Cell::from(format!("{:>10}", ByteSize(bytes).to_string()))
                    .style(Style::new().fg(dim_if(size_color(bytes)))),
                Cell::from(size_bar(bytes, largest)).style(Style::new().fg(FAINT)),
                Cell::from(badge).style(Style::new().fg(dim_if(badge_color))),
                Cell::from(candidate.rule.category.as_str().to_owned())
                    .style(Style::new().fg(dim_if(category_color(candidate.rule.category)))),
                Cell::from(candidate.rule.name.clone()),
                Cell::from(age(candidate.newest)).style(Style::new().fg(MUTED)),
                Cell::from(shorten(&display_path(&candidate.path), path_width)),
            ])
            .style(style),
        );
    }
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
            Row::new([
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
    let mut state = TableState::default().with_selected(selected_row);
    frame.render_stateful_widget(table, area, &mut state);
}

fn detail(app: &App) -> Vec<Line<'static>> {
    let Some(item) = app.current() else {
        let text = if app.finished.is_some() {
            "Nothing found."
        } else {
            "Scanning, results appear as they are found."
        };
        return vec![Line::styled(format!(" {text}"), Style::new().fg(MUTED))];
    };
    let candidate = &item.candidate;
    let mut first = vec![Span::raw(format!(" {}", display_path(&candidate.path)))];
    if candidate.usage.partial {
        first.push(Span::styled("  size is partial", Style::new().fg(MUTED)));
    }
    let second = match &item.status {
        Status::Failed(error) => format!(" failed: {error}"),
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
            .fg(Color::White)
            .bg(Color::Indexed(238))
            .add_modifier(Modifier::BOLD),
    )
}

fn key_hints(hints: &[(&'static str, &'static str)]) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(hints.len() * 2);
    for (key, label) in hints {
        spans.push(key_chip(key));
        spans.push(Span::styled(format!(" {label}  "), Style::new().fg(MUTED)));
    }
    spans
}

fn footer(app: &App) -> Line<'static> {
    if app.filtering {
        let mut spans = vec![
            Span::styled(" filter ", Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}█   ", app.filter)),
        ];
        spans.extend(key_hints(&[("enter", "apply"), ("esc", "clear")]));
        return Line::from(spans);
    }
    let selected = app.selected().count();
    let mut spans = vec![Span::raw(" ")];
    let delete_hint = if selected > 0 {
        ("enter", "delete selected")
    } else {
        ("d", "delete this")
    };
    spans.extend(key_hints(&[
        ("space", "select"),
        ("a", "select all safe"),
        delete_hint,
        ("f", "safe only"),
        ("c", "category"),
        ("/", "search"),
        ("?", "help"),
        ("q", "quit"),
    ]));
    if app.filtered() {
        spans.extend(key_hints(&[("x", "clear filters")]));
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
    if let Some(message) = &app.message {
        spans.push(Span::styled(format!(" {message} "), Style::new().fg(MUTED)));
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

fn confirm(frame: &mut Frame, app: &App, ids: &[u64], mode: crate::delete::Mode) {
    const SHOWN: usize = 8;
    const DIRTY_SHOWN: usize = 3;
    let items: Vec<_> = ids.iter().filter_map(|&id| app.item(id)).collect();
    let bytes: u64 = items
        .iter()
        .map(|item| item.candidate.usage.allocated)
        .sum();
    let review = items
        .iter()
        .filter(|item| item.candidate.rule.safety == Safety::Review)
        .count();
    let dirty_lines = items
        .iter()
        .filter_map(|item| {
            app.changes_in(&item.candidate)
                .map(|_| project_of(&item.candidate))
        })
        .collect::<std::collections::HashSet<_>>()
        .len()
        .min(DIRTY_SHOWN + 1);
    let area = centered(
        frame.area(),
        90,
        cells(items.len().min(SHOWN) + 10 + dirty_lines),
    );
    let inner_width = usize::from(area.width.saturating_sub(16));
    let mut lines = vec![Line::raw("")];
    for item in items.iter().take(SHOWN) {
        lines.push(Line::raw(format!(
            "  {:>10}  {}",
            ByteSize(item.candidate.usage.allocated).to_string(),
            shorten(&display_path(&item.candidate.path), inner_width)
        )));
    }
    if items.len() > SHOWN {
        lines.push(Line::styled(
            format!("  and {} more", items.len() - SHOWN),
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
    if let (Some(free), Some(after)) = (app.free, app.free_after(bytes, mode)) {
        lines.push(Line::raw(format!(
            "  {} free now, {} free afterwards.",
            ByteSize(free),
            ByteSize(after)
        )));
    }
    if review > 0 {
        lines.push(Line::raw(format!(
            "  {review} of these are marked review: check they are generated before deleting."
        )));
    }
    let mut dirty: Vec<(String, usize)> = Vec::new();
    for item in &items {
        let project = display_path(project_of(&item.candidate));
        if let Some(changes) = app.changes_in(&item.candidate)
            && !dirty.iter().any(|(seen, _)| *seen == project)
        {
            dirty.push((project, changes));
        }
    }
    for (project, changes) in dirty.iter().take(DIRTY_SHOWN) {
        let note = format!(" has {}.", uncommitted(*changes));
        let width = inner_width.saturating_sub(note.chars().count());
        lines.push(Line::styled(
            format!("  {}{note}", shorten(project, width)),
            Style::new().fg(WARNING),
        ));
    }
    if dirty.len() > DIRTY_SHOWN {
        lines.push(Line::styled(
            format!(
                "  and {} more projects with uncommitted changes.",
                dirty.len() - DIRTY_SHOWN
            ),
            Style::new().fg(WARNING),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  y confirm   t trash or permanent   n cancel",
        Style::new().fg(MUTED),
    ));
    let title = format!(
        " Delete {} item{} · {} ",
        items.len(),
        if items.len() == 1 { "" } else { "s" },
        ByteSize(bytes)
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::new().borders(Borders::ALL).title(title)),
        area,
    );
}

fn help(frame: &mut Frame) {
    const SECTIONS: [HelpSection; 4] = [
        (
            "Navigate",
            &[
                (&["j", "k", "↑", "↓"], "move"),
                (&["g", "home"], "first"),
                (&["G", "end"], "last"),
                (&["pgup", "pgdn"], "page"),
            ],
        ),
        (
            "Select",
            &[
                (&["space"], "toggle and move down"),
                (&["a"], "select every safe item in view"),
                (&["P"], "select every item in the cursor's project"),
                (&["u"], "clear selection"),
            ],
        ),
        (
            "Filter",
            &[
                (&["f"], "show only items that are safe to delete"),
                (&["c"], "cycle category: dependencies, build, cache, temp"),
                (&["/"], "search by path or rule"),
                (&["x"], "clear all filters"),
            ],
        ),
        (
            "Act",
            &[
                (
                    &["d", "del"],
                    "delete selection, or the item under the cursor",
                ),
                (&["enter"], "delete selection"),
                (&["s"], "cycle sort: size, path, category"),
                (&["p"], "group by rule or by project"),
                (&["q", "esc", "ctrl-c"], "quit and stop the scan"),
            ],
        ),
    ];
    fn chips_width(keys: &[&str]) -> usize {
        keys.iter().map(|key| key.chars().count() + 3).sum()
    }
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
            for key in *keys {
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
    let [top, main, info, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(Paragraph::new(header(app)), top);
    table(frame, app, main);
    frame.render_widget(Paragraph::new(detail(app)), info);
    frame.render_widget(Paragraph::new(footer(app)), bottom);
    match &app.overlay {
        Overlay::Confirm { ids, mode } => confirm(frame, app, ids, *mode),
        Overlay::Help => help(frame),
        Overlay::None => {}
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
