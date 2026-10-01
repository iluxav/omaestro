//! Rectangles on the desktop, in Hyprland's logical pixels, and the named
//! placements `window:place()` understands.

use crate::backend::Monitor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Monitor {
    /// The monitor's area in logical pixels: physical size divided by the
    /// scale, sides swapped when it is rotated.
    pub fn logical(&self) -> Rect {
        let (width, height) = if self.transform % 2 == 1 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        Rect {
            x: self.x,
            y: self.y,
            w: (width as f64 / scale).round() as i64,
            h: (height as f64 / scale).round() as i64,
        }
    }

    /// The area windows may use: `logical` minus the reserved edges (bars).
    pub fn usable(&self) -> Rect {
        let [left, top, right, bottom] = self.reserved;
        let full = self.logical();
        Rect {
            x: full.x + left,
            y: full.y + top,
            w: (full.w - left - right).max(1),
            h: (full.h - top - bottom).max(1),
        }
    }
}

/// The names `place` accepts.
pub const PLACES: [&str; 15] = [
    "left",
    "right",
    "top",
    "bottom",
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
    "left-third",
    "middle-third",
    "right-third",
    "left-two-thirds",
    "right-two-thirds",
    "center",
    "max",
];

/// A fraction of `area`: `x`, `y`, `w`, `h` in `0..=1`.
pub fn fraction(area: Rect, x: f64, y: f64, w: f64, h: f64) -> Rect {
    let scale = |part: f64, whole: i64| (part * whole as f64).round() as i64;
    Rect {
        x: area.x + scale(x, area.w),
        y: area.y + scale(y, area.h),
        w: scale(w, area.w).max(1),
        h: scale(h, area.h).max(1),
    }
}

/// Where a window of size `current` goes for a named placement inside `area`.
pub fn place(name: &str, area: Rect, current: Rect) -> Result<Rect, String> {
    let third = 1.0 / 3.0;
    let rect = match name {
        "left" => fraction(area, 0.0, 0.0, 0.5, 1.0),
        "right" => fraction(area, 0.5, 0.0, 0.5, 1.0),
        "top" => fraction(area, 0.0, 0.0, 1.0, 0.5),
        "bottom" => fraction(area, 0.0, 0.5, 1.0, 0.5),
        "top-left" => fraction(area, 0.0, 0.0, 0.5, 0.5),
        "top-right" => fraction(area, 0.5, 0.0, 0.5, 0.5),
        "bottom-left" => fraction(area, 0.0, 0.5, 0.5, 0.5),
        "bottom-right" => fraction(area, 0.5, 0.5, 0.5, 0.5),
        "left-third" => fraction(area, 0.0, 0.0, third, 1.0),
        "middle-third" => fraction(area, third, 0.0, third, 1.0),
        "right-third" => fraction(area, 2.0 * third, 0.0, third, 1.0),
        "left-two-thirds" => fraction(area, 0.0, 0.0, 2.0 * third, 1.0),
        "right-two-thirds" => fraction(area, third, 0.0, 2.0 * third, 1.0),
        "max" => area,
        "center" => {
            let w = current.w.min(area.w);
            let h = current.h.min(area.h);
            Rect {
                x: area.x + (area.w - w) / 2,
                y: area.y + (area.h - h) / 2,
                w,
                h,
            }
        }
        other => {
            return Err(format!(
                "unknown placement '{other}'; one of {}",
                PLACES.join(", ")
            ));
        }
    };
    Ok(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(
        width: i64,
        height: i64,
        scale: f64,
        transform: i64,
        x: i64,
        reserved: [i64; 4],
    ) -> Monitor {
        Monitor {
            width,
            height,
            scale,
            transform,
            x,
            reserved,
            ..Monitor::default()
        }
    }

    #[test]
    fn logical_size_accounts_for_scale_rotation_and_bars() {
        let lg = monitor(5120, 2160, 1.25, 0, 0, [0, 26, 28, 0]);
        assert_eq!(
            lg.logical(),
            Rect {
                x: 0,
                y: 0,
                w: 4096,
                h: 1728
            }
        );
        assert_eq!(
            lg.usable(),
            Rect {
                x: 0,
                y: 26,
                w: 4068,
                h: 1702
            }
        );

        // A 4K screen rotated to portrait, left of the first one.
        let benq = monitor(3840, 2160, 1.25, 1, -1728, [0, 26, 28, 0]);
        assert_eq!(
            benq.logical(),
            Rect {
                x: -1728,
                y: 0,
                w: 1728,
                h: 3072
            }
        );
    }

    #[test]
    fn named_placements() {
        let area = Rect {
            x: 100,
            y: 30,
            w: 1000,
            h: 600,
        };
        let current = Rect {
            x: 0,
            y: 0,
            w: 400,
            h: 200,
        };
        let at = |name| place(name, area, current).unwrap();
        assert_eq!(
            at("left"),
            Rect {
                x: 100,
                y: 30,
                w: 500,
                h: 600
            }
        );
        assert_eq!(
            at("right"),
            Rect {
                x: 600,
                y: 30,
                w: 500,
                h: 600
            }
        );
        assert_eq!(
            at("top-right"),
            Rect {
                x: 600,
                y: 30,
                w: 500,
                h: 300
            }
        );
        assert_eq!(
            at("bottom-left"),
            Rect {
                x: 100,
                y: 330,
                w: 500,
                h: 300
            }
        );
        assert_eq!(
            at("middle-third"),
            Rect {
                x: 433,
                y: 30,
                w: 333,
                h: 600
            }
        );
        assert_eq!(
            at("right-two-thirds"),
            Rect {
                x: 433,
                y: 30,
                w: 667,
                h: 600
            }
        );
        assert_eq!(at("max"), area);
        assert_eq!(
            at("center"),
            Rect {
                x: 400,
                y: 230,
                w: 400,
                h: 200
            }
        );
        assert!(
            place("middle", area, current)
                .unwrap_err()
                .starts_with("unknown placement 'middle'")
        );
    }

    #[test]
    fn fractions() {
        let area = Rect {
            x: 0,
            y: 0,
            w: 1000,
            h: 1000,
        };
        assert_eq!(
            fraction(area, 0.25, 0.25, 0.5, 0.5),
            Rect {
                x: 250,
                y: 250,
                w: 500,
                h: 500
            }
        );
    }
}
