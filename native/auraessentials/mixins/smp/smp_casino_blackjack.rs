// The casino's blackjack: one player against the dealer, six decks shuffled
// for every hand. The dealer stands on all 17s and checks for blackjack
// first; a blackjack pays 3 to 2; the player may double on their first two
// cards (one more card, the bet doubled); no split, insurance or surrender.
// With basic strategy the house keeps about 1% (the test plays 400,000
// hands). The dealer draws one card every 0.6 s in the menu. Closing the
// menu or leaving stands on the hand, as does a minute without a move: it
// is played out and paid all the same, once. Added to the game as
// `game::smp_casino_blackjack`; also a module of this crate, for its tests.

use super::smp_casino::{Color, Icon, Rng, Seat, Stake, filler, icon};

pub const DECKS: usize = 6;
/// Between two of the dealer's cards.
pub const DEALER_MS: i64 = 600;
/// A hand left this long without a move stands.
pub const IDLE_MS: i64 = 60_000;

pub const HELP: [&str; 1] = [
    "Blackjack (6 decks, shuffled every hand): the dealer stands on all 17s; blackjack pays 3 to 2; double on your first two cards; no split or insurance. House edge about 1% with basic strategy.",
];

/// A card: rank 1 (ace) to 13 (king), suit 0 to 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub rank: u8,
    pub suit: u8,
}

impl Card {
    pub fn value(self) -> u8 {
        self.rank.min(10)
    }
    pub fn name(self) -> String {
        let rank = match self.rank {
            1 => "A".to_owned(),
            11 => "J".to_owned(),
            12 => "Q".to_owned(),
            13 => "K".to_owned(),
            n => n.to_string(),
        };
        format!("{rank}{}", ["♠", "♥", "♦", "♣"][usize::from(self.suit % 4)])
    }
    fn red(self) -> bool {
        matches!(self.suit, 1 | 2)
    }
}

/// A hand's best total, and whether an ace counts 11 in it (soft).
pub fn total(cards: &[Card]) -> (u8, bool) {
    let hard: u8 = cards.iter().map(|c| c.value()).sum();
    if cards.iter().any(|c| c.rank == 1) && hard + 10 <= 21 {
        (hard + 10, true)
    } else {
        (hard, false)
    }
}

pub fn blackjack(cards: &[Card]) -> bool {
    cards.len() == 2 && total(cards).0 == 21
}

/// Six decks, from which a hand draws without putting back.
#[derive(Clone, Debug)]
pub struct Shoe(Vec<Card>);

impl Shoe {
    pub fn new() -> Self {
        let mut cards = Vec::with_capacity(52 * DECKS);
        for _ in 0..DECKS {
            for suit in 0..4 {
                for rank in 1..=13 {
                    cards.push(Card { rank, suit });
                }
            }
        }
        Self(cards)
    }
    pub fn draw(&mut self, rng: &mut Rng) -> Card {
        let i = rng.below(self.0.len() as u64) as usize;
        self.0.swap_remove(i)
    }
}

impl Default for Shoe {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The player's move; since when.
    Player { since: i64 },
    /// The dealer draws; their next card at.
    Dealer { next: i64 },
}

/// How a hand ended, for the line and the payout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Blackjack,
    Win,
    Push,
    Lose,
    Bust,
    DealerBlackjack,
}

impl End {
    /// What the hand pays back on `stake` (the stake included).
    pub fn payout(self, stake: i64) -> i64 {
        match self {
            End::Blackjack => stake + stake * 3 / 2,
            End::Win => stake * 2,
            End::Push => stake,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Hand {
    pub stake: Stake,
    pub player: Vec<Card>,
    pub dealer: Vec<Card>,
    pub shoe: Shoe,
    pub doubled: bool,
    pub phase: Phase,
}

impl Hand {
    /// How it ends, once the dealer is done (or at once for a blackjack or a
    /// bust).
    fn end(&self) -> Option<End> {
        let (p, _) = total(&self.player);
        let (d, _) = total(&self.dealer);
        if blackjack(&self.player) {
            return Some(if blackjack(&self.dealer) {
                End::Push
            } else {
                End::Blackjack
            });
        }
        if blackjack(&self.dealer) {
            return Some(End::DealerBlackjack);
        }
        if p > 21 {
            return Some(End::Bust);
        }
        if !matches!(self.phase, Phase::Dealer { .. }) || d < 17 {
            return None;
        }
        Some(if d > 21 || p > d {
            End::Win
        } else if p == d {
            End::Push
        } else {
            End::Lose
        })
    }
}

/// A hand played out, to be paid.
#[derive(Clone, Debug)]
pub struct Done {
    pub hand: Hand,
    pub end: End,
}

/// A player's table: the last hand, shown until the next.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub last: Option<(Vec<Card>, Vec<Card>, End)>,
}

#[derive(Default)]
pub struct Blackjack {
    pub hands: Vec<Hand>,
}

impl Blackjack {
    pub fn busy(&self) -> bool {
        !self.hands.is_empty()
    }
    pub fn playing(&self, uuid: u128) -> bool {
        self.hands.iter().any(|h| h.stake.uuid == uuid)
    }
    fn hand(&mut self, uuid: u128) -> Option<&mut Hand> {
        self.hands.iter_mut().find(|h| h.stake.uuid == uuid)
    }
    /// The bet is held: two cards each. A blackjack ends it at once.
    pub fn deal(&mut self, stake: Stake, now: i64, rng: &mut Rng) -> Option<Done> {
        let mut shoe = Shoe::new();
        let mut player = Vec::with_capacity(6);
        let mut dealer = Vec::with_capacity(6);
        for _ in 0..2 {
            player.push(shoe.draw(rng));
            dealer.push(shoe.draw(rng));
        }
        let hand = Hand {
            stake,
            player,
            dealer,
            shoe,
            doubled: false,
            phase: Phase::Player { since: now },
        };
        // The dealer checks for blackjack first.
        if let Some(end) = hand.end() {
            return Some(Done { hand, end });
        }
        self.hands.push(hand);
        None
    }
    fn finish(&mut self, uuid: u128) -> Option<Done> {
        let i = self.hands.iter().position(|h| h.stake.uuid == uuid)?;
        let end = self.hands[i].end()?;
        Some(Done {
            hand: self.hands.swap_remove(i),
            end,
        })
    }
    /// One more card; a bust ends the hand.
    pub fn hit(&mut self, uuid: u128, now: i64, rng: &mut Rng) -> Option<Done> {
        let h = self.hand(uuid)?;
        if !matches!(h.phase, Phase::Player { .. }) {
            return None;
        }
        let card = h.shoe.draw(rng);
        h.player.push(card);
        h.phase = Phase::Player { since: now };
        if total(&h.player).0 >= 21 {
            if total(&h.player).0 == 21 {
                h.phase = Phase::Dealer {
                    next: now + DEALER_MS,
                };
                return None;
            }
            return self.finish(uuid);
        }
        None
    }
    /// The dealer plays.
    pub fn stand(&mut self, uuid: u128, now: i64) -> bool {
        match self.hand(uuid) {
            Some(h) if matches!(h.phase, Phase::Player { .. }) => {
                h.phase = Phase::Dealer {
                    next: now + DEALER_MS,
                };
                true
            }
            _ => false,
        }
    }
    /// Whether `uuid` may double now (first two cards); the caller holds the
    /// second bet first, then calls `doubled`.
    pub fn may_double(&self, uuid: u128) -> bool {
        self.hands.iter().any(|h| {
            h.stake.uuid == uuid && h.player.len() == 2 && matches!(h.phase, Phase::Player { .. })
        })
    }
    /// The bet doubled (held): one card, then the dealer plays.
    pub fn doubled(&mut self, uuid: u128, extra: i64, now: i64, rng: &mut Rng) -> Option<Done> {
        let h = self.hand(uuid)?;
        h.stake.amount += extra;
        h.doubled = true;
        let card = h.shoe.draw(rng);
        h.player.push(card);
        h.phase = Phase::Dealer {
            next: now + DEALER_MS,
        };
        if total(&h.player).0 > 21 {
            return self.finish(uuid);
        }
        None
    }
    /// The dealers' cards due by `now`; hands left idle stand. The hands
    /// played out, taken out to be paid once, and those that changed.
    pub fn step(&mut self, now: i64, rng: &mut Rng) -> (Vec<Done>, Vec<u128>) {
        let mut changed = Vec::new();
        for h in &mut self.hands {
            match h.phase {
                Phase::Player { since } if now - since >= IDLE_MS => {
                    h.phase = Phase::Dealer {
                        next: now + DEALER_MS,
                    };
                    changed.push(h.stake.uuid);
                }
                Phase::Dealer { next } if now >= next => {
                    // The hidden card turns first (shown from this step), then one card a step.
                    if total(&h.dealer).0 < 17 {
                        let card = h.shoe.draw(rng);
                        h.dealer.push(card);
                        h.phase = Phase::Dealer {
                            next: now + DEALER_MS,
                        };
                    } else {
                        h.phase = Phase::Dealer { next: i64::MIN };
                    }
                    changed.push(h.stake.uuid);
                }
                _ => {}
            }
        }
        let mut done = Vec::new();
        let mut i = 0;
        while i < self.hands.len() {
            let h = &self.hands[i];
            let settled = matches!(h.phase, Phase::Dealer { next: i64::MIN });
            match h.end() {
                Some(end) if settled || matches!(end, End::Bust) => done.push(Done {
                    hand: self.hands.swap_remove(i),
                    end,
                }),
                _ => i += 1,
            }
        }
        (done, changed)
    }

    /// The table's cells (all but the bottom row's shared cells).
    pub fn layout(&self, cells: &mut [Option<Icon>], uuid: u128, seat: &Seat) {
        for c in &mut cells[..45] {
            *c = filler("minecraft:green_stained_glass_pane");
        }
        let hand = self.hands.iter().find(|h| h.stake.uuid == uuid);
        let (player, dealer, hidden, message) = match (hand, &seat.blackjack.last) {
            (Some(h), _) => {
                let hidden = matches!(h.phase, Phase::Player { .. });
                let msg = if hidden {
                    "Hit, stand or double".to_owned()
                } else {
                    "The dealer plays...".to_owned()
                };
                (&h.player[..], &h.dealer[..], hidden, msg)
            }
            (None, Some((p, d, end))) => (&p[..], &d[..], false, end_line(*end)),
            (None, None) => (
                &[][..],
                &[][..],
                false,
                "Place your bet and deal".to_owned(),
            ),
        };
        for (i, c) in dealer.iter().take(9).enumerate() {
            cells[9 + i] = if hidden && i == 1 {
                icon("minecraft:map", "Hidden card", Color::Gray)
            } else {
                card(*c)
            };
        }
        for (i, c) in player.iter().take(9).enumerate() {
            cells[27 + i] = card(*c);
        }
        if !dealer.is_empty() {
            let shown = if hidden {
                format!("Dealer shows {}", total(&dealer[..1]).0)
            } else {
                format!("Dealer: {}", show_total(dealer))
            };
            cells[4] = icon("minecraft:armor_stand", shown, Color::White);
            cells[36] = icon(
                "minecraft:name_tag",
                format!("You: {}", show_total(player)),
                Color::White,
            );
        }
        cells[22] = icon("minecraft:oak_sign", message, Color::Yellow);
        if hand.is_some_and(|h| matches!(h.phase, Phase::Player { .. })) {
            cells[38] = icon("minecraft:lime_wool", "HIT", Color::Green);
            cells[40] = icon("minecraft:red_wool", "STAND", Color::Red);
            if hand.is_some_and(|h| h.player.len() == 2) {
                cells[42] = icon(
                    "minecraft:gold_block",
                    "DOUBLE (one card, twice the bet)",
                    Color::Gold,
                );
            }
        }
        cells[53] = if hand.is_some() {
            icon("minecraft:gray_concrete", "Hand in play", Color::Gray)
        } else {
            icon("minecraft:lime_concrete", "DEAL", Color::Green)
        };
    }
}

fn show_total(cards: &[Card]) -> String {
    match total(cards) {
        (t, true) if t < 21 => format!("{t} (soft)"),
        (t, _) => t.to_string(),
    }
}

/// How a hand ended, in a line.
pub fn end_line(end: End) -> String {
    match end {
        End::Blackjack => "Blackjack! Paid 3 to 2".into(),
        End::Win => "You win".into(),
        End::Push => "Push: your bet back".into(),
        End::Lose => "The dealer wins".into(),
        End::Bust => "Bust".into(),
        End::DealerBlackjack => "The dealer has blackjack".into(),
    }
}

/// A card as a cell: its blackjack value as the count.
fn card(c: Card) -> Option<Icon> {
    let mut i = icon(
        "minecraft:paper",
        c.name(),
        if c.red() { Color::Red } else { Color::White },
    );
    if let Some(i) = i.as_mut() {
        i.count = c.value();
    }
    i
}

#[cfg(test)]
mod tests {
    use super::super::smp_casino::{Fx, Game, POT, View, tests::*};
    use super::*;

    fn c(rank: u8) -> Card {
        Card { rank, suit: 0 }
    }

    #[test]
    fn totals_soft_and_hard() {
        assert_eq!(total(&[c(1), c(6)]), (17, true));
        assert_eq!(total(&[c(1), c(6), c(10)]), (17, false));
        assert_eq!(total(&[c(1), c(1), c(9)]), (21, true));
        assert_eq!(total(&[c(13), c(12)]), (20, false));
        assert!(blackjack(&[c(1), c(11)]) && !blackjack(&[c(1), c(5), c(5)]));
        assert_eq!(End::Blackjack.payout(10), 25);
        assert_eq!(End::Blackjack.payout(15), 37, "3 to 2, rounded down");
        assert_eq!(
            (
                End::Win.payout(10),
                End::Push.payout(10),
                End::Lose.payout(10)
            ),
            (20, 10, 0)
        );
    }

    #[test]
    fn the_dealer_stands_on_all_17s() {
        let mut rng = Rng::from_seed([3; 32]);
        for _ in 0..2000 {
            let mut b = Blackjack::default();
            let stake = Stake {
                uuid: A,
                name: "Alex".into(),
                game: Game::Blackjack,
                amount: 10,
            };
            if b.deal(stake, 0, &mut rng).is_some() {
                continue;
            }
            b.stand(A, 0);
            let mut now = 0;
            loop {
                now += DEALER_MS;
                let (done, _) = b.step(now, &mut rng);
                if let Some(d) = done.into_iter().next() {
                    let (t, _) = total(&d.hand.dealer);
                    assert!(t >= 17, "{:?}", d.hand.dealer);
                    // It stopped at the first 17 or more.
                    assert!(
                        total(&d.hand.dealer[..d.hand.dealer.len() - 1]).0 < 17
                            || d.hand.dealer.len() == 2
                    );
                    break;
                }
            }
        }
    }

    /// Basic strategy for 6 decks, dealer stands on 17, no split.
    fn strategy(player: &[Card], up: Card) -> char {
        let (t, soft) = total(player);
        let u = up.value();
        let u = if u == 1 { 11 } else { u };
        let two = player.len() == 2;
        let d = |yes: bool| {
            if yes && two {
                'D'
            } else if yes {
                'H'
            } else {
                'S'
            }
        };
        if soft {
            return match t {
                13 | 14 => {
                    if (5..=6).contains(&u) && two {
                        'D'
                    } else {
                        'H'
                    }
                }
                15 | 16 => {
                    if (4..=6).contains(&u) && two {
                        'D'
                    } else {
                        'H'
                    }
                }
                17 => {
                    if (3..=6).contains(&u) && two {
                        'D'
                    } else {
                        'H'
                    }
                }
                18 => {
                    if (3..=6).contains(&u) {
                        d(true)
                    } else if u <= 8 {
                        'S'
                    } else {
                        'H'
                    }
                }
                _ => 'S',
            };
        }
        match t {
            0..=8 => 'H',
            9 => {
                if (3..=6).contains(&u) && two {
                    'D'
                } else {
                    'H'
                }
            }
            10 => {
                if u <= 9 && two {
                    'D'
                } else {
                    'H'
                }
            }
            11 => {
                if u <= 10 && two {
                    'D'
                } else {
                    'H'
                }
            }
            12 => {
                if (4..=6).contains(&u) {
                    'S'
                } else {
                    'H'
                }
            }
            13..=16 => {
                if u <= 6 {
                    'S'
                } else {
                    'H'
                }
            }
            _ => 'S',
        }
    }

    #[test]
    fn basic_strategy_gets_back_about_99_percent() {
        let mut rng = Rng::from_seed([3; 32]);
        let (mut staked, mut back) = (0i64, 0i64);
        for _ in 0..400_000 {
            let mut b = Blackjack::default();
            let stake = Stake {
                uuid: A,
                name: "Alex".into(),
                game: Game::Blackjack,
                amount: 100,
            };
            let mut done = b.deal(stake, 0, &mut rng);
            while done.is_none() {
                let h = &b.hands[0];
                if !matches!(h.phase, Phase::Player { .. }) {
                    break;
                }
                done = match strategy(&h.player, h.dealer[0]) {
                    'H' => b.hit(A, 0, &mut rng),
                    'D' => b.doubled(A, 100, 0, &mut rng),
                    _ => {
                        b.stand(A, 0);
                        None
                    }
                };
            }
            let mut now = 0;
            while done.is_none() {
                now += DEALER_MS;
                done = b.step(now, &mut rng).0.into_iter().next();
            }
            let d = done.unwrap();
            staked += d.hand.stake.amount;
            back += d.end.payout(d.hand.stake.amount);
        }
        let rtp = back as f64 / staked as f64;
        assert!((0.97..1.0).contains(&rtp), "{rtp}");
    }

    #[test]
    fn a_hand_through_the_casino_pays_once_and_closing_stands() {
        let (mut e, mut c, dir) = setup("blackjack", 100_000);
        let before = money(&e);
        let mut paid_hands = 0;
        let mut now = 0;
        c.sit(A, "Alex", View::Blackjack);
        for round in 0..40 {
            c.seats.get_mut(&A).unwrap().window = Some(round);
            c.click(&mut e, A, 53, false, now);
            c.click(&mut e, A, 53, false, now + 10);
            assert!(e.held(A) <= 10, "one deal per click");
            if c.blackjack.playing(A) {
                if round % 3 == 0 {
                    // Double, with the second bet held.
                    c.click(&mut e, A, 42, false, now + 20);
                    c.click(&mut e, A, 42, false, now + 30);
                    assert!(e.held(A) <= 20);
                } else if round % 3 == 1 {
                    // The menu closes: the hand stands.
                    assert!(c.closed(A, round, 0));
                    c.sit(A, "Alex", View::Blackjack);
                } else {
                    // Stand by hand; a second click does nothing more.
                    c.click(&mut e, A, 40, false, now + 20);
                    c.click(&mut e, A, 40, false, now + 30);
                }
            }
            for _ in 0..20 {
                now += 300;
                c.tick(&mut e, now);
            }
            if !c.blackjack.playing(A) {
                paid_hands += 1;
            }
            assert_eq!(money(&e), before);
            now += 5000;
        }
        assert_eq!(paid_hands, 40);
        assert_eq!(e.held(A), 0);
        assert_eq!(c.ledger.totals["blackjack"].rounds, 40);
        let _ = (e.pool(POT), Fx::BossHide);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
