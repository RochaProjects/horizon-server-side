// The casino's roulette: European, one zero, 37 pockets in the wheel's real
// order. One bet a spin (a number, red or black, odd or even, 1-18 or 19-36,
// a dozen, a column), paid at the true odds, so the house's edge is the zero:
// 1/37 = 2.70% on every bet. The pocket is drawn the moment the player
// spins; the menu's top rows then show the wheel turning under a pointer,
// fast first and slowing down to land on it (20 pictures over 4 s). Closing
// the menu or leaving doesn't stop it: the round ends and is paid all the
// same. Added to the game as `game::smp_casino_roulette`; also a module of
// this crate, for its tests.

use super::smp_casino::{Color, FRAME_MS, Icon, Rng, Seat, Stake, View, filler, icon};

/// The pockets in the order they sit on the wheel.
pub const WHEEL: [u8; 37] = [
    0, 32, 15, 19, 4, 21, 2, 25, 17, 34, 6, 27, 13, 36, 11, 30, 8, 23, 10, 5, 24, 16, 33, 1, 20,
    14, 31, 9, 22, 18, 29, 7, 28, 12, 35, 3, 26,
];
const RED: [u8; 18] = [
    1, 3, 5, 7, 9, 12, 14, 16, 18, 19, 21, 23, 25, 27, 30, 32, 34, 36,
];
/// Pictures of a spin, `FRAME_MS` apart.
pub const FRAMES: i64 = 20;

pub const HELP: [&str; 2] = [
    "Roulette (European, one zero, 37 pockets): a number pays 35 to 1; a dozen or a column 2 to 1; red, black, odd, even, 1-18, 19-36 1 to 1.",
    "Zero loses every bet but a bet on 0. House edge 2.70% on every bet (97.3% paid back on average).",
];

pub fn is_red(n: u8) -> bool {
    RED.contains(&n)
}

/// "17 Black".
pub fn pocket_name(n: u8) -> String {
    let colour = match n {
        0 => "Green",
        n if is_red(n) => "Red",
        _ => "Black",
    };
    format!("{n} {colour}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bet {
    Number(u8),
    Red,
    Black,
    Odd,
    Even,
    Low,
    High,
    /// 1: 1-12, 2: 13-24, 3: 25-36.
    Dozen(u8),
    /// 1: 1, 4, 7... 34; 2: 2, 5... 35; 3: 3, 6... 36.
    Column(u8),
}

impl Bet {
    pub fn wins(self, n: u8) -> bool {
        if n == 0 {
            return self == Bet::Number(0);
        }
        match self {
            Bet::Number(k) => k == n,
            Bet::Red => is_red(n),
            Bet::Black => !is_red(n),
            Bet::Odd => !n.is_multiple_of(2),
            Bet::Even => n.is_multiple_of(2),
            Bet::Low => n <= 18,
            Bet::High => n >= 19,
            Bet::Dozen(d) => (n - 1) / 12 + 1 == d,
            Bet::Column(c) => (n - 1) % 3 + 1 == c,
        }
    }
    /// What a winning bet pays back for each coin staked, the stake included.
    pub fn multiple(self) -> i64 {
        match self {
            Bet::Number(_) => 36,
            Bet::Dozen(_) | Bet::Column(_) => 3,
            _ => 2,
        }
    }
    /// What the bet pays back on pocket `n`: 0 when it loses.
    pub fn payout(self, n: u8, stake: i64) -> i64 {
        if self.wins(n) {
            stake * self.multiple()
        } else {
            0
        }
    }
    pub fn label(self) -> String {
        match self {
            Bet::Number(n) => format!("{n}"),
            Bet::Red => "Red".into(),
            Bet::Black => "Black".into(),
            Bet::Odd => "Odd".into(),
            Bet::Even => "Even".into(),
            Bet::Low => "1 to 18".into(),
            Bet::High => "19 to 36".into(),
            Bet::Dozen(d) => format!(
                "{} 12",
                ["1st", "2nd", "3rd"][usize::from(d.clamp(1, 3) - 1)]
            ),
            Bet::Column(c) => format!("Column {c}"),
        }
    }
    fn odds(self) -> &'static str {
        match self.multiple() {
            36 => "35 to 1",
            3 => "2 to 1",
            _ => "1 to 1",
        }
    }
}

/// A player's table: the bet chosen, where the wheel stopped, the last
/// pocket and whether it won.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub bet: Option<Bet>,
    /// The wheel's index under the pointer.
    pub at: usize,
    pub last: Option<(u8, bool)>,
}

/// A spin: drawn when it starts, paid when its last picture is shown.
#[derive(Clone, Debug)]
pub struct Spin {
    pub stake: Stake,
    pub bet: Bet,
    /// The wheel's index under the pointer at the start, and where it stops.
    pub from: usize,
    pub stop: usize,
    /// Pockets the wheel passes: two turns and the way to `stop`.
    pub total: i64,
    pub started: i64,
}

impl Spin {
    /// The wheel's index under the pointer at frame `f` (0..=FRAMES): an
    /// ease-out, fast first, the last pockets one by one.
    pub fn position(&self, f: i64) -> usize {
        let f = f.clamp(0, FRAMES);
        let done = self.total * (FRAMES * FRAMES - (FRAMES - f) * (FRAMES - f)) / (FRAMES * FRAMES);
        (self.from + done as usize) % 37
    }
    fn frame(&self, now: i64) -> i64 {
        ((now - self.started) / FRAME_MS).clamp(0, FRAMES)
    }
    #[cfg(test)]
    pub fn test(stake: Stake) -> Self {
        Self {
            stake,
            bet: Bet::Red,
            from: 0,
            stop: 0,
            total: 74,
            started: 0,
        }
    }
}

/// The spins under way.
#[derive(Default)]
pub struct Roulette {
    pub rounds: Vec<Spin>,
}

impl Roulette {
    pub fn busy(&self) -> bool {
        !self.rounds.is_empty()
    }
    pub fn playing(&self, uuid: u128) -> bool {
        self.rounds.iter().any(|s| s.stake.uuid == uuid)
    }
    /// The bet is held: the pocket is drawn now, the wheel starts at `from`.
    pub fn spin(&mut self, stake: Stake, bet: Bet, from: usize, now: i64, rng: &mut Rng) {
        let stop = rng.below(37) as usize;
        let from = from % 37;
        self.rounds.push(Spin {
            stake,
            bet,
            from,
            stop,
            total: 74 + ((stop + 37 - from) % 37) as i64,
            started: now,
        });
    }
    /// The picture `uuid`'s spin is at.
    pub fn frame(&self, uuid: u128, now: i64) -> Option<i64> {
        self.rounds
            .iter()
            .find(|s| s.stake.uuid == uuid)
            .map(|s| s.frame(now))
    }
    /// The spins whose wheel has stopped, each with its pocket: taken out,
    /// to be paid once.
    pub fn ended(&mut self, now: i64) -> Vec<(Spin, u8)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.rounds.len() {
            if now - self.rounds[i].started >= FRAMES * FRAME_MS {
                let s = self.rounds.swap_remove(i);
                let n = WHEEL[s.stop];
                out.push((s, n));
            } else {
                i += 1;
            }
        }
        out
    }

    /// The table's cells (all but the bottom row).
    pub fn layout(&self, cells: &mut [Option<Icon>], uuid: u128, seat: &Seat, now: i64) {
        let t = &seat.roulette;
        if seat.view == View::RouletteNumbers {
            for n in 1..=36u8 {
                cells[usize::from(n) - 1] = pocket(n, Some(Bet::Number(n)) == t.bet);
            }
            for c in &mut cells[36..45] {
                *c = filler("minecraft:gray_stained_glass_pane");
            }
            cells[36] = icon("minecraft:arrow", "Back to the table", Color::White);
            cells[40] = pocket(0, t.bet == Some(Bet::Number(0)));
            return;
        }
        let spin = self.rounds.iter().find(|s| s.stake.uuid == uuid);
        let at = spin.map_or(t.at, |s| s.position(s.frame(now)));
        for c in &mut cells[..9] {
            *c = filler("minecraft:black_stained_glass_pane");
        }
        let pointer = match (spin, t.last) {
            (Some(_), _) => "The wheel turns...".to_owned(),
            (None, Some((n, true))) => format!("{}: you won!", pocket_name(n)),
            (None, Some((n, false))) => format!("{}: no luck", pocket_name(n)),
            (None, None) => "The ball lands under this arrow".to_owned(),
        };
        cells[4] = icon("minecraft:hopper", pointer, Color::Yellow);
        for k in 0..9 {
            let n = WHEEL[(at + 37 + k - 4) % 37];
            cells[9 + k] = pocket(n, false);
        }
        let bets: [(usize, Bet, &'static str); 15] = [
            (18, Bet::Red, "minecraft:red_wool"),
            (19, Bet::Black, "minecraft:black_wool"),
            (20, Bet::Odd, "minecraft:white_wool"),
            (21, Bet::Even, "minecraft:gray_wool"),
            (22, Bet::Low, "minecraft:cyan_terracotta"),
            (23, Bet::High, "minecraft:purple_terracotta"),
            (24, Bet::Dozen(1), "minecraft:light_blue_concrete"),
            (25, Bet::Dozen(2), "minecraft:light_blue_concrete"),
            (26, Bet::Dozen(3), "minecraft:light_blue_concrete"),
            (27, Bet::Column(1), "minecraft:yellow_concrete"),
            (28, Bet::Column(2), "minecraft:yellow_concrete"),
            (29, Bet::Column(3), "minecraft:yellow_concrete"),
            (33, Bet::Number(0), "minecraft:lime_concrete"),
            (30, Bet::Low, ""),
            (31, Bet::Low, ""),
        ];
        for (slot, bet, item) in bets {
            if item.is_empty() {
                continue;
            }
            let chosen = t.bet == Some(bet);
            let name = format!(
                "{}{} (pays {})",
                if chosen { "» " } else { "" },
                bet.label(),
                bet.odds()
            );
            let mut i = icon(
                item,
                name,
                if chosen { Color::Yellow } else { Color::White },
            );
            if let (Some(i), Bet::Dozen(k) | Bet::Column(k)) = (i.as_mut(), bet) {
                i.count = k;
            }
            cells[slot] = i;
        }
        for slot in [30, 32, 34, 35] {
            cells[slot] = filler("minecraft:gray_stained_glass_pane");
        }
        let number = match t.bet {
            Some(Bet::Number(n)) if n > 0 => format!("» Number {n} (pays 35 to 1): pick another"),
            _ => "A single number (pays 35 to 1)...".into(),
        };
        cells[31] = icon("minecraft:target", number, Color::White);
        let shade = match (spin.is_some(), t.last) {
            (false, Some((_, true))) => "minecraft:lime_stained_glass_pane",
            (false, Some((_, false))) => "minecraft:red_stained_glass_pane",
            _ => "minecraft:gray_stained_glass_pane",
        };
        for c in &mut cells[36..45] {
            *c = filler(shade);
        }
        cells[40] = icon(
            "minecraft:paper",
            match t.bet {
                Some(b) => format!("Your bet: {} (pays {})", b.label(), b.odds()),
                None => "Pick a bet above, then spin".into(),
            },
            Color::White,
        );
        cells[53] = if spin.is_some() {
            icon("minecraft:gray_concrete", "The wheel turns...", Color::Gray)
        } else {
            icon("minecraft:lime_concrete", "SPIN", Color::Green)
        };
    }
}

/// A pocket as a cell: its colour, its number as the count.
fn pocket(n: u8, chosen: bool) -> Option<Icon> {
    let item = match n {
        0 => "minecraft:lime_concrete",
        n if is_red(n) => "minecraft:red_concrete",
        _ => "minecraft:black_concrete",
    };
    let name = format!("{}{}", if chosen { "» " } else { "" }, pocket_name(n));
    let mut i = icon(
        item,
        name,
        if chosen { Color::Yellow } else { Color::White },
    );
    if let Some(i) = i.as_mut() {
        i.count = n.max(1);
    }
    i
}

/// What a click on the table does.
#[derive(Debug, PartialEq)]
pub enum Click {
    Nothing,
    Show,
    Spin,
}

/// A click on the table (not its bottom row).
pub fn click(seat: &mut Seat, slot: usize) -> Click {
    let t = &mut seat.roulette;
    if seat.view == View::RouletteNumbers {
        let bet = match slot {
            0..36 => Bet::Number(slot as u8 + 1),
            40 => Bet::Number(0),
            36 => {
                seat.view = View::Roulette;
                return Click::Show;
            }
            _ => return Click::Nothing,
        };
        t.bet = Some(bet);
        seat.view = View::Roulette;
        return Click::Show;
    }
    let bet = match slot {
        18 => Bet::Red,
        19 => Bet::Black,
        20 => Bet::Odd,
        21 => Bet::Even,
        22 => Bet::Low,
        23 => Bet::High,
        24..=26 => Bet::Dozen(slot as u8 - 23),
        27..=29 => Bet::Column(slot as u8 - 26),
        33 => Bet::Number(0),
        31 => {
            seat.view = View::RouletteNumbers;
            return Click::Show;
        }
        53 => return Click::Spin,
        _ => return Click::Nothing,
    };
    t.bet = Some(bet);
    Click::Show
}

#[cfg(test)]
mod tests {
    use super::super::smp_casino::{Fx, Game, POT, tests::*};
    use super::*;

    #[test]
    fn the_wheel_holds_each_number_once_and_the_colours_alternate() {
        let mut seen = WHEEL.to_vec();
        seen.sort_unstable();
        assert_eq!(seen, (0..=36).collect::<Vec<u8>>());
        for pair in WHEEL[1..].windows(2) {
            assert_ne!(is_red(pair[0]), is_red(pair[1]), "{pair:?}");
        }
    }

    #[test]
    fn every_bet_pays_its_true_odds_and_the_edge_is_one_in_37() {
        let mut bets = vec![
            Bet::Red,
            Bet::Black,
            Bet::Odd,
            Bet::Even,
            Bet::Low,
            Bet::High,
        ];
        bets.extend((1..=3).flat_map(|k| [Bet::Dozen(k), Bet::Column(k)]));
        bets.extend((0..=36).map(Bet::Number));
        for bet in bets {
            let winners = (0..=36u8).filter(|n| bet.wins(*n)).count() as i64;
            assert_eq!(
                winners * bet.multiple(),
                36,
                "{bet:?}: 36 of 37 coins paid back"
            );
            // Over the 37 pockets, a stake of 37 x 100 gets back 36 x 100.
            let back: i64 = (0..=36).map(|n| bet.payout(n, 100)).sum();
            assert_eq!(back, 3600, "{bet:?}");
        }
        assert!(!Bet::Red.wins(0) && !Bet::Even.wins(0) && !Bet::Low.wins(0));
        assert!(Bet::Column(1).wins(34) && Bet::Column(3).wins(36) && Bet::Dozen(3).wins(25));
        assert_eq!(Bet::Number(17).payout(17, 10), 360);
        assert_eq!(Bet::Dozen(2).payout(13, 10), 30);
        assert_eq!(Bet::Black.payout(17, 10), 20);
    }

    #[test]
    fn the_wheel_slows_down_and_lands_on_the_drawn_pocket() {
        let mut rng = Rng::from_seed([3; 32]);
        for from in [0, 5, 36] {
            for _ in 0..50 {
                let mut r = Roulette::default();
                let stake = Stake {
                    uuid: A,
                    name: "Alex".into(),
                    game: Game::Roulette,
                    amount: 10,
                };
                r.spin(stake, Bet::Red, from, 0, &mut rng);
                let s = &r.rounds[0];
                assert_eq!(s.position(0), from);
                assert_eq!(s.position(FRAMES), s.stop);
                let steps: Vec<i64> = (1..=FRAMES)
                    .map(|f| {
                        let done = |f: i64| {
                            s.total * (FRAMES * FRAMES - (FRAMES - f).pow(2)) / (FRAMES * FRAMES)
                        };
                        done(f) - done(f - 1)
                    })
                    .collect();
                assert!(
                    steps.windows(2).all(|w| w[0] >= w[1] - 1),
                    "slows down: {steps:?}"
                );
                assert!(steps[0] >= 7 && *steps.last().unwrap() <= 1, "{steps:?}");
            }
        }
    }

    #[test]
    fn a_spin_through_the_menu_pays_once_even_when_the_menu_closes() {
        let (mut e, mut c, dir) = setup("roulette", 50_000);
        let before = money(&e);
        c.sit(A, "Alex", View::Roulette);
        c.seats.get_mut(&A).unwrap().window = Some(3);
        assert!(
            c.click(&mut e, A, 53, false, 0)
                .iter()
                .any(|f| matches!(f, Fx::Bar { .. })),
            "no bet yet"
        );
        c.click(&mut e, A, 49, false, 0);
        assert_eq!(c.seats[&A].stake, 25);
        c.click(&mut e, A, 18, false, 0);
        assert_eq!(c.seats[&A].roulette.bet, Some(Bet::Red));
        c.click(&mut e, A, 53, false, 0);
        // A double click: the second spin is refused, nothing more held.
        c.click(&mut e, A, 53, false, 50);
        assert_eq!(e.held(A), 25);
        // Frames: one picture per 200 ms, not per tick.
        let shows = |fx: &[Fx]| fx.iter().filter(|f| matches!(f, Fx::Show(A))).count();
        assert_eq!(
            shows(&c.tick(&mut e, 10)),
            0,
            "the spin's click showed frame 0"
        );
        assert_eq!(shows(&c.tick(&mut e, 210)), 1);
        assert_eq!(shows(&c.tick(&mut e, 260)), 0);
        assert_eq!(shows(&c.tick(&mut e, 400)), 1);
        // The menu closes mid-spin: the round still ends and is paid, once.
        assert!(c.closed(A, 3, 0));
        let n = WHEEL[c.roulette.rounds[0].stop];
        c.tick(&mut e, 3999);
        assert_eq!(e.held(A), 25, "not before the wheel stops");
        c.tick(&mut e, 4000);
        c.tick(&mut e, 4200);
        assert_eq!(e.held(A), 0);
        let expected = 10_000 - 25 + Bet::Red.payout(n, 25);
        assert_eq!(e.balance(A), expected);
        assert_eq!(money(&e), before);
        assert_eq!(c.ledger.totals["roulette"].rounds, 1);
        assert!(!c.animating());
        let _ = e.pool(POT);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn many_spins_keep_about_the_house_edge() {
        let (mut e, mut c, dir) = setup("edge", 1_000_000_000);
        c.config.daily_loss_cap = i64::MAX / 4;
        c.config.daily_win_cap = i64::MAX / 4;
        c.config.cooldown_ms = 0;
        let mut rng = Rng::from_seed([3; 32]);
        let mut now = 0;
        for i in 0..20_000 {
            if e.balance(A) < 100 {
                e.adjust("eco_give", A, 1_000_000, "test").unwrap();
            }
            let stake = c.bet(&mut e, A, "Alex", Game::Roulette, 10, now).unwrap();
            // Even-money bets: little spread, so 20,000 spins show the edge.
            let bet = if i % 2 == 0 { Bet::Red } else { Bet::Odd };
            c.roulette.spin(stake, bet, 0, now, &mut rng);
            now += FRAMES * FRAME_MS;
            c.tick(&mut e, now);
        }
        let t = &c.ledger.totals["roulette"];
        let rtp = t.paid as f64 / t.wagered as f64;
        assert!((0.94..1.0).contains(&rtp), "{rtp}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
