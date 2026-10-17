use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Div, ElementId, IntoElement, ParentElement, Rgba,
    SharedString, Styled, div, prelude::*, px, relative,
};
use kuizer::stats::Bucket;

use crate::motion;
use crate::theme::{self, unit};
use crate::tooltip::{Tip, Tooltip};

const PULSE: Duration = Duration::from_millis(1_400);
const SWEEP: Duration = Duration::from_millis(1_100);
const SWEEP_WIDTH: f32 = 0.3;
const COLUMN_HEIGHT: f32 = 88.;

pub struct Segment {
    pub label: SharedString,
    pub bytes: u64,
    pub fill: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    Bytes,
    Count,
}

impl Measure {
    fn of(self, bucket: &Bucket) -> u64 {
        match self {
            Self::Bytes => bucket.bytes,
            Self::Count => bucket.count as u64,
        }
    }
}

/// @name composition
/// @description A stacked bar of `segments` as parts of `whole`, with a legend underneath.
/// Each part shows its size and share in a tooltip. Parts too small to see keep a sliver so
/// they can still be hovered.
///
/// @example
/// composition("disk", &segments, disk_size)
pub fn composition(id: &'static str, segments: &[Segment], whole: u64) -> Div {
    let bar = div()
        .flex()
        .w_full()
        .h(unit(12.))
        .gap(px(1.))
        .bg(theme::track())
        .children(
            segments
                .iter()
                .enumerate()
                .filter(|(_, segment)| segment.bytes > 0)
                .map(|(index, segment)| {
                    let share = theme::share(segment.bytes, whole);
                    div()
                        .id((id, index))
                        .debug_selector(move || format!("{id}-{index}"))
                        .h_full()
                        .w(relative(share.max(0.004)))
                        .flex_none()
                        .bg(segment.fill)
                        .hover(|style| style.opacity(0.8))
                        .tip(
                            Tooltip::details(segment.label.clone())
                                .row("Size", theme::bytes(segment.bytes))
                                .row("Share", percent(share))
                                .build(),
                        )
                }),
        );
    let legend = div()
        .flex()
        .flex_wrap()
        .gap_x(unit(16.))
        .gap_y(unit(6.))
        .text_size(unit(12.))
        .children(segments.iter().map(|segment| {
            div()
                .flex()
                .items_center()
                .gap(unit(6.))
                .child(swatch(segment.fill))
                .child(
                    div()
                        .text_color(theme::muted())
                        .child(segment.label.clone()),
                )
                .child(div().child(theme::bytes(segment.bytes)))
        }));
    div()
        .flex()
        .flex_col()
        .gap(unit(10.))
        .child(bar)
        .child(legend)
}

/// @name columns
/// @description A column chart of `buckets`, scaled to the largest one by `measure`. Every
/// column shows its exact size and item count in a tooltip; `counted` labels the item count.
///
/// @example
/// columns("ages", &stats::by_age(&candidates, now), Measure::Bytes, "Items")
pub fn columns(
    id: &'static str,
    buckets: &[Bucket],
    measure: Measure,
    counted: &'static str,
) -> Div {
    let largest = buckets
        .iter()
        .map(|bucket| measure.of(bucket))
        .max()
        .unwrap_or(0);
    div()
        .flex()
        .gap(unit(4.))
        .w_full()
        .children(buckets.iter().enumerate().map(|(index, bucket)| {
            let value = measure.of(bucket);
            let height = theme::share(value, largest);
            let fill = if value == 0 {
                theme::border()
            } else if value == largest {
                theme::bar_strong()
            } else {
                theme::bar()
            };
            div()
                .id((id, index))
                .debug_selector(move || format!("{id}-{index}"))
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(unit(6.))
                .group(SharedString::from(format!("{id}-{index}")))
                .tip(
                    Tooltip::details(bucket.label.clone())
                        .row("Size", theme::bytes(bucket.bytes))
                        .row(counted, bucket.count.to_string())
                        .build(),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .justify_end()
                        .h(unit(COLUMN_HEIGHT))
                        .child(
                            div()
                                .w_full()
                                .h(relative(height))
                                .min_h(px(2.))
                                .bg(fill)
                                .group_hover(format!("{id}-{index}"), |style| {
                                    style.bg(theme::primary())
                                }),
                        ),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(unit(11.))
                        .text_color(theme::faint())
                        .child(bucket.label.clone()),
                )
        }))
}

pub fn swatch(fill: Rgba) -> Div {
    div().size(unit(8.)).flex_none().bg(fill)
}

/// @name block
/// @description A skeleton placeholder of the given size that breathes while content loads,
/// or stays still when motion is off.
///
/// @example
/// block("stat-value", 0, unit(90.), unit(20.))
pub fn block(
    id: &'static str,
    index: usize,
    width: impl Into<gpui::Length>,
    height: impl Into<gpui::Length>,
) -> AnyElement {
    let shape = div()
        .w(width.into())
        .h(height.into())
        .flex_none()
        .bg(theme::skeleton());
    breathe(ElementId::from((id, index)), shape)
}

pub fn breathe(id: impl Into<ElementId>, element: Div) -> AnyElement {
    if motion::reduced() {
        return element.opacity(0.8).into_any_element();
    }
    element
        .with_animation(
            id,
            Animation::new(PULSE)
                .repeat()
                .with_easing(gpui::pulsating_between(0.45, 1.)),
            |element, delta| element.opacity(delta),
        )
        .into_any_element()
}

/// @name loader
/// @description The global loader: a thin bar with a sweep moving across while work runs in
/// the background. Without motion it is a still bar.
///
/// @example
/// root.when(busy, |this| this.child(loader()))
pub fn loader() -> AnyElement {
    let track = div()
        .debug_selector(|| "loader".to_owned())
        .relative()
        .w_full()
        .h(px(2.))
        .overflow_hidden()
        .bg(theme::track());
    if motion::reduced() {
        return track
            .child(div().size_full().bg(theme::bar_soft()))
            .into_any_element();
    }
    track
        .child(
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(SWEEP_WIDTH))
                .bg(theme::bar())
                .with_animation(
                    "loader-sweep",
                    Animation::new(SWEEP)
                        .repeat()
                        .with_easing(motion::ease_in_out),
                    |sweep, delta| sweep.left(relative(delta * (1. + SWEEP_WIDTH) - SWEEP_WIDTH)),
                ),
        )
        .into_any_element()
}

pub fn percent(share: f32) -> String {
    let value = share * 100.;
    if value > 0. && value < 1. {
        "under 1%".to_owned()
    } else {
        format!("{value:.0}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_round_and_mark_slivers() {
        assert_eq!(percent(0.), "0%");
        assert_eq!(percent(0.004), "under 1%");
        assert_eq!(percent(0.256), "26%");
        assert_eq!(percent(1.), "100%");
    }

    #[test]
    fn columns_measure_bytes_or_counts() {
        let bucket = Bucket {
            label: "x".to_owned(),
            bytes: 10,
            count: 2,
        };
        assert_eq!(Measure::Bytes.of(&bucket), 10);
        assert_eq!(Measure::Count.of(&bucket), 2);
    }
}
