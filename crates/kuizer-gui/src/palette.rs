use gpui::ScrollHandle;

use crate::commands::Spec;

/// The open command palette: what was typed and which row is highlighted.
#[derive(Default)]
pub struct Palette {
    pub query: String,
    pub active: usize,
    pub scroll: ScrollHandle,
}

impl Palette {
    /// Moves the highlight by `step` rows, wrapping around at either end.
    pub fn step(&mut self, step: isize, count: usize) {
        if count == 0 {
            self.active = 0;
            return;
        }
        let count = count.cast_signed();
        let active = self.active.min(count.cast_unsigned() - 1).cast_signed();
        self.active = (active + step).rem_euclid(count).cast_unsigned();
    }
}

/// @name rank
/// @description Keeps the specs whose label fuzzily matches the query, best match first. Equal
/// scores keep the table order, and an empty query keeps every spec in table order.
///
/// @example
/// let shown = rank("resc", commands::table());
pub fn rank(query: &str, specs: Vec<Spec>) -> Vec<Spec> {
    let mut scored: Vec<(i32, usize, Spec)> = specs
        .into_iter()
        .enumerate()
        .filter_map(|(index, spec)| Some((score(query, &spec.label)?, index, spec)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, spec)| spec).collect()
}

const MATCH: i32 = 1;
const WORD_START: i32 = 8;
const CONSECUTIVE: i32 = 5;
const GAP: i32 = 2;
const LATE_START: i32 = 4;
/// One point off for every this many label characters the query leaves out, so a tighter match
/// wins a tie.
const UNMATCHED: usize = 4;

fn word_start(chars: &[char], index: usize) -> bool {
    index == 0
        || matches!(chars[index - 1], ' ' | '/' | '-' | '_' | '.' | '~')
        || (chars[index].is_uppercase() && chars[index - 1].is_lowercase())
}

/// @name score
/// @description Fuzzy-matches a query against a label: every query character has to appear in
/// the label in order, ignoring case and the spaces in the query. Matches at word starts and runs
/// of consecutive characters score higher, and so do shorter labels, so `zi` prefers `Zoom in`
/// and `resc` prefers `Rescan`. Returns `None` when the label does not match.
///
/// @example
/// assert!(score("zi", "Zoom in") > score("zi", "Go to Duplicates, size"));
pub fn score(query: &str, label: &str) -> Option<i32> {
    let query: Vec<char> = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = label.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|ch| ch.to_lowercase().next().unwrap_or(*ch))
        .collect();
    let mut previous: Vec<Option<i32>> = vec![None; lower.len()];
    for (step, &wanted) in query.iter().enumerate() {
        let mut current: Vec<Option<i32>> = vec![None; lower.len()];
        let mut best_before: Option<i32> = None;
        for (index, &ch) in lower.iter().enumerate() {
            if ch == wanted {
                let gain = MATCH
                    + if word_start(&chars, index) {
                        WORD_START
                    } else {
                        0
                    };
                current[index] = if step == 0 {
                    let late = i32::try_from(index).unwrap_or(i32::MAX).min(LATE_START);
                    Some(gain - late)
                } else {
                    let run = index
                        .checked_sub(1)
                        .and_then(|before| previous[before])
                        .map(|before| before + gain + CONSECUTIVE);
                    let jump = best_before.map(|before| before + gain - GAP);
                    run.max(jump)
                };
            }
            if step > 0
                && let Some(score) = previous[index]
            {
                best_before = Some(best_before.map_or(score, |best| best.max(score)));
            }
        }
        previous = current;
    }
    let unmatched = lower.len().saturating_sub(query.len()) / UNMATCHED;
    let unmatched = i32::try_from(unmatched).unwrap_or(i32::MAX);
    previous
        .into_iter()
        .flatten()
        .max()
        .map(|best| best - unmatched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{Command, table};

    #[test]
    fn every_character_has_to_appear_in_order() {
        assert!(score("rsc", "Rescan").is_some());
        assert!(score("RESCAN", "rescan").is_some());
        assert!(score("go dup", "Go to Duplicates").is_some());
        assert!(score("csr", "Rescan").is_none());
        assert!(score("x", "Rescan").is_none());
        assert_eq!(score("", "Rescan"), Some(0));
    }

    #[test]
    fn word_starts_and_runs_score_higher() {
        let zoom = score("zi", "Zoom in").unwrap();
        let other = score("zi", "Go to Duplicates, size").unwrap_or(i32::MIN);
        assert!(zoom > other);
        assert!(score("resc", "Rescan").unwrap() > score("resc", "Reset zoom scale").unwrap());
        assert!(score("hist", "Go to History").unwrap() > score("hist", "Hide this tab").unwrap());
        assert!(score("res", "Rescan").unwrap() > score("res", "Go to Issues").unwrap_or(0));
    }

    #[test]
    fn rank_puts_the_best_match_first_and_drops_the_rest() {
        let ranked = rank("trash", table());
        assert_eq!(ranked[0].command, Command::Trash);
        assert!(
            ranked
                .iter()
                .all(|spec| score("trash", &spec.label).is_some())
        );
        let all = rank("", table());
        assert_eq!(all.len(), table().len());
        assert_eq!(all[0].command, table()[0].command);
        assert!(rank("qqqq", table()).is_empty());
    }

    #[test]
    fn the_highlight_wraps() {
        let mut palette = Palette::default();
        palette.step(-1, 5);
        assert_eq!(palette.active, 4);
        palette.step(1, 5);
        assert_eq!(palette.active, 0);
        palette.active = 9;
        palette.step(1, 3);
        assert_eq!(palette.active, 0);
        palette.step(1, 0);
        assert_eq!(palette.active, 0);
    }
}
