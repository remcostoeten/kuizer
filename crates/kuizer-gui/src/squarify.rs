//! The squarified treemap layout (Bruls, Huizing and van Wijk), as pure functions over plain
//! rectangles so it can be tested without a window.

/// A rectangle in any unit, with its origin at the top left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }

    /// The area two rectangles share, zero when they only touch.
    #[cfg(test)]
    pub fn overlap(&self, other: &Rect) -> f32 {
        let w = self.right().min(other.right()) - self.x.max(other.x);
        let h = self.bottom().min(other.bottom()) - self.y.max(other.y);
        if w > 0. && h > 0. { w * h } else { 0. }
    }
}

/// A direction the keyboard moves the focus tile in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// @name squarify
/// @description Splits `bounds` into one rectangle per size, each with an area proportional to
/// its size, laid out in rows that keep the tiles as close to square as possible. The result is
/// in the same order as `sizes`, which should be sorted largest first. Together the rectangles
/// fill `bounds` without overlapping; a size of zero gets an empty rectangle.
///
/// @example
/// let rects = squarify(&[6, 6, 4, 3, 2, 2, 1], Rect::new(0., 0., 6., 4.));
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "tile sizes only need a few significant digits"
)]
pub fn squarify(sizes: &[u64], bounds: Rect) -> Vec<Rect> {
    let mut rects = vec![Rect::new(bounds.x, bounds.y, 0., 0.); sizes.len()];
    let total: f64 = sizes.iter().map(|&size| size as f64).sum();
    if total <= 0. || bounds.w <= 0. || bounds.h <= 0. {
        return rects;
    }
    let scale = f64::from(bounds.w) * f64::from(bounds.h) / total;
    let items: Vec<(usize, f64)> = sizes
        .iter()
        .enumerate()
        .filter(|(_, size)| **size > 0)
        .map(|(index, &size)| (index, size as f64 * scale))
        .collect();
    let (mut x, mut y) = (f64::from(bounds.x), f64::from(bounds.y));
    let (mut w, mut h) = (f64::from(bounds.w), f64::from(bounds.h));
    let mut start = 0;
    while start < items.len() {
        let side = w.min(h);
        let mut end = start + 1;
        let mut sum = items[start].1;
        while end < items.len() {
            let next = sum + items[end].1;
            if worst(&items[start..=end], next, side) > worst(&items[start..end], sum, side) {
                break;
            }
            sum = next;
            end += 1;
        }
        let last_row = end == items.len();
        let row = &items[start..end];
        if w >= h {
            let width = if last_row { w } else { sum / h };
            let mut top = y;
            for (position, &(index, area)) in row.iter().enumerate() {
                let height = if position + 1 == row.len() {
                    y + h - top
                } else {
                    area / width
                };
                rects[index] = Rect::new(x as f32, top as f32, width as f32, height as f32);
                top += height;
            }
            x += width;
            w -= width;
        } else {
            let height = if last_row { h } else { sum / w };
            let mut left = x;
            for (position, &(index, area)) in row.iter().enumerate() {
                let width = if position + 1 == row.len() {
                    x + w - left
                } else {
                    area / height
                };
                rects[index] = Rect::new(left as f32, y as f32, width as f32, height as f32);
                left += width;
            }
            y += height;
            h -= height;
        }
        start = end;
    }
    rects
}

/// The worst aspect ratio in a row of areas laid along `side`.
fn worst(row: &[(usize, f64)], sum: f64, side: f64) -> f64 {
    let largest = row.iter().map(|item| item.1).fold(0., f64::max);
    let smallest = row.iter().map(|item| item.1).fold(f64::INFINITY, f64::min);
    let side = side * side;
    let sum = sum * sum;
    (side * largest / sum).max(sum / (side * smallest))
}

/// @name neighbor
/// @description The tile the focus moves to from `from` in `direction`: the nearest tile on
/// that side, preferring tiles that line up with the current one. `None` at the edge.
///
/// @example
/// let next = neighbor(&rects, 0, Direction::Right);
pub fn neighbor(rects: &[Rect], from: usize, direction: Direction) -> Option<usize> {
    const EDGE: f32 = 0.5;
    let current = rects.get(from)?;
    let (cx, cy) = current.center();
    rects
        .iter()
        .enumerate()
        .filter(|(index, rect)| *index != from && rect.area() > 0.)
        .filter_map(|(index, rect)| {
            let (gap, along, offset) = match direction {
                Direction::Right if rect.x >= current.right() - EDGE => (
                    rect.x - current.right(),
                    spread(current.y, current.bottom(), rect.y, rect.bottom()),
                    (rect.center().1 - cy).abs(),
                ),
                Direction::Left if rect.right() <= current.x + EDGE => (
                    current.x - rect.right(),
                    spread(current.y, current.bottom(), rect.y, rect.bottom()),
                    (rect.center().1 - cy).abs(),
                ),
                Direction::Down if rect.y >= current.bottom() - EDGE => (
                    rect.y - current.bottom(),
                    spread(current.x, current.right(), rect.x, rect.right()),
                    (rect.center().0 - cx).abs(),
                ),
                Direction::Up if rect.bottom() <= current.y + EDGE => (
                    current.y - rect.bottom(),
                    spread(current.x, current.right(), rect.x, rect.right()),
                    (rect.center().0 - cx).abs(),
                ),
                _ => return None,
            };
            Some((index, gap.max(0.) + along * 2. + offset * 0.01))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// How far apart two ranges are, zero when they overlap.
fn spread(a_start: f32, a_end: f32, b_start: f32, b_end: f32) -> f32 {
    (b_start - a_end).max(a_start - b_end).max(0.)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 1e-3;

    fn check(sizes: &[u64], bounds: Rect) -> Vec<Rect> {
        let rects = squarify(sizes, bounds);
        assert_eq!(rects.len(), sizes.len());
        let total: u64 = sizes.iter().sum();
        #[allow(clippy::cast_precision_loss, reason = "test sizes are small")]
        for (rect, &size) in rects.iter().zip(sizes) {
            let want = bounds.area() * size as f32 / total as f32;
            assert!(
                (rect.area() - want).abs() <= want.max(1.) * TOLERANCE,
                "{size} got {} instead of {want}",
                rect.area()
            );
            assert!(rect.x >= bounds.x - TOLERANCE && rect.right() <= bounds.right() + TOLERANCE);
            assert!(rect.y >= bounds.y - TOLERANCE && rect.bottom() <= bounds.bottom() + TOLERANCE);
        }
        for (index, a) in rects.iter().enumerate() {
            for b in &rects[index + 1..] {
                assert!(a.overlap(b) <= bounds.area() * 1e-6, "{a:?} overlaps {b:?}");
            }
        }
        let filled: f32 = rects.iter().map(Rect::area).sum();
        assert!((filled - bounds.area()).abs() <= bounds.area() * TOLERANCE);
        rects
    }

    #[test]
    fn areas_follow_the_sizes_and_fill_the_bounds() {
        check(&[6, 6, 4, 3, 2, 2, 1], Rect::new(0., 0., 6., 4.));
        check(&[100], Rect::new(10., 20., 300., 200.));
        check(&[50, 30, 10, 5, 3, 1, 1], Rect::new(0., 0., 1000., 600.));
        check(&[7, 7, 7, 7], Rect::new(0., 0., 200., 900.));
        let many: Vec<u64> = (1..=300).rev().collect();
        check(&many, Rect::new(5., 5., 1280., 640.));
    }

    #[test]
    fn the_classic_example_keeps_tiles_square() {
        let rects = check(&[6, 6, 4, 3, 2, 2, 1], Rect::new(0., 0., 6., 4.));
        assert_eq!(rects[0], Rect::new(0., 0., 3., 2.));
        assert_eq!(rects[1], Rect::new(0., 2., 3., 2.));
        for rect in &rects {
            let ratio = rect.w.max(rect.h) / rect.w.min(rect.h);
            assert!(ratio < 3., "{rect:?} is too thin");
        }
    }

    #[test]
    fn the_layout_is_stable_and_skips_empty_sizes() {
        let sizes = [40, 30, 0, 20, 10, 0];
        let bounds = Rect::new(0., 0., 400., 300.);
        let first = squarify(&sizes, bounds);
        assert_eq!(first, squarify(&sizes, bounds), "same input, same layout");
        assert!(first[2].area().abs() < f32::EPSILON);
        assert!(first[5].area().abs() < f32::EPSILON);
        check(&[40, 30, 20, 10], bounds);
        assert!(squarify(&[], bounds).is_empty());
        assert!(
            squarify(&[0, 0], bounds)
                .iter()
                .all(|rect| rect.area() == 0.)
        );
        assert!(
            squarify(&[5], Rect::new(0., 0., 0., 10.))
                .iter()
                .all(|rect| rect.area() == 0.)
        );
    }

    #[test]
    fn neighbors_follow_the_arrow_keys() {
        // A 2 by 2 grid: 0 1 / 2 3.
        let rects = [
            Rect::new(0., 0., 10., 10.),
            Rect::new(10., 0., 10., 10.),
            Rect::new(0., 10., 10., 10.),
            Rect::new(10., 10., 10., 10.),
        ];
        assert_eq!(neighbor(&rects, 0, Direction::Right), Some(1));
        assert_eq!(neighbor(&rects, 0, Direction::Down), Some(2));
        assert_eq!(neighbor(&rects, 3, Direction::Left), Some(2));
        assert_eq!(neighbor(&rects, 3, Direction::Up), Some(1));
        assert_eq!(neighbor(&rects, 0, Direction::Left), None);
        assert_eq!(neighbor(&rects, 0, Direction::Up), None);
        assert_eq!(neighbor(&rects, 9, Direction::Up), None);
    }
}
