// The casino's slot machine: three reels of 32 stops with item symbols, one
// payline (the middle row). Each reel's stop is drawn when the player pulls
// the lever; the reels then scroll in the menu and stop one after another
// (at 1.6, 2.4 and 3.2 s), and the line is paid from the published paytable:
// three of a kind, or two berries anywhere on it. The reels hold each symbol
// a fixed number of times, so the odds are exact: 94.2% of what is staked is
// paid back on average (the test enumerates all 32,768 lines), and one pull
// in 4.5 pays. Closing the menu or leaving doesn't stop a pull: it ends and
// is paid all the same. Added to the game as `game::smp_casino_slots`; also
// a module of this crate, for its tests.

use super::smp_casino::{Color, FRAME_MS, Icon, Rng, Seat, Stake, filler, icon};

/// The symbols: item, name, stops on each reel, what three on the line pay
/// for each coin staked (the stake included).
pub const SYMBOLS: [(&str, &str, usize, i64); 6] = [
    ("minecraft:sweet_berries", "Berries", 9, 6),
    ("minecraft:melon_slice", "Melon", 8, 10),
    ("minecraft:golden_apple", "Golden Apple", 7, 15),
    ("minecraft:emerald", "Emerald", 5, 30),
    ("minecraft:diamond", "Diamond", 2, 100),
    ("minecraft:nether_star", "Nether Star", 1, 500),
];
/// Two berries on the line (not three) pay this for each coin staked.
pub const TWO_BERRIES: i64 = 2;
const BERRIES: u8 = 0;
pub const STOPS: usize = 32;

/// The reels, by stop: indices into `SYMBOLS`, no symbol twice in a row.
pub const REELS: [[u8; STOPS]; 3] = [
    [
        1, 0, 2, 3, 1, 4, 0, 1, 3, 1, 0, 5, 1, 2, 0, 1, 0, 2, 0, 2, 3, 2, 3, 2, 0, 1, 2, 1, 0, 4,
        3, 0,
    ],
    [
        1, 5, 0, 1, 0, 1, 2, 3, 1, 0, 2, 3, 0, 3, 2, 4, 2, 1, 0, 1, 2, 0, 3, 2, 4, 1, 0, 2, 3, 0,
        1, 0,
    ],
    [
        0, 1, 4, 3, 1, 0, 2, 0, 2, 3, 2, 0, 3, 0, 2, 1, 0, 4, 1, 2, 1, 2, 1, 3, 0, 1, 0, 2, 3, 0,
        1, 5,
    ],
];
/// The frame each reel stops at; the pull is paid at the last.
pub const STOP_AT: [i64; 3] = [8, 12, 16];
pub const FRAMES: i64 = 16;

pub const HELP: [&str; 2] = [
    "Slots (3 reels of 32 stops, one line): three Nether Stars pay 500x, Diamonds 100x, Emeralds 30x, Golden Apples 15x, Melons 10x, Berries 6x; two Berries 2x (the stake included).",
    "Each reel holds 9 Berries, 8 Melons, 7 Golden Apples, 5 Emeralds, 2 Diamonds, 1 Nether Star: 94.2% paid back on average (house edge 5.8%), one pull in 4.5 pays.",
];

/// What a line pays for each coin staked: 0 when it loses.
pub fn multiple(line: [u8; 3]) -> i64 {
    if line[0] == line[1] && line[1] == line[2] {
        return SYMBOLS[usize::from(line[0])].3;
    }
    if line.iter().filter(|s| **s == BERRIES).count() == 2 {
        return TWO_BERRIES;
    }
    0
}

pub fn line(stops: [usize; 3]) -> [u8; 3] {
    [0, 1, 2].map(|r| REELS[r][stops[r] % STOPS])
}

/// "Diamond · Diamond · Melon".
pub fn line_name(stops: [usize; 3]) -> String {
    line(stops).map(|s| SYMBOLS[usize::from(s)].1).join(" · ")
}

/// A player's machine: where its reels stand and whether the last pull won.
#[derive(Clone, Debug, Default)]
pub struct Machine {
    pub stops: [usize; 3],
    pub last: Option<bool>,
}

/// A pull: drawn when it starts, paid when the last reel stops.
#[derive(Clone, Debug)]
pub struct Pull {
    pub stake: Stake,
    pub stops: [usize; 3],
    pub started: i64,
}

impl Pull {
    fn frame(&self, now: i64) -> i64 {
        ((now - self.started) / FRAME_MS).clamp(0, FRAMES)
    }
    /// Reel `r`'s stop at frame `f`: scrolling, slower as it nears its stop.
    pub fn position(&self, r: usize, f: i64) -> usize {
        let left = (STOP_AT[r] - f).max(0);
        let behind = (left * (left + 1) / 2) as usize;
        (self.stops[r] + STOPS * 8 - behind % STOPS) % STOPS
    }
}

#[derive(Default)]
pub struct Slots {
    pub rounds: Vec<Pull>,
}

impl Slots {
    pub fn busy(&self) -> bool {
        !self.rounds.is_empty()
    }
    pub fn playing(&self, uuid: u128) -> bool {
        self.rounds.iter().any(|p| p.stake.uuid == uuid)
    }
    pub fn pull(&mut self, stake: Stake, now: i64, rng: &mut Rng) {
        let stops = [0; 3].map(|_| rng.below(STOPS as u64) as usize);
        self.rounds.push(Pull {
            stake,
            stops,
            started: now,
        });
    }
    /// The picture `uuid`'s pull is at, and whether a reel stops on it.
    pub fn frame(&self, uuid: u128, now: i64) -> Option<(i64, bool)> {
        let p = self.rounds.iter().find(|p| p.stake.uuid == uuid)?;
        let f = p.frame(now);
        Some((f, STOP_AT.contains(&f)))
    }
    /// The pulls whose last reel stopped: taken out, to be paid once.
    pub fn ended(&mut self, now: i64) -> Vec<Pull> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.rounds.len() {
            if now - self.rounds[i].started >= FRAMES * FRAME_MS {
                out.push(self.rounds.swap_remove(i));
            } else {
                i += 1;
            }
        }
        out
    }

    /// The machine's cells (all but the bottom row).
    pub fn layout(&self, cells: &mut [Option<Icon>], uuid: u128, seat: &Seat, now: i64) {
        let m = &seat.slots;
        let pull = self.rounds.iter().find(|p| p.stake.uuid == uuid);
        for c in &mut cells[..45] {
            *c = filler("minecraft:black_stained_glass_pane");
        }
        for c in &mut cells[..9] {
            *c = filler("minecraft:yellow_stained_glass_pane");
        }
        let top = match (pull, m.last) {
            (Some(_), _) => "The reels turn...".to_owned(),
            (None, Some(true)) => format!("{}: you won!", line_name(m.stops)),
            (None, Some(false)) => format!("{}: no luck", line_name(m.stops)),
            (None, None) => "Three of a kind on the middle row wins".to_owned(),
        };
        cells[4] = icon("minecraft:bell", top, Color::Yellow);
        let won = pull.is_none() && m.last == Some(true);
        let marker = if won {
            "minecraft:gold_block"
        } else {
            "minecraft:lime_stained_glass_pane"
        };
        cells[18] = icon(marker, "Payline →", Color::Green);
        cells[26] = icon(marker, "← Payline", Color::Green);
        for (r, col) in [2usize, 4, 6].into_iter().enumerate() {
            let at = match pull {
                Some(p) => p.position(r, p.frame(now)),
                None => m.stops[r],
            };
            for (row, d) in [(1usize, STOPS - 1), (2, 0), (3, 1)] {
                let s = SYMBOLS[usize::from(REELS[r][(at + d) % STOPS])];
                let name = if row == 2 {
                    s.1.to_owned()
                } else {
                    " ".to_owned()
                };
                cells[row * 9 + col] = icon(
                    s.0,
                    name,
                    if row == 2 {
                        Color::White
                    } else {
                        Color::DarkGray
                    },
                );
            }
        }
        // The paytable, on the fifth row.
        for (i, (item, name, _, pays)) in SYMBOLS.iter().enumerate().rev() {
            cells[36 + (5 - i)] = icon(item, format!("3 {name}: {pays}x"), Color::Gold);
        }
        cells[42] = icon(
            "minecraft:sweet_berries",
            format!("2 Berries: {TWO_BERRIES}x"),
            Color::Gold,
        );
        cells[43] = icon("minecraft:book", "94.2% paid back on average", Color::Aqua);
        cells[53] = if pull.is_some() {
            icon("minecraft:gray_concrete", "The reels turn...", Color::Gray)
        } else {
            icon("minecraft:lever", "PULL", Color::Green)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::super::smp_casino::{Fx, Game, View, tests::*};
    use super::*;

    #[test]
    fn each_reel_holds_the_published_stops() {
        for reel in REELS {
            for (i, s) in SYMBOLS.iter().enumerate() {
                assert_eq!(
                    reel.iter().filter(|x| usize::from(**x) == i).count(),
                    s.2,
                    "{}",
                    s.1
                );
            }
            for k in 0..STOPS {
                assert_ne!(reel[k], reel[(k + 1) % STOPS], "no symbol twice in a row");
            }
        }
        assert_eq!(SYMBOLS.iter().map(|s| s.2).sum::<usize>(), STOPS);
    }

    #[test]
    fn the_paytable_pays_back_94_2_percent() {
        let (mut back, mut hits) = (0i64, 0);
        for a in 0..STOPS {
            for b in 0..STOPS {
                for c in 0..STOPS {
                    let m = multiple(line([a, b, c]));
                    back += m;
                    hits += i64::from(m > 0);
                }
            }
        }
        let lines = (STOPS * STOPS * STOPS) as f64;
        let rtp = back as f64 / lines;
        assert!((0.92..=0.95).contains(&rtp), "{rtp}");
        assert_eq!(format!("{:.1}", rtp * 100.0), "94.2");
        assert_eq!(format!("{:.1}", lines / hits as f64), "4.5");
        assert_eq!(multiple([5, 5, 5]), 500);
        assert_eq!(multiple([0, 3, 0]), 2);
        assert_eq!(multiple([0, 0, 0]), 6);
        assert_eq!(multiple([0, 3, 4]), 0);
    }

    #[test]
    fn the_reels_scroll_and_stop_one_after_another_on_the_draw() {
        let p = Pull {
            stake: Stake {
                uuid: A,
                name: "Alex".into(),
                game: Game::Slots,
                amount: 10,
            },
            stops: [3, 17, 31],
            started: 0,
        };
        for (r, stop) in STOP_AT.into_iter().enumerate() {
            assert_ne!(p.position(r, stop - 1), p.stops[r], "still turning");
            for f in stop..=FRAMES {
                assert_eq!(p.position(r, f), p.stops[r]);
            }
        }
    }

    #[test]
    fn a_pull_is_paid_once_and_frames_stay_at_5_a_second() {
        let (mut e, mut c, dir) = setup("slots", 100_000);
        let before = money(&e);
        c.sit(A, "Alex", View::Slots);
        c.seats.get_mut(&A).unwrap().window = Some(5);
        c.click(&mut e, A, 53, false, 0);
        c.click(&mut e, A, 53, false, 30);
        assert_eq!(e.held(A), 10, "the double click pulled once");
        let mut shows = 0;
        for t in (0..FRAMES * FRAME_MS).step_by(50) {
            shows += c
                .tick(&mut e, t)
                .iter()
                .filter(|f| matches!(f, Fx::Show(A)))
                .count();
        }
        assert_eq!(shows as i64, FRAMES - 1, "one picture per frame");
        let stops = c.slots.rounds[0].stops;
        c.tick(&mut e, FRAMES * FRAME_MS);
        assert_eq!(e.held(A), 0);
        assert_eq!(e.balance(A), 10_000 - 10 + 10 * multiple(line(stops)));
        assert_eq!(money(&e), before);
        assert!(!c.animating());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
