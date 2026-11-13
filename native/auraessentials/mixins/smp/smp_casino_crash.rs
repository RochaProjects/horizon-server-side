// The casino's crash: one round for everyone. The first bet opens a 10 s
// betting window. Then the multiplier climbs from 1.00x (2x at 8 s, 10x at
// 27 s) until it crashes; whoever cashes out first gets their bet times the
// multiplier at that moment, and the others lose it. The crash point is
// drawn when the flight starts, P(crash >= x) = (100% - edge) / x, so with
// the 4% edge any way of playing gets back 96% on average. A crash at
// 1.00x happens about 5% of the time, and the flight never goes past the cap
// (100x). A player may set an automatic cash-out before the flight. Leaving
// the server mid-flight cashes out at that moment, and closing the menu
// doesn't (/casino crash opens it again). Every menu, the boss bar and the
// action bar show the climb, 5 pictures a second. Added to the game as
// `game::smp_casino_crash`; also a module of this crate, for its tests.

use super::smp_casino::{Color, FRAME_MS, Icon, Rng, Seat, Stake, filler, icon};
use std::collections::VecDeque;

pub const BETTING_MS: i64 = 10_000;
/// How long the crash stays shown before the next round may open.
pub const SHOWN_MS: i64 = 3000;
/// The multiplier doubles every 8 s.
const DOUBLING_MS: f64 = 8000.0;
/// The automatic cash-outs a player can pick, in hundredths (0: none).
pub const AUTOS: [u32; 7] = [0, 150, 200, 300, 500, 1000, 2500];

pub const HELP: [&str; 1] = [
    "Crash: bet before the flight, then cash out before it crashes; you get your bet times the multiplier. P(crash at x or more) = 96% / x, capped at 100x: whatever you do, 96% is paid back on average (house edge 4%).",
];

/// The multiplier, in hundredths, `ms` into a flight.
pub fn multiple_at(ms: i64) -> u32 {
    let m = 100.0 * (ms.max(0) as f64 * std::f64::consts::LN_2 / DOUBLING_MS).exp();
    m.floor().min(f64::from(u32::MAX)) as u32
}

/// The first millisecond the multiplier reaches `h` hundredths.
pub fn time_to(h: u32) -> i64 {
    let mut t =
        ((f64::from(h.max(100)) / 100.0).ln() / std::f64::consts::LN_2 * DOUBLING_MS).ceil() as i64;
    while t > 0 && multiple_at(t - 1) >= h {
        t -= 1;
    }
    while multiple_at(t) < h {
        t += 1;
    }
    t
}

/// A crash point, in hundredths: P(>= x) = (1 - edge) / x, at least 1.00x,
/// at most `max` (in whole times the bet).
pub fn draw(rng: &mut Rng, edge_percent: i64, max: i64) -> u32 {
    let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
    let keep = 1.0 - edge_percent.clamp(0, 99) as f64 / 100.0;
    let x = (100.0 * keep / (1.0 - u)).floor();
    x.clamp(100.0, (max.max(1) * 100) as f64) as u32
}

/// "2.31x".
pub fn times(h: u32) -> String {
    format!("{}.{:02}x", h / 100, h % 100)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Betting {
        ends: i64,
    },
    Flying {
        started: i64,
        crash: u32,
        crash_at: i64,
    },
    Crashed {
        at: i64,
        crash: u32,
    },
}

#[derive(Clone, Debug)]
pub struct Bet {
    pub stake: Stake,
    /// Cash out by itself at this multiplier (hundredths); 0: by hand.
    pub auto: u32,
}

/// What moved in a step of the round.
#[derive(Default, Debug)]
pub struct Step {
    pub took_off: bool,
    /// Bets cashed out by their automatic cash-out: paid at that multiplier.
    pub cashed: Vec<(Bet, u32)>,
    /// The crash, and the bets still in: lost.
    pub crashed: Option<(u32, Vec<Bet>)>,
}

pub struct Crash {
    pub phase: Phase,
    pub bets: Vec<Bet>,
    /// The last rounds' crash points, newest first.
    pub history: VecDeque<u32>,
}

impl Default for Crash {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            bets: Vec::new(),
            history: VecDeque::new(),
        }
    }
}

impl Crash {
    pub fn busy(&self) -> bool {
        self.phase != Phase::Idle
    }
    pub fn playing(&self, uuid: u128) -> bool {
        self.bets.iter().any(|b| b.stake.uuid == uuid)
    }
    /// Whether a bet may be placed now (before a flight).
    pub fn open(&self) -> bool {
        matches!(self.phase, Phase::Idle | Phase::Betting { .. })
    }
    /// A held bet joins the next flight (`open` was checked).
    pub fn join(&mut self, stake: Stake, auto: u32, now: i64) {
        if self.phase == Phase::Idle {
            self.phase = Phase::Betting {
                ends: now + BETTING_MS,
            };
        }
        self.bets.push(Bet { stake, auto });
    }
    /// `uuid` cashes out: their bet and the multiplier, when the flight is
    /// still climbing. Once only: the bet leaves the round.
    pub fn cash_out(&mut self, uuid: u128, now: i64) -> Option<(Bet, u32)> {
        let Phase::Flying {
            started, crash_at, ..
        } = self.phase
        else {
            return None;
        };
        if now >= crash_at {
            return None;
        }
        let i = self.bets.iter().position(|b| b.stake.uuid == uuid)?;
        Some((self.bets.swap_remove(i), multiple_at(now - started)))
    }
    /// A bet taken back before the flight (its player left): None once flying.
    pub fn withdraw(&mut self, uuid: u128) -> Option<Bet> {
        if !matches!(self.phase, Phase::Betting { .. }) {
            return None;
        }
        let i = self.bets.iter().position(|b| b.stake.uuid == uuid)?;
        let bet = self.bets.swap_remove(i);
        if self.bets.is_empty() {
            self.phase = Phase::Idle;
        }
        Some(bet)
    }
    /// The round moves on to `now`.
    pub fn step(&mut self, now: i64, rng: &mut Rng, edge_percent: i64, max: i64) -> Step {
        let mut step = Step::default();
        if let Phase::Betting { ends } = self.phase
            && now >= ends
        {
            let crash = draw(rng, edge_percent, max);
            self.phase = Phase::Flying {
                started: ends,
                crash,
                crash_at: ends + time_to(crash),
            };
            step.took_off = true;
        }
        if let Phase::Flying {
            started,
            crash,
            crash_at,
        } = self.phase
        {
            // Automatic cash-outs below the crash, reached by now.
            let mut i = 0;
            while i < self.bets.len() {
                let b = &self.bets[i];
                if b.auto > 0 && b.auto < crash && now >= started + time_to(b.auto) {
                    let bet = self.bets.swap_remove(i);
                    let at = bet.auto;
                    step.cashed.push((bet, at));
                } else {
                    i += 1;
                }
            }
            if now >= crash_at {
                step.crashed = Some((crash, std::mem::take(&mut self.bets)));
                self.phase = Phase::Crashed {
                    at: crash_at,
                    crash,
                };
                self.history.push_front(crash);
                self.history.truncate(8);
            }
        }
        if let Phase::Crashed { at, .. } = self.phase
            && now >= at + SHOWN_MS
        {
            self.phase = Phase::Idle;
        }
        step
    }
    /// The picture the round is at: it changes 5 times a second in flight,
    /// once a second before.
    pub fn frame(&self, now: i64) -> i64 {
        match self.phase {
            Phase::Idle => 0,
            Phase::Betting { ends } => 1_000_000 + (ends - now).max(0) / 1000,
            Phase::Flying { started, .. } => 2_000_000 + (now - started).max(0) / FRAME_MS,
            Phase::Crashed { .. } => 3_000_000,
        }
    }
    /// The multiplier now, in flight.
    pub fn now_multiple(&self, now: i64) -> Option<u32> {
        match self.phase {
            Phase::Flying { started, crash, .. } => Some(multiple_at(now - started).min(crash)),
            _ => None,
        }
    }

    /// The boss bar's line, colour and fill (0 to 100).
    pub fn boss(&self, now: i64, max: i64) -> Option<(String, Color, u32)> {
        let fill = |h: u32| {
            let f = (f64::from(h) / 100.0).ln() / (max.max(2) as f64).ln();
            (f * 100.0).clamp(0.0, 100.0) as u32
        };
        Some(match self.phase {
            Phase::Idle => return None,
            Phase::Betting { ends } => (
                format!(
                    "Crash: takes off in {} s · {} bet(s) · /casino crash",
                    ((ends - now).max(0) + 999) / 1000,
                    self.bets.len()
                ),
                Color::Yellow,
                0,
            ),
            Phase::Flying { .. } => {
                let h = self.now_multiple(now).unwrap_or(100);
                (format!("Crash: {}", times(h)), Color::Green, fill(h))
            }
            Phase::Crashed { crash, .. } => (
                format!("Crash: crashed at {}", times(crash)),
                Color::Red,
                fill(crash),
            ),
        })
    }

    /// The crash menu's cells (all but the bottom row's shared cells).
    pub fn layout(&self, cells: &mut [Option<Icon>], uuid: u128, seat: &Seat, now: i64, max: i64) {
        // The chart: 9 columns of time, 4 rows of (log) multiplier.
        let (elapsed, top, crashed) = match self.phase {
            Phase::Flying { started, crash, .. } => {
                (now - started, multiple_at(now - started).min(crash), false)
            }
            Phase::Crashed { crash, .. } => (time_to(crash), crash, true),
            _ => (0, 100, false),
        };
        let level = |h: u32| {
            let f = (f64::from(h) / 100.0).ln() / (max.max(2) as f64).ln().min(5f64.ln() * 2.0);
            (f * 4.0).clamp(0.0, 4.0)
        };
        let fill = if crashed {
            "minecraft:red_stained_glass_pane"
        } else {
            "minecraft:lime_stained_glass_pane"
        };
        for col in 0..9usize {
            let h = multiple_at(elapsed * (col as i64 + 1) / 9);
            let l = level(h);
            for row in 0..4usize {
                let from_bottom = 3 - row;
                cells[row * 9 + col] = if elapsed > 0 && l > from_bottom as f64 {
                    filler(fill)
                } else {
                    filler("minecraft:black_stained_glass_pane")
                };
            }
        }
        if elapsed > 0 {
            let row = 3 - (level(top).ceil() as usize).clamp(1, 4).saturating_sub(1);
            let (item, color) = if crashed {
                ("minecraft:tnt", Color::Red)
            } else {
                ("minecraft:firework_rocket", Color::Green)
            };
            let name = if crashed {
                format!("Crashed at {}", times(top))
            } else {
                times(top)
            };
            cells[row * 9 + 8] = icon(item, name, color);
        }
        for c in &mut cells[36..45] {
            *c = filler("minecraft:gray_stained_glass_pane");
        }
        let status = match self.phase {
            Phase::Idle => "No round: the first bet opens one (10 s to join)".to_owned(),
            Phase::Betting { ends } => {
                format!("Takes off in {} s", ((ends - now).max(0) + 999) / 1000)
            }
            Phase::Flying { .. } => format!("Flying: {}", times(top)),
            Phase::Crashed { crash, .. } => format!("Crashed at {}", times(crash)),
        };
        cells[38] = icon("minecraft:clock", status, Color::Yellow);
        cells[39] = icon(
            "minecraft:paper",
            format!("{} bet(s) in this round", self.bets.len()),
            Color::White,
        );
        let past: Vec<String> = self.history.iter().take(6).map(|h| times(*h)).collect();
        cells[40] = icon(
            "minecraft:map",
            if past.is_empty() {
                "Last crashes: none yet".to_owned()
            } else {
                format!("Last crashes: {}", past.join(", "))
            },
            Color::Gray,
        );
        cells[42] = icon(
            "minecraft:repeater",
            match seat.crash_auto {
                0 => "Auto cash-out: off (click to set)".to_owned(),
                a => format!("Auto cash-out at {} (click to change)", times(a)),
            },
            Color::Aqua,
        );
        let mine = self.bets.iter().find(|b| b.stake.uuid == uuid);
        cells[53] = match (self.phase, mine) {
            (Phase::Flying { .. }, Some(b)) => icon(
                "minecraft:gold_block",
                format!(
                    "CASH OUT at {}: {}",
                    times(top),
                    b.stake.amount * i64::from(top) / 100
                ),
                Color::Gold,
            ),
            (Phase::Idle | Phase::Betting { .. }, Some(b)) => icon(
                "minecraft:lime_stained_glass",
                format!("Your bet is in: {}", b.stake.amount),
                Color::Green,
            ),
            (Phase::Idle | Phase::Betting { .. }, None) => {
                icon("minecraft:lime_concrete", "BET", Color::Green)
            }
            _ => icon(
                "minecraft:gray_concrete",
                "Wait for the next round",
                Color::Gray,
            ),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::super::smp_casino::{Fx, Game, POT, View, tests::*};
    use super::*;

    fn stake(uuid: u128, amount: i64) -> Stake {
        Stake {
            uuid,
            name: "x".into(),
            game: Game::Crash,
            amount,
        }
    }

    #[test]
    fn the_curve_and_its_inverse() {
        assert_eq!(multiple_at(0), 100);
        assert_eq!(multiple_at(8000), 200);
        assert_eq!(multiple_at(16_000), 400);
        for h in [100, 101, 150, 199, 200, 231, 1000, 10_000] {
            let t = time_to(h);
            assert!(
                multiple_at(t) >= h && (t == 0 || multiple_at(t - 1) < h),
                "{h}"
            );
        }
        assert_eq!(times(231), "2.31x");
    }

    #[test]
    fn crash_points_follow_96_over_x() {
        let mut rng = Rng::from_seed([3; 32]);
        let n = 200_000;
        let points: Vec<u32> = (0..n).map(|_| draw(&mut rng, 4, 100)).collect();
        let share = |x: u32| points.iter().filter(|p| **p >= x).count() as f64 / n as f64;
        for x in [100, 150, 200, 500, 1000] {
            let expected = if x == 100 {
                1.0
            } else {
                0.96 * 100.0 / f64::from(x)
            };
            assert!(
                (share(x) - expected).abs() < 0.01,
                "{x}: {} vs {expected}",
                share(x)
            );
        }
        let instant = points.iter().filter(|p| **p == 100).count() as f64 / n as f64;
        // Below 1.01x, shown as 1.00x: 1 - 96/101.
        assert!((instant - 0.0495).abs() < 0.005, "{instant}");
        assert!(points.iter().all(|p| (100..=10_000).contains(p)));
    }

    #[test]
    fn a_round_cashes_out_by_hand_and_by_itself_then_crashes() {
        let mut rng = Rng::from_seed([3; 32]);
        let mut c = Crash::default();
        c.join(stake(A, 100), 0, 0);
        c.join(stake(B, 100), 150, 1000);
        assert_eq!(c.phase, Phase::Betting { ends: BETTING_MS });
        assert!(c.cash_out(A, 5000).is_none(), "not before the flight");
        // A crash at 3.00x.

        c.phase = Phase::Flying {
            started: BETTING_MS,
            crash: 300,
            crash_at: BETTING_MS + time_to(300),
        };
        let s = c.step(BETTING_MS + time_to(150), &mut rng, 4, 100);
        assert_eq!(s.cashed.len(), 1);
        assert_eq!((s.cashed[0].0.stake.uuid, s.cashed[0].1), (B, 150));
        let (bet, m) = c.cash_out(A, BETTING_MS + 8000).unwrap();
        assert_eq!((bet.stake.uuid, m), (A, 200));
        assert!(c.cash_out(A, BETTING_MS + 8100).is_none(), "once");
        c.join(stake(B, 50), 0, 0);
        let s = c.step(BETTING_MS + time_to(300), &mut rng, 4, 100);
        let (crash, lost) = s.crashed.unwrap();
        assert_eq!((crash, lost.len()), (300, 1));
        assert!(
            c.cash_out(B, BETTING_MS + time_to(300)).is_none(),
            "too late"
        );
        c.step(BETTING_MS + time_to(300) + SHOWN_MS, &mut rng, 4, 100);
        assert_eq!(c.phase, Phase::Idle);
    }

    #[test]
    fn through_the_casino_every_bet_is_paid_once_and_leaving_cashes_out() {
        let (mut e, mut c, dir) = setup("crash", 100_000);
        let before = money(&e);
        for (u, n) in [(A, "Alex"), (B, "Steve")] {
            c.sit(u, n, View::Crash);
            c.seats.get_mut(&u).unwrap().window = Some(7);
            c.click(&mut e, u, 53, false, 0);
            c.click(&mut e, u, 53, false, 20);
        }
        assert_eq!((e.held(A), e.held(B)), (10, 10), "double clicks bet once");
        c.tick(&mut e, BETTING_MS);
        let Phase::Flying {
            crash, crash_at, ..
        } = c.crash.phase
        else {
            panic!("{:?}", c.crash.phase)
        };
        // Steve leaves the server right away; Alex keeps flying.
        c.gone(&mut e, &[A], BETTING_MS);
        assert_eq!(e.held(B), 0);
        assert_eq!(
            e.balance(B),
            10_000 - 10 + 10 * 100 / 100,
            "cashed out at 1.00x"
        );
        // Closing the menu doesn't cash out.
        assert!(c.closed(A, 7, 0));
        let mut t = BETTING_MS;
        while t <= crash_at + SHOWN_MS {
            c.tick(&mut e, t);
            t += 50;
        }
        c.tick(&mut e, crash_at + SHOWN_MS);
        assert_eq!(e.held(A), 0);
        assert_eq!(
            e.balance(A),
            9990,
            "lost: no cash-out before {}",
            times(crash)
        );
        assert_eq!(money(&e), before);
        assert!(!c.animating());
        let _ = e.pool(POT);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_flight_is_drawn_at_take_off_and_shown_5_times_a_second() {
        let (mut e, mut c, dir) = setup("crash-frames", 100_000);
        c.sit(A, "Alex", View::Crash);
        c.seats.get_mut(&A).unwrap().window = Some(7);
        c.seats.get_mut(&A).unwrap().crash_auto = 0;
        c.click(&mut e, A, 53, false, 0);
        c.tick(&mut e, BETTING_MS);
        c.crash.phase = Phase::Flying {
            started: BETTING_MS,
            crash: 10_000,
            crash_at: BETTING_MS + time_to(10_000),
        };
        let mut shows = 0;
        for t in (BETTING_MS + 1..BETTING_MS + 2002).step_by(50) {
            shows += c
                .tick(&mut e, t)
                .iter()
                .filter(|f| matches!(f, Fx::Show(A)))
                .count();
        }
        assert_eq!(shows, 10, "2 s of flight, 10 pictures");
        // A cash-out click at 1.00x + 2 s.
        let t = BETTING_MS + 2000;
        c.click(&mut e, A, 53, false, t);
        let m = i64::from(multiple_at(2000));
        assert_eq!(e.balance(A), 10_000 - 10 + 10 * m / 100);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn playing_any_way_pays_back_about_96_percent() {
        let mut rng = Rng::from_seed([3; 32]);
        let n = 100_000;
        for target in [110u32, 200, 500, 2000] {
            let mut back = 0i64;
            for _ in 0..n {
                let crash = draw(&mut rng, 4, 100);
                if target < crash {
                    back += i64::from(target);
                }
            }
            let rtp = back as f64 / (n as f64 * 100.0);
            assert!((0.90..1.02).contains(&rtp), "{target}: {rtp}");
        }
    }
}
