use std::io;
use std::time::Duration;

use crossbeam_channel::{RecvTimeoutError, TryRecvError};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row, Table};

use super::view::{CURRENT_WIDTH, MUTED, cells, shorten};
use crate::detect::Safety;
use crate::output::display_path;
use crate::scan::{Candidate, Collected, Event, Issue, Scan, Summary};
use crate::units::ByteSize;

const FRAME: Duration = Duration::from_millis(50);
const EVENTS_PER_FRAME: usize = 5_000;
const FIXED_COLUMNS: u16 = 10 + 6 + 12 + 20 + 4;

#[derive(Debug, Default)]
struct Scroll {
    offset: usize,
    height: usize,
    follow: bool,
}

impl Scroll {
    fn last_offset(&self, total: usize) -> usize {
        total.saturating_sub(self.height)
    }

    fn fit(&mut self, total: usize) {
        let last = self.last_offset(total);
        if self.follow || self.offset >= last {
            self.offset = last;
        }
    }

    fn by(&mut self, delta: isize, total: usize) {
        let last = self.last_offset(total);
        self.offset = self.offset.saturating_add_signed(delta).min(last);
        self.follow = self.offset == last;
    }

    fn top(&mut self) {
        self.offset = 0;
        self.follow = false;
    }

    fn bottom(&mut self, total: usize) {
        self.offset = self.last_offset(total);
        self.follow = true;
    }

    fn page(&self) -> isize {
        isize::try_from(self.height.max(1)).unwrap_or(isize::MAX)
    }
}

struct Watch {
    rows: Vec<usize>,
    candidates: Vec<Candidate>,
    total: u64,
    safe: u64,
    issues: Vec<Issue>,
    summary: Option<Summary>,
    safe_only: bool,
    scroll: Scroll,
}

impl Watch {
    fn take(&mut self, event: Event) {
        match event {
            Event::Candidate(candidate) => {
                let is_safe = candidate.rule.safety == Safety::Safe;
                if !self.safe_only || is_safe {
                    self.rows.push(self.candidates.len());
                    self.total += candidate.usage.allocated;
                    if is_safe {
                        self.safe += candidate.usage.allocated;
                    }
                }
                self.candidates.push(candidate);
            }
            Event::Issue(issue) => self.issues.push(issue),
            Event::Finished(summary) => self.summary = Some(summary),
        }
    }
}

fn row(candidate: &Candidate, path_width: usize) -> Row<'static> {
    let mut path = shorten(&display_path(&candidate.path), path_width);
    if candidate.usage.partial {
        path.push_str(" (partial)");
    }
    let style = match candidate.rule.safety {
        Safety::Safe => Style::new(),
        Safety::Review => Style::new().fg(MUTED),
    };
    Row::new(vec![
        Span::styled(
            format!("{:>10}", ByteSize(candidate.usage.allocated).to_string()),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::raw(candidate.rule.safety.as_str()),
        Span::raw(candidate.rule.category.as_str()),
        Span::raw(candidate.rule.name.clone()),
        Span::raw(path),
    ])
    .style(style)
}

fn header(watch: &Watch, scan: &Scan) -> Line<'static> {
    let progress = watch
        .summary
        .as_ref()
        .map_or_else(|| scan.progress(), |summary| summary.progress);
    let state = match &watch.summary {
        Some(summary) if summary.cancelled => "cancelled".to_owned(),
        Some(summary) => format!("done in {:.1}s", summary.duration.as_secs_f64()),
        None => "scanning".to_owned(),
    };
    let mut spans = vec![
        Span::styled(" kuizer scan ", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(
                " {state}  {} dirs  {} files  {} issues ",
                progress.dirs,
                progress.files,
                watch.issues.len()
            ),
            Style::new().fg(MUTED),
        ),
    ];
    if watch.summary.is_none()
        && let Some(folder) = scan.current()
    {
        spans.push(Span::styled(
            format!(" in {} ", shorten(&display_path(&folder), CURRENT_WIDTH)),
            Style::new().fg(MUTED),
        ));
    }
    Line::from(spans)
}

fn footer(watch: &Watch) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(
                " {} found  {}  {} safe ",
                watch.rows.len(),
                ByteSize(watch.total),
                ByteSize(watch.safe)
            ),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " j k scroll  ^d ^u half page  g G top bottom  q quit ",
            Style::new().fg(MUTED),
        ),
    ])
}

fn position(watch: &Watch) -> String {
    let total = watch.rows.len();
    if total == 0 {
        return String::new();
    }
    let first = watch.scroll.offset + 1;
    let last = (watch.scroll.offset + watch.scroll.height).min(total);
    let mode = if watch.scroll.follow { " follow" } else { "" };
    format!("{first}-{last} of {total}{mode} ")
}

fn draw(frame: &mut Frame, watch: &mut Watch, scan: &Scan) {
    let [top, main, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    watch.scroll.height = usize::from(main.height.saturating_sub(1));
    watch.scroll.fit(watch.rows.len());

    frame.render_widget(Paragraph::new(header(watch, scan)), top);

    let path_width = usize::from(main.width.saturating_sub(FIXED_COLUMNS + 11));
    let rows: Vec<Row> = watch
        .rows
        .iter()
        .skip(watch.scroll.offset)
        .take(watch.scroll.height)
        .map(|&index| row(&watch.candidates[index], path_width))
        .collect();
    let widths = [
        Constraint::Length(10),
        Constraint::Length(6),
        Constraint::Length(12),
        Constraint::Length(20),
        Constraint::Fill(1),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(["      SIZE", "SAFETY", "CATEGORY", "RULE", "PATH"])
                .style(Style::new().fg(MUTED)),
        )
        .column_spacing(1);
    frame.render_widget(table, main);

    let position = position(watch);
    let [keys, place] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(cells(position.len())),
    ])
    .areas(bottom);
    frame.render_widget(Paragraph::new(footer(watch)), keys);
    frame.render_widget(
        Paragraph::new(Span::styled(position, Style::new().fg(MUTED))),
        place,
    );
}

fn handle_key(watch: &mut Watch, code: KeyCode, modifiers: KeyModifiers) -> bool {
    let total = watch.rows.len();
    let page = watch.scroll.page();
    let control = modifiers.contains(KeyModifiers::CONTROL);
    let scroll = &mut watch.scroll;
    match code {
        KeyCode::Char('c') if control => return true,
        KeyCode::Char('d') if control => scroll.by(page / 2, total),
        KeyCode::Char('u') if control => scroll.by(-page / 2, total),
        KeyCode::Char('f') if control => scroll.by(page, total),
        KeyCode::Char('b') if control => scroll.by(-page, total),
        KeyCode::Char('e') if control => scroll.by(1, total),
        KeyCode::Char('y') if control => scroll.by(-1, total),
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('j') | KeyCode::Down => scroll.by(1, total),
        KeyCode::Char('k') | KeyCode::Up => scroll.by(-1, total),
        KeyCode::PageDown | KeyCode::Char(' ') => scroll.by(page, total),
        KeyCode::PageUp => scroll.by(-page, total),
        KeyCode::Char('g') | KeyCode::Home => scroll.top(),
        KeyCode::Char('G') | KeyCode::End => scroll.bottom(total),
        _ => {}
    }
    false
}

fn finish(scan: &Scan, watch: &mut Watch) -> Summary {
    if let Some(summary) = watch.summary.take() {
        return summary;
    }
    scan.cancel();
    loop {
        match scan.events.recv_timeout(Duration::from_secs(5)) {
            Ok(Event::Finished(summary)) => return summary,
            Ok(event) => watch.take(event),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                return Summary {
                    progress: scan.progress(),
                    duration: Duration::ZERO,
                    cancelled: true,
                };
            }
        }
    }
}

/// @name watch
/// @description Shows a running scan full screen with a fixed header and footer and a
/// scrollable result list, then returns everything collected once the user quits. Quitting
/// before the scan ends cancels it.
///
/// @example
/// let collected = tui::watch(&handle, args.safe_only)?;
pub fn watch(scan: &Scan, safe_only: bool) -> io::Result<Collected> {
    let mut watch = Watch {
        rows: Vec::new(),
        candidates: Vec::new(),
        total: 0,
        safe: 0,
        issues: Vec::new(),
        summary: None,
        safe_only,
        scroll: Scroll {
            follow: true,
            ..Scroll::default()
        },
    };
    let mut terminal = ratatui::try_init()?;
    let outcome = (|| -> io::Result<()> {
        loop {
            for _ in 0..EVENTS_PER_FRAME {
                if watch.summary.is_some() {
                    break;
                }
                match scan.events.try_recv() {
                    Ok(event) => watch.take(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        watch.summary = Some(Summary {
                            progress: scan.progress(),
                            duration: Duration::ZERO,
                            cancelled: true,
                        });
                    }
                }
            }
            terminal.draw(|frame| draw(frame, &mut watch, scan))?;
            if event::poll(FRAME)?
                && let TermEvent::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && handle_key(&mut watch, key.code, key.modifiers)
            {
                return Ok(());
            }
        }
    })();
    ratatui::restore();
    outcome?;
    let summary = finish(scan, &mut watch);
    Ok(Collected {
        candidates: watch.candidates,
        issues: watch.issues,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll(height: usize) -> Scroll {
        Scroll {
            offset: 0,
            height,
            follow: true,
        }
    }

    #[test]
    fn follows_new_rows_until_scrolled_up() {
        let mut scroll = scroll(10);
        scroll.fit(25);
        assert_eq!(scroll.offset, 15);
        scroll.by(-3, 25);
        assert!(!scroll.follow);
        scroll.fit(40);
        assert_eq!(scroll.offset, 12);
        scroll.bottom(40);
        scroll.fit(50);
        assert_eq!(scroll.offset, 40);
    }

    #[test]
    fn clamps_to_the_list() {
        let mut scroll = scroll(10);
        scroll.by(-5, 4);
        assert_eq!(scroll.offset, 0);
        scroll.top();
        scroll.by(100, 30);
        assert_eq!(scroll.offset, 20);
        assert!(scroll.follow);
    }
}
