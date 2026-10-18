use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Div, ElementId, IntoElement, ParentElement, Rgba,
    SharedString, Styled, div, prelude::*, relative,
};
use kuizer::stats::Bucket;

use crate::motion;
use crate::theme::{self, font, size, space, unit};
use crate::tooltip::{Tip, Tooltip};

const PULSE: Duration = Duration::from_millis(1_400);
const SWEEP: Duration = Duration::from_millis(1_100);
const SWEEP_WIDTH: f32 = 0.3;

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
        .h(unit(size::METER))
        .gap(unit(size::HAIR))
        .overflow_hidden()
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
                        .flex_shrink()
                        .bg(segment.fill)
                        .hover(|style| style.opacity(0.8))
                        .tip(segment_tip(segment, share))
                }),
        );
    let legend = div()
        .flex()
        .flex_wrap()
        .gap_x(unit(space::XL))
        .gap_y(unit(space::S))
        .text_size(unit(font::SMALL))
        .children(segments.iter().enumerate().map(|(index, segment)| {
            div()
                .id(ElementId::NamedInteger(legend_id(id), index as u64))
                .debug_selector(move || format!("{id}-legend-{index}"))
                .group("legend")
                .flex()
                .items_center()
                .gap(unit(space::S))
                .child(swatch(segment.fill))
                .child(
                    div()
                        .text_color(theme::muted())
                        .group_hover("legend", |style| style.text_color(theme::text()))
                        .child(segment.label.clone()),
                )
                .child(div().child(theme::bytes(segment.bytes)))
                .tip(segment_tip(segment, theme::share(segment.bytes, whole)))
        }));
    div()
        .flex()
        .flex_col()
        .gap(unit(space::L))
        .child(bar)
        .child(legend)
}

fn segment_tip(
    segment: &Segment,
    share: f32,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    Tooltip::details(segment.label.clone())
        .row("Size", theme::bytes(segment.bytes))
        .row("Share", percent(share))
        .build()
}

fn legend_id(id: &'static str) -> SharedString {
    format!("{id}-legend").into()
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
        .gap(unit(space::XS))
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
                .gap(unit(space::S))
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
                        .h(unit(size::COLUMNS))
                        .child(
                            div()
                                .w_full()
                                .h(relative(height))
                                .min_h(unit(size::MARKER))
                                .bg(fill)
                                .group_hover(format!("{id}-{index}"), |style| {
                                    style.bg(theme::primary())
                                }),
                        ),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(unit(font::CAPTION))
                        .text_color(theme::faint())
                        .child(bucket.label.clone()),
                )
        }))
}

pub fn swatch(fill: Rgba) -> Div {
    div().size(unit(size::SWATCH)).flex_none().bg(fill)
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
        .h(unit(size::MARKER))
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
