// Player trades (`/trade`): the requests, both sides' offers and
// confirmations, and the trade menu's layout, with no game types (`S` is the
// game's item stack). The menu is display only and the offered stacks stay
// in their owners' inventories: the glue swaps them in one checked step once
// both have confirmed (`native_exchange_items`), so every way out of a trade
// (closing, `/trade cancel`, leaving, dying) moves nothing. Added to the game
// as `game::smp_trade`; also a module of this crate, for its tests.

use super::smp_economy::{Economy, MAX_AMOUNT, Out, Player, parse_amount};

/// How long a request waits for an answer.
pub const REQUEST_MS: i64 = 60_000;
/// Stacks one side may offer (the menu's 4 by 4 squares).
pub const OFFER_SLOTS: usize = 16;
/// The menu's rows: a 6-row chest.
pub const ROWS: u8 = 6;
pub const SIZE: usize = ROWS as usize * 9;
/// The own side's buttons, and the other side's status.
pub const MONEY: usize = 37;
pub const OTHER_MONEY: usize = 43;
pub const CONFIRM: usize = 45;
pub const CANCEL: usize = 48;
pub const OTHER_STATUS: usize = 52;

/// The menu slot of the `i`th square of an offer: the own side on the left
/// (columns 0 to 3), the other's on the right (5 to 8), four rows.
pub fn offer_slot(i: usize, own: bool) -> usize {
    (i / 4) * 9 + i % 4 + if own { 0 } else { 5 }
}

/// Which square of the own offer a menu slot is.
fn own_square(slot: usize) -> Option<usize> {
    let (row, col) = (slot / 9, slot % 9);
    (row < 4 && col < 4).then_some(row * 4 + col)
}

/// One side of a trade.
#[derive(Clone, Debug, PartialEq)]
pub struct Side<S> {
    pub uuid: u128,
    pub name: String,
    /// Inventory slots offered, with the stack each held when offered: the
    /// stack it must still hold at the swap.
    pub offer: Vec<(usize, S)>,
    pub money: i64,
    pub confirmed: bool,
    /// The menu's window, once open.
    pub window: Option<i32>,
}

impl<S> Side<S> {
    fn new(uuid: u128, name: &str) -> Self {
        Self {
            uuid,
            name: name.to_owned(),
            offer: Vec::new(),
            money: 0,
            confirmed: false,
            window: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trade<S> {
    pub sides: [Side<S>; 2],
}

impl<S> Trade<S> {
    /// The side of `uuid` (0 or 1).
    pub fn side_of(&self, uuid: u128) -> Option<usize> {
        self.sides.iter().position(|s| s.uuid == uuid)
    }
    fn unconfirm(&mut self) {
        for s in &mut self.sides {
            s.confirmed = false;
        }
    }
}

struct Request {
    from: u128,
    from_name: String,
    to: u128,
    at: i64,
}

/// Finds a player who is on by name: their uuid, name, and whether they
/// may trade (survival or adventure).
pub type Online<'a> = dyn Fn(&str) -> Option<(u128, String, bool)> + 'a;

/// What the glue does for the trades.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Open (or show again) the trade menu of the trade `uuid` is in.
    Show(u128),
    /// Close the player's trade menu (their trade ended).
    Close(u128),
    /// Both confirmed: swap the trade `uuid` is in.
    Settle(u128),
}

/// What a menu cell shows; the glue makes the item.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell<S> {
    Empty,
    Stack(S),
    Divider,
    /// A side's money: whether it is the viewer's, the amount.
    Money(bool, i64),
    /// The viewer's confirm button: confirmed or not.
    Confirm(bool),
    Cancel,
    /// The other side: their name, confirmed or not.
    Other(String, bool),
}

/// The open trades and the requests waiting.
pub struct Trades<S> {
    requests: Vec<Request>,
    pub open: Vec<Trade<S>>,
}

impl<S> Default for Trades<S> {
    fn default() -> Self {
        Self {
            requests: Vec::new(),
            open: Vec::new(),
        }
    }
}

impl<S: Clone + PartialEq> Trades<S> {
    pub fn trade_of(&self, uuid: u128) -> Option<&Trade<S>> {
        self.open.iter().find(|t| t.side_of(uuid).is_some())
    }
    fn index_of(&self, uuid: u128) -> Option<usize> {
        self.open.iter().position(|t| t.side_of(uuid).is_some())
    }

    /// Ends the trade `uuid` is in, nothing moved: its players' menus close
    /// and both are told why.
    pub fn end(&mut self, uuid: u128, why: &str) -> (Vec<Out>, Vec<Act>) {
        let Some(i) = self.index_of(uuid) else {
            return (Vec::new(), Vec::new());
        };
        let t = self.open.remove(i);
        let mut out = Vec::new();
        let mut acts = Vec::new();
        for s in &t.sides {
            out.push(Out::Tell(
                s.uuid,
                format!("Trade cancelled: {why}. Nothing was exchanged."),
            ));
            acts.push(Act::Close(s.uuid));
        }
        (out, acts)
    }

    /// `/trade <player>`, `/trade accept|deny [player]`, `/trade cancel`,
    /// `/trade money <amount>`; None for another command. `online` finds a
    /// player who is on by name (uuid, name, may trade).
    pub fn command(
        &mut self,
        economy: &Economy,
        me: &Player,
        may_trade: bool,
        online: &Online,
        now: i64,
        line: &str,
    ) -> Option<(Vec<Out>, Vec<Act>)> {
        let mut words = line.split_whitespace();
        if words.next()? != "trade" {
            return None;
        }
        self.requests.retain(|r| now - r.at < REQUEST_MS);
        let err = |s: &str| Some((vec![Out::Error(s.to_owned())], Vec::new()));
        let usage = "Usage: /trade <player>, /trade accept|deny [player], /trade cancel, /trade money <amount>";
        let arg = words.next();
        let rest = words.next();
        match arg {
            None => err(usage),
            Some("cancel") => {
                if self.index_of(me.uuid).is_some() {
                    return Some(self.end(me.uuid, &format!("{} cancelled", me.name)));
                }
                let before = self.requests.len();
                self.requests.retain(|r| r.from != me.uuid);
                if self.requests.len() < before {
                    Some((
                        vec![Out::Reply("Your trade request is withdrawn.".into())],
                        Vec::new(),
                    ))
                } else {
                    err("You have no trade to cancel.")
                }
            }
            Some("money") => {
                let Some(i) = self.index_of(me.uuid) else {
                    return err("You are not in a trade.");
                };
                let Some(amount) = rest
                    .and_then(parse_amount)
                    .or(rest.filter(|r| *r == "0").map(|_| 0))
                else {
                    return err("Usage: /trade money <amount> (0 to offer none)");
                };
                if amount > MAX_AMOUNT || amount > economy.balance(me.uuid) {
                    return err(&format!(
                        "You only have {}.",
                        economy.money(economy.balance(me.uuid))
                    ));
                }
                let t = &mut self.open[i];
                let side = t.side_of(me.uuid).expect("found");
                t.sides[side].money = amount;
                t.unconfirm();
                let other = t.sides[1 - side].uuid;
                Some((
                    vec![
                        Out::Reply(format!("You offer {}.", economy.money(amount))),
                        Out::Tell(
                            other,
                            format!("{} offers {}.", me.name, economy.money(amount)),
                        ),
                    ],
                    vec![Act::Show(me.uuid), Act::Show(other)],
                ))
            }
            Some(verb @ ("accept" | "deny")) => {
                let pick = |r: &&Request| {
                    r.to == me.uuid && rest.is_none_or(|n| r.from_name.eq_ignore_ascii_case(n))
                };
                let Some(r) = self.requests.iter().rfind(pick) else {
                    return err("You have no trade request to answer.");
                };
                let (from, from_name) = (r.from, r.from_name.clone());
                self.requests
                    .retain(|r| !(r.from == from && r.to == me.uuid));
                if verb == "deny" {
                    return Some((
                        vec![
                            Out::Reply(format!("You refused {from_name}'s trade.")),
                            Out::Tell(from, format!("{} refused your trade.", me.name)),
                        ],
                        Vec::new(),
                    ));
                }
                if !may_trade {
                    return err("You can trade only in survival or adventure mode.");
                }
                let Some((_, _, they_may)) = online(&from_name).filter(|(u, _, _)| *u == from)
                else {
                    return err(&format!("{from_name} is no longer here."));
                };
                if !they_may {
                    return err(&format!("{from_name} can't trade in their game mode."));
                }
                if self.index_of(me.uuid).is_some() || self.index_of(from).is_some() {
                    return err("One of you is already in a trade.");
                }
                self.requests
                    .retain(|r| r.from != from && r.from != me.uuid);
                self.open.push(Trade {
                    sides: [Side::new(from, &from_name), Side::new(me.uuid, &me.name)],
                });
                Some((Vec::new(), vec![Act::Show(from), Act::Show(me.uuid)]))
            }
            Some(name) => {
                if rest.is_some() {
                    return err(usage);
                }
                if !may_trade {
                    return err("You can trade only in survival or adventure mode.");
                }
                let Some((to, to_name, they_may)) = online(name) else {
                    return err(&format!("{name} is not here."));
                };
                if to == me.uuid {
                    return err("You can't trade with yourself.");
                }
                if !they_may {
                    return err(&format!("{to_name} can't trade in their game mode."));
                }
                if self.index_of(me.uuid).is_some() || self.index_of(to).is_some() {
                    return err("One of you is already in a trade.");
                }
                // A request back is an answer.
                if self
                    .requests
                    .iter()
                    .any(|r| r.from == to && r.to == me.uuid)
                {
                    return self.command(
                        economy,
                        me,
                        may_trade,
                        online,
                        now,
                        &format!("trade accept {to_name}"),
                    );
                }
                self.requests.retain(|r| r.from != me.uuid);
                self.requests.push(Request {
                    from: me.uuid,
                    from_name: me.name.clone(),
                    to,
                    at: now,
                });
                Some((
                    vec![
                        Out::Reply(format!("Trade request sent to {to_name}.")),
                        Out::Tell(
                            to,
                            format!(
                                "{} wants to trade with you: /trade accept {} or /trade deny {} (60 s).",
                                me.name, me.name, me.name
                            ),
                        ),
                    ],
                    Vec::new(),
                ))
            }
        }
    }

    /// A click in `uuid`'s trade menu: on their inventory's slot `inventory`
    /// holding `stack` (offered whole), or on the menu's slot `menu`.
    pub fn click(
        &mut self,
        uuid: u128,
        menu: Option<usize>,
        inventory: Option<(usize, S)>,
    ) -> (Vec<Out>, Vec<Act>) {
        let Some(i) = self.index_of(uuid) else {
            return (Vec::new(), Vec::new());
        };
        let t = &mut self.open[i];
        let me = t.side_of(uuid).expect("found");
        let other = t.sides[1 - me].uuid;
        let shown = vec![Act::Show(uuid), Act::Show(other)];
        if let Some((slot, stack)) = inventory {
            let side = &mut t.sides[me];
            if side.offer.iter().any(|(s, _)| *s == slot) {
                return (Vec::new(), Vec::new());
            }
            if side.offer.len() >= OFFER_SLOTS {
                return (
                    vec![Out::Tell(
                        uuid,
                        format!("You can offer {OFFER_SLOTS} stacks at most."),
                    )],
                    Vec::new(),
                );
            }
            side.offer.push((slot, stack));
            t.unconfirm();
            return (Vec::new(), shown);
        }
        match menu {
            Some(CANCEL) => {
                let why = format!("{} cancelled", t.sides[me].name);
                self.end(uuid, &why)
            }
            Some(CONFIRM) => {
                t.sides[me].confirmed = !t.sides[me].confirmed;
                if t.sides.iter().all(|s| s.confirmed) {
                    (Vec::new(), vec![Act::Settle(uuid)])
                } else {
                    (Vec::new(), shown)
                }
            }
            Some(slot) => match own_square(slot).filter(|&q| q < t.sides[me].offer.len()) {
                Some(q) => {
                    t.sides[me].offer.remove(q);
                    t.unconfirm();
                    (Vec::new(), shown)
                }
                None => (Vec::new(), Vec::new()),
            },
            None => (Vec::new(), Vec::new()),
        }
    }

    /// Checks `uuid`'s offer against their inventory now (`stacks`, by
    /// slot): a stack that changed (used, dropped, grown by a pickup) leaves
    /// the offer and both confirmations are cleared. Whether anything left.
    pub fn refresh(&mut self, uuid: u128, stacks: &[Option<S>]) -> bool {
        let Some(i) = self.index_of(uuid) else {
            return false;
        };
        let t = &mut self.open[i];
        let me = t.side_of(uuid).expect("found");
        let before = t.sides[me].offer.len();
        t.sides[me]
            .offer
            .retain(|(slot, s)| stacks.get(*slot).and_then(|x| x.as_ref()) == Some(s));
        let changed = t.sides[me].offer.len() < before;
        if changed {
            t.unconfirm();
        }
        changed
    }

    /// What `uuid`'s trade menu shows, slot by slot.
    pub fn layout(&self, uuid: u128) -> Vec<Cell<S>> {
        let mut cells = vec![Cell::Empty; SIZE];
        let Some(t) = self.trade_of(uuid) else {
            return cells;
        };
        let me = t.side_of(uuid).expect("found");
        let (mine, theirs) = (&t.sides[me], &t.sides[1 - me]);
        for row in 0..ROWS as usize {
            cells[row * 9 + 4] = Cell::Divider;
        }
        for (i, (_, s)) in mine.offer.iter().enumerate() {
            cells[offer_slot(i, true)] = Cell::Stack(s.clone());
        }
        for (i, (_, s)) in theirs.offer.iter().enumerate() {
            cells[offer_slot(i, false)] = Cell::Stack(s.clone());
        }
        cells[MONEY] = Cell::Money(true, mine.money);
        cells[OTHER_MONEY] = Cell::Money(false, theirs.money);
        cells[CONFIRM] = Cell::Confirm(mine.confirmed);
        cells[CANCEL] = Cell::Cancel;
        cells[OTHER_STATUS] = Cell::Other(theirs.name.clone(), theirs.confirmed);
        cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u128 = 1;
    const B: u128 = 2;
    type S = (&'static str, u32);

    fn economy() -> Economy {
        // One folder per call: the tests run side by side.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("smp-trade-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut e = Economy::open(&dir, Default::default()).unwrap();
        e.seen(A, "Alex");
        e.seen(B, "Steve");
        e.adjust("eco_give", A, 100, "test").unwrap();
        e
    }
    fn player(uuid: u128) -> Player {
        Player {
            uuid,
            name: if uuid == A { "Alex" } else { "Steve" }.into(),
            level: 0,
        }
    }
    fn online(name: &str) -> Option<(u128, String, bool)> {
        match name.to_ascii_lowercase().as_str() {
            "alex" => Some((A, "Alex".into(), true)),
            "steve" => Some((B, "Steve".into(), true)),
            _ => None,
        }
    }
    fn open() -> (Economy, Trades<S>) {
        let e = economy();
        let mut t = Trades::default();
        t.command(&e, &player(A), true, &online, 0, "trade Steve")
            .unwrap();
        let (_, acts) = t
            .command(&e, &player(B), true, &online, 1_000, "trade accept")
            .unwrap();
        assert_eq!(acts, [Act::Show(A), Act::Show(B)]);
        (e, t)
    }

    #[test]
    fn a_request_opens_a_trade_once_accepted() {
        let (_, t) = open();
        assert_eq!(t.open.len(), 1);
        let cells = t.layout(B);
        assert_eq!(cells[OTHER_STATUS], Cell::Other("Alex".into(), false));
        assert_eq!(cells[CONFIRM], Cell::Confirm(false));
    }

    #[test]
    fn requests_expire_and_can_be_refused() {
        let e = economy();
        let mut t: Trades<S> = Trades::default();
        t.command(&e, &player(A), true, &online, 0, "trade Steve")
            .unwrap();
        let (out, _) = t
            .command(&e, &player(B), true, &online, REQUEST_MS, "trade accept")
            .unwrap();
        assert!(matches!(&out[0], Out::Error(_)));
        t.command(&e, &player(A), true, &online, 0, "trade Steve")
            .unwrap();
        t.command(&e, &player(B), true, &online, 10, "trade deny Alex")
            .unwrap();
        let (out, _) = t
            .command(&e, &player(B), true, &online, 20, "trade accept")
            .unwrap();
        assert!(matches!(&out[0], Out::Error(_)));
        // Creative players don't trade; nor does a player with themselves.
        let (out, _) = t
            .command(&e, &player(A), false, &online, 0, "trade Steve")
            .unwrap();
        assert!(matches!(&out[0], Out::Error(_)));
        let (out, _) = t
            .command(&e, &player(A), true, &online, 0, "trade Alex")
            .unwrap();
        assert!(matches!(&out[0], Out::Error(_)));
        assert!(t.open.is_empty());
    }

    #[test]
    fn any_change_clears_both_confirmations() {
        let (e, mut t) = open();
        t.click(A, None, Some((3, ("minecraft:diamond", 5))));
        t.click(A, Some(CONFIRM), None);
        t.click(B, Some(CONFIRM), None);
        // Both confirmed: settled; a click after the settle's failure, or a
        // change, needs both again.
        assert_eq!(t.open[0].sides.iter().filter(|s| s.confirmed).count(), 2);
        t.click(B, None, Some((0, ("minecraft:emerald", 1))));
        assert!(t.open[0].sides.iter().all(|s| !s.confirmed));
        t.click(A, Some(CONFIRM), None);
        t.command(&e, &player(B), true, &online, 0, "trade money 0")
            .unwrap();
        assert!(t.open[0].sides.iter().all(|s| !s.confirmed));
        // Taking a stack back out of the offer.
        t.click(A, Some(CONFIRM), None);
        t.click(A, Some(offer_slot(0, true)), None);
        assert!(t.open[0].sides[0].offer.is_empty());
        assert!(!t.open[0].sides[0].confirmed);
    }

    #[test]
    fn the_second_confirmation_settles() {
        let (_, mut t) = open();
        t.click(A, None, Some((3, ("minecraft:diamond", 5))));
        assert_eq!(
            t.click(A, Some(CONFIRM), None).1,
            [Act::Show(A), Act::Show(B)]
        );
        assert_eq!(t.click(B, Some(CONFIRM), None).1, [Act::Settle(B)]);
    }

    #[test]
    fn an_offer_is_one_slot_once_and_sixteen_at_most() {
        let (_, mut t) = open();
        t.click(A, None, Some((3, ("minecraft:diamond", 5))));
        t.click(A, None, Some((3, ("minecraft:diamond", 5))));
        assert_eq!(t.open[0].sides[0].offer.len(), 1);
        for slot in 10..30 {
            t.click(A, None, Some((slot, ("minecraft:dirt", 1))));
        }
        assert_eq!(t.open[0].sides[0].offer.len(), OFFER_SLOTS);
        assert_eq!(
            t.layout(B)[offer_slot(0, false)],
            Cell::Stack(("minecraft:diamond", 5))
        );
    }

    #[test]
    fn a_stack_that_changed_leaves_the_offer() {
        let (_, mut t) = open();
        t.click(A, None, Some((3, ("minecraft:diamond", 5))));
        t.click(A, None, Some((4, ("minecraft:dirt", 1))));
        t.click(A, Some(CONFIRM), None);
        let mut inv: Vec<Option<S>> = vec![None; 36];
        inv[3] = Some(("minecraft:diamond", 5));
        inv[4] = Some(("minecraft:dirt", 1));
        assert!(!t.refresh(A, &inv));
        assert!(t.open[0].sides[0].confirmed);
        // A pickup grew the diamonds.
        inv[3] = Some(("minecraft:diamond", 6));
        assert!(t.refresh(A, &inv));
        assert_eq!(t.open[0].sides[0].offer, [(4, ("minecraft:dirt", 1))]);
        assert!(!t.open[0].sides[0].confirmed);
    }

    #[test]
    fn every_way_out_ends_the_trade_with_nothing_moved() {
        // Cancel button, /trade cancel; closing and leaving go through
        // `end` from the glue.
        let (e, mut t) = open();
        let (out, acts) = t.click(B, Some(CANCEL), None);
        assert_eq!(acts, [Act::Close(A), Act::Close(B)]);
        assert!(matches!(&out[0], Out::Tell(A, s) if s.contains("Nothing was exchanged")));
        assert!(t.open.is_empty());
        let (_, mut t) = open();
        t.command(&e, &player(A), true, &online, 0, "trade cancel")
            .unwrap();
        assert!(t.open.is_empty());
        let (_, mut t) = open();
        t.end(B, "Steve left");
        assert!(t.open.is_empty());
        // Money beyond the balance is refused when offered.
        let (_, mut t) = open();
        let (out, _) = t
            .command(&e, &player(A), true, &online, 0, "trade money 101")
            .unwrap();
        assert!(matches!(&out[0], Out::Error(_)));
        t.command(&e, &player(A), true, &online, 0, "trade money 100")
            .unwrap();
        assert_eq!(t.open[0].sides[0].money, 100);
    }
}
