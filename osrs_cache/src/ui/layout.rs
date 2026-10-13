//! IF3 size and position modes.
//!
//! Size modes (RuneLite `WidgetSizeMode`, verified): 0 absolute, 1 `parent - raw`, 2 `parent * raw / 16384`.
//! Mode 4 (aspect: width from height or vice versa) is not in RuneLite's constants; it is the client's
//! `alignWidgetSize` behaviour as remembered, and no cache component uses it for a layer.
//! Position modes (RuneLite `WidgetPositionMode`, verified): 0 left/top, 1 centre, 2 right/bottom,
//! 3 left/top in 16384ths, 4 centre + 16384ths, 5 right/bottom in 16384ths.

use crate::interface::Component;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        Rect { x: x0, y: y0, w: (x1 - x0).max(0), h: (y1 - y0).max(0) }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect { x: self.x + dx, y: self.y + dy, w: self.w, h: self.h }
    }
}

/// Raw layout inputs of one component (possibly overridden by a script).
#[derive(Debug, Clone, Copy)]
pub struct RawLayout {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub width_mode: i8,
    pub height_mode: i8,
    pub x_mode: i8,
    pub y_mode: i8,
}

impl RawLayout {
    pub fn of(c: &Component) -> RawLayout {
        RawLayout {
            x: c.x as i32,
            y: c.y as i32,
            width: c.width as i32,
            height: c.height,
            width_mode: c.width_mode,
            height_mode: c.height_mode,
            x_mode: c.x_mode,
            y_mode: c.y_mode,
        }
    }
}

fn size(mode: i8, raw: i32, parent: i32) -> i32 {
    match mode {
        1 => parent - raw,
        2 => ((parent as i64 * raw as i64) >> 14) as i32,
        _ => raw,
    }
}

fn pos(mode: i8, raw: i32, parent: i32, own: i32) -> i32 {
    match mode {
        1 => (parent - own) / 2 + raw,
        2 => parent - own - raw,
        3 => ((parent as i64 * raw as i64) >> 14) as i32,
        4 => (parent - own) / 2 + ((parent as i64 * raw as i64) >> 14) as i32,
        5 => parent - own - ((parent as i64 * raw as i64) >> 14) as i32,
        _ => raw,
    }
}

pub fn resolve_raw(l: &RawLayout, is_if3: bool, parent_w: i32, parent_h: i32) -> Rect {
    if !is_if3 {
        return Rect { x: l.x, y: l.y, w: l.width, h: l.height };
    }
    let mut w = size(l.width_mode, l.width, parent_w);
    let mut h = size(l.height_mode, l.height, parent_h);
    if l.width_mode == 4 {
        w = h * l.width / l.height.max(1);
    }
    if l.height_mode == 4 {
        h = w * l.height / l.width.max(1);
    }
    let x = pos(l.x_mode, l.x, parent_w, w);
    let y = pos(l.y_mode, l.y, parent_h, h);
    Rect { x, y, w, h }
}

/// Position and size of `c` relative to its parent's origin, given the parent's size.
pub fn resolve(c: &Component, parent_w: i32, parent_h: i32) -> Rect {
    resolve_raw(&RawLayout::of(c), c.is_if3, parent_w, parent_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(x: i16, y: i16, w: u16, h: i32, m: [i8; 4]) -> Component {
        Component {
            is_if3: true,
            x,
            y,
            width: w,
            height: h,
            width_mode: m[0],
            height_mode: m[1],
            x_mode: m[2],
            y_mode: m[3],
            ..Default::default()
        }
    }

    #[test]
    fn absolute() {
        assert_eq!(resolve(&comp(5, 6, 30, 40, [0; 4]), 100, 100), Rect::new(5, 6, 30, 40));
    }

    #[test]
    fn minus_and_fraction() {
        let r = resolve(&comp(0, 0, 10, 20, [1, 1, 0, 0]), 100, 200);
        assert_eq!((r.w, r.h), (90, 180));
        let r = resolve(&comp(0, 0, 8192, 4096, [2, 2, 0, 0]), 100, 200);
        assert_eq!((r.w, r.h), (50, 50));
    }

    #[test]
    fn position_modes() {
        let c = |xm, ym, x, y| resolve(&comp(x, y, 20, 10, [0, 0, xm, ym]), 100, 50);
        assert_eq!(c(1, 1, 3, 2), Rect::new(43, 22, 20, 10));
        assert_eq!(c(2, 2, 3, 2), Rect::new(77, 38, 20, 10));
        assert_eq!(c(3, 3, 8192, 8192), Rect::new(50, 25, 20, 10));
        assert_eq!(c(4, 4, 4096, 4096), Rect::new(65, 32, 20, 10));
        assert_eq!(c(5, 5, 4096, 4096), Rect::new(55, 28, 20, 10));
    }

    #[test]
    fn aspect() {
        let r = resolve(&comp(0, 0, 2, 1, [4, 1, 0, 0]), 100, 60);
        assert_eq!((r.w, r.h), (118, 59));
    }

    #[test]
    fn if1_is_absolute() {
        let c = Component { x: 1, y: 2, width: 3, height: 4, width_mode: 1, ..Default::default() };
        assert_eq!(resolve(&c, 100, 100), Rect::new(1, 2, 3, 4));
    }

    #[test]
    fn rect_ops() {
        let a = Rect::new(0, 0, 10, 10);
        assert_eq!(a.intersect(&Rect::new(5, 5, 10, 10)), Rect::new(5, 5, 5, 5));
        assert!(a.intersect(&Rect::new(20, 0, 5, 5)).is_empty());
        assert!(a.contains(9, 9) && !a.contains(10, 9));
    }
}
