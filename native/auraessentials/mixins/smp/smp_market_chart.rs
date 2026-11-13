// A price chart drawn as a map's picture: 128 x 128 colours (`MapColor` id
// times 4 plus the shade), the curve over a dark background with a grid, the
// item's name, the span, the price now, the change and the low and high, in
// a 3 x 5 pixel font. Green when the price rose over the span, red when it
// fell. The glue sends it as map data (`native_send_map`) for a filled map
// it gives the player, and again each minute while they carry it. No game
// types. Added to the game as `game::smp_market_chart`; also a module of this
// crate, for its tests.

pub const SIZE: usize = 128;

/// `MapColor` ids, with the brightest shade (2).
const fn c(id: u8, shade: u8) -> u8 {
    id * 4 + shade
}
pub const BACKGROUND: u8 = c(29, 0); // COLOR_BLACK, dark
const GRID: u8 = c(21, 3); // COLOR_GRAY, darkest
const AXIS: u8 = c(22, 0); // COLOR_LIGHT_GRAY
const TEXT: u8 = c(8, 2); // SNOW
const DIM: u8 = c(22, 2); // COLOR_LIGHT_GRAY
pub const UP: u8 = c(19, 2); // COLOR_LIGHT_GREEN
pub const DOWN: u8 = c(28, 2); // COLOR_RED
const FILL_UP: u8 = c(27, 3); // COLOR_GREEN, darkest
const FILL_DOWN: u8 = c(28, 3);

/// 3 x 5 glyphs, rows top to bottom, 3 bits each (bit 2 = left).
fn glyph(ch: char) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 3, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '.' => [0, 0, 0, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        ':' => [0, 2, 0, 2, 0],
        '%' => [5, 1, 2, 4, 5],
        '+' => [0, 2, 7, 2, 0],
        '-' => [0, 0, 7, 0, 0],
        '/' => [1, 1, 2, 4, 4],
        '^' => [2, 7, 7, 0, 0],
        'v' => [0, 0, 7, 7, 2],
        _ => [0; 5],
    }
}

/// A picture being drawn.
pub struct Canvas(pub Vec<u8>);

impl Canvas {
    fn new() -> Self {
        Self(vec![BACKGROUND; SIZE * SIZE])
    }
    fn set(&mut self, x: i32, y: i32, color: u8) {
        if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) {
            self.0[y as usize * SIZE + x as usize] = color;
        }
    }
    /// Text from `x`, `y` (top left), 4 pixels a character.
    pub fn text(&mut self, x: i32, y: i32, s: &str, color: u8) {
        for (i, ch) in s.chars().enumerate() {
            // `▲` `▼` as the font's arrows.
            let ch = match ch {
                '▲' => '^',
                '▼' => 'v',
                c => c,
            };
            for (row, bits) in glyph(ch).iter().enumerate() {
                for col in 0..3 {
                    if bits & (4 >> col) != 0 {
                        self.set(x + i as i32 * 4 + col, y + row as i32, color);
                    }
                }
            }
        }
    }
    fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), color: u8) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.set(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
}

/// A short price: `412`, `41.2`, `1.2K`, `3.4M`.
pub fn short(v: f64) -> String {
    if v >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if v >= 1e4 {
        format!("{:.1}K", v / 1e3)
    } else if v >= 100.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// The chart of `prices` (oldest first) for `title` over `span` ("24H",
/// "7D"): the picture's colours.
pub fn render(title: &str, span: &str, prices: &[f64]) -> Vec<u8> {
    let mut c = Canvas::new();
    let title: String = title.chars().take(24).collect();
    c.text(2, 2, &title, TEXT);
    let (plot_x, plot_y, plot_w, plot_h) = (2i32, 20i32, 124i32, 92i32);
    if prices.len() < 2 {
        c.text(2, 60, "NO TRADES YET", DIM);
        c.text(2, 120, span, DIM);
        return c.0;
    }
    let first = prices[0];
    let last = *prices.last().expect("two");
    let (mut lo, mut hi) = prices
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), &p| (l.min(p), h.max(p)));
    if hi - lo < hi.abs() * 0.01 {
        // A flat line: a band of 1% around it.
        let m = (hi + lo) / 2.0;
        lo = m * 0.995;
        hi = m * 1.005;
    }
    let change = if first > 0.0 {
        (last / first - 1.0) * 100.0
    } else {
        0.0
    };
    let (line, fill) = if change >= 0.0 {
        (UP, FILL_UP)
    } else {
        (DOWN, FILL_DOWN)
    };
    let arrow = if change >= 0.0 { '▲' } else { '▼' };
    c.text(2, 10, &short(last), TEXT);
    let ch = format!("{arrow}{:.1}%", change.abs());
    c.text(126 - ch.chars().count() as i32 * 4, 10, &ch, line);
    // The grid: four rows, the span's quarters.
    for i in 0..=4 {
        let y = plot_y + plot_h * i / 4;
        for x in (plot_x..plot_x + plot_w).step_by(2) {
            c.set(x, y, GRID);
        }
        let x = plot_x + plot_w * i / 4;
        for y in (plot_y..plot_y + plot_h).step_by(2) {
            c.set(x, y, GRID);
        }
    }
    let to_y = |p: f64| plot_y + plot_h - ((p - lo) / (hi - lo) * f64::from(plot_h)).round() as i32;
    let n = prices.len();
    let to_x =
        |i: usize| plot_x + (i as f64 / (n - 1) as f64 * f64::from(plot_w - 1)).round() as i32;
    // The area under the curve, then the curve.
    let mut prev = (to_x(0), to_y(prices[0]));
    for (i, &p) in prices.iter().enumerate().skip(1) {
        let pt = (to_x(i), to_y(p));
        for x in prev.0..=pt.0 {
            let t = if pt.0 == prev.0 {
                1.0
            } else {
                f64::from(x - prev.0) / f64::from(pt.0 - prev.0)
            };
            let y = (f64::from(prev.1) + t * f64::from(pt.1 - prev.1)).round() as i32;
            for yy in (y + 2..=plot_y + plot_h).step_by(2) {
                c.set(x, yy, fill);
            }
        }
        prev = pt;
    }
    let mut prev = (to_x(0), to_y(prices[0]));
    for (i, &p) in prices.iter().enumerate().skip(1) {
        let pt = (to_x(i), to_y(p));
        c.line(prev, pt, line);
        c.line((prev.0, prev.1 - 1), (pt.0, pt.1 - 1), line);
        prev = pt;
    }
    c.line(
        (plot_x, plot_y + plot_h),
        (plot_x + plot_w - 1, plot_y + plot_h),
        AXIS,
    );
    c.text(2, 116, &format!("L {}", short(lo)), DIM);
    let high = format!("H {}", short(hi));
    c.text(64, 116, &high, DIM);
    c.text(126 - span.chars().count() as i32 * 4, 122, span, DIM);
    c.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rising_chart_is_green_and_a_falling_one_red() {
        let up: Vec<f64> = (0..100).map(|i| 100.0 + f64::from(i)).collect();
        let pic = render("Diamond", "24H", &up);
        assert_eq!(pic.len(), SIZE * SIZE);
        assert!(pic.contains(&UP) && !pic.contains(&DOWN));
        let down: Vec<f64> = up.iter().rev().copied().collect();
        let pic = render("Diamond", "24H", &down);
        assert!(pic.contains(&DOWN) && !pic.contains(&UP));
        // The curve's ends: low on the left of a rising one.
        let pic = render("X", "24H", &up);
        let col = |x: usize| (20..113).find(|&y| pic[y * SIZE + x] == UP);
        assert!(col(3).unwrap() > col(124).unwrap());
    }

    #[test]
    fn no_history_and_flat_prices_draw_without_dividing_by_zero() {
        let pic = render("Wheat", "7D", &[]);
        assert!(pic.contains(&TEXT) || pic.contains(&DIM));
        let pic = render("Wheat", "7D", &[2.0; 50]);
        assert!(pic.contains(&UP));
        assert_eq!(short(412.4), "412");
        assert_eq!(short(41.24), "41.2");
        assert_eq!(short(12_400.0), "12.4K");
    }
}
