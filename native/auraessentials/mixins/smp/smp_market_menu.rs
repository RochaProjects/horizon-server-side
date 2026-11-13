// `/market`: the moving market as a chest menu. The first page shows the
// categories (the shop's) and the day's movers; a category's page its items,
// each with its buy and sell price, its change over 1 h and 24 h (green ▲,
// red ▼) and its volume; an item's page buys or sells 1, 16 or 64, sells all
// the player carries, and gives the price chart of the last 24 h or 7 days
// as a map (smp_market_chart). The menus follow the prices at each minute.
// Buying and selling go through the market's rules (smp_market). This file
// lays the pages out and says what a click means. No game types. Added to
// the game as `game::smp_market_menu`; also a module of this crate, for its
// tests.

use super::smp_market::Config as MarketConfig;
use super::smp_shop::Config as ShopConfig;
use std::collections::HashMap;

pub const ROWS: u8 = 6;
pub const SIZE: usize = 54;
const PER_PAGE: usize = 45;
const BACK: usize = 45;
const NEWS: usize = 46;
const PREV: usize = 47;
const BALANCE: usize = 48;
const NEXT: usize = 51;
const CLOSE: usize = 53;
const CATEGORY_SLOTS: [usize; 21] = [
    10, 11, 12, 13, 14, 15, 16, 19, 20, 21, 22, 23, 24, 25, 28, 29, 30, 31, 32, 33, 34,
];
/// An item's page.
const ICON: usize = 13;
const BUYS: [(usize, u32); 3] = [(19, 1), (20, 16), (21, 64)];
const SELLS: [(usize, u32); 3] = [(23, 1), (24, 16), (25, 64)];
const SELL_ALL: usize = 31;
const CHART_DAY: usize = 39;
const CHART_WEEK: usize = 41;

/// Which chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Chart {
    Day,
    Week,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Main,
    /// A category (index, or the categories' count for "Other"), its page.
    Category(usize, usize),
    /// An item, and the category page to go back to.
    Item(String, usize, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Empty,
    Filler,
    /// Its icon, name, how many items.
    Category(String, String, usize),
    /// An item in a list (its prices and moves in the lore).
    Item(String),
    /// The item of the page, with every number.
    Detail(String),
    Buy(String, u32),
    Sell(String, u32),
    SellAll(String),
    Chart(String, Chart),
    News,
    Back,
    Prev,
    Next,
    Balance,
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Click {
    Nothing,
    /// The page changed: show it.
    Show,
    Buy(String, u32),
    Sell(String, u32),
    SellAll(String),
    Chart(String, Chart),
    Close,
}

/// The categories: the shop's, then "Other" for the market's items in none.
pub fn categories(shop: &ShopConfig, market: &MarketConfig) -> Vec<(String, String, Vec<String>)> {
    let mut out: Vec<(String, String, Vec<String>)> = shop
        .categories
        .iter()
        .map(|c| {
            let items = c
                .items
                .iter()
                .filter(|i| market.items.contains_key(*i))
                .cloned()
                .collect();
            (c.icon.clone(), c.name.clone(), items)
        })
        .filter(|c: &(String, String, Vec<String>)| !c.2.is_empty())
        .collect();
    let other: Vec<String> = market
        .items
        .keys()
        .filter(|i| !shop.categories.iter().any(|c| c.items.contains(i)))
        .cloned()
        .collect();
    if !other.is_empty() {
        out.push(("minecraft:chest".into(), "Other".into(), other));
    }
    out
}

/// The open `/market` menus: window and page, by player.
#[derive(Default)]
pub struct Menus {
    pub views: HashMap<u128, (Option<i32>, Page)>,
}

impl Menus {
    pub fn layout(&self, shop: &ShopConfig, market: &MarketConfig, uuid: u128) -> Vec<Cell> {
        let Some((_, page)) = self.views.get(&uuid) else {
            return Vec::new();
        };
        let cats = categories(shop, market);
        let mut cells = vec![Cell::Empty; SIZE];
        for c in cells.iter_mut().skip(PER_PAGE) {
            *c = Cell::Filler;
        }
        cells[BALANCE] = Cell::Balance;
        cells[CLOSE] = Cell::Close;
        match page {
            Page::Main => {
                for (slot, (icon, name, items)) in CATEGORY_SLOTS.iter().zip(&cats) {
                    cells[*slot] = Cell::Category(icon.clone(), name.clone(), items.len());
                }
                cells[NEWS] = Cell::News;
            }
            Page::Category(c, p) => {
                let items = cats.get(*c).map(|c| c.2.as_slice()).unwrap_or_default();
                for (i, id) in items.iter().skip(p * PER_PAGE).take(PER_PAGE).enumerate() {
                    cells[i] = Cell::Item(id.clone());
                }
                cells[BACK] = Cell::Back;
                if *p > 0 {
                    cells[PREV] = Cell::Prev;
                }
                if (p + 1) * PER_PAGE < items.len() {
                    cells[NEXT] = Cell::Next;
                }
            }
            Page::Item(id, ..) => {
                for c in cells.iter_mut().take(PER_PAGE) {
                    *c = Cell::Filler;
                }
                cells[ICON] = Cell::Detail(id.clone());
                let p = market.items.get(id).copied().unwrap_or_default();
                for (slot, n) in BUYS {
                    cells[slot] = if p.buy > 0 {
                        Cell::Buy(id.clone(), n)
                    } else {
                        Cell::Filler
                    };
                }
                for (slot, n) in SELLS {
                    cells[slot] = if p.sell > 0 {
                        Cell::Sell(id.clone(), n)
                    } else {
                        Cell::Filler
                    };
                }
                if p.sell > 0 {
                    cells[SELL_ALL] = Cell::SellAll(id.clone());
                }
                cells[CHART_DAY] = Cell::Chart(id.clone(), Chart::Day);
                cells[CHART_WEEK] = Cell::Chart(id.clone(), Chart::Week);
                cells[BACK] = Cell::Back;
            }
        }
        cells
    }

    /// A click on the menu's `slot`: the page moves here; a trade is the
    /// glue's to make.
    pub fn click(
        &mut self,
        shop: &ShopConfig,
        market: &MarketConfig,
        uuid: u128,
        slot: usize,
    ) -> Click {
        let cells = self.layout(shop, market, uuid);
        let Some((_, page)) = self.views.get_mut(&uuid) else {
            return Click::Nothing;
        };
        let cats = categories(shop, market);
        match (cells.get(slot), page.clone()) {
            (Some(Cell::Close), _) => Click::Close,
            (Some(Cell::Category(..)), Page::Main) => {
                match CATEGORY_SLOTS.iter().position(|s| *s == slot) {
                    Some(i) if i < cats.len() => {
                        *page = Page::Category(i, 0);
                        Click::Show
                    }
                    _ => Click::Nothing,
                }
            }
            (Some(Cell::Item(id)), Page::Category(c, p)) => {
                *page = Page::Item(id.clone(), c, p);
                Click::Show
            }
            (Some(Cell::Back), Page::Category(..)) => {
                *page = Page::Main;
                Click::Show
            }
            (Some(Cell::Back), Page::Item(_, c, p)) => {
                *page = Page::Category(c, p);
                Click::Show
            }
            (Some(Cell::Prev), Page::Category(c, p)) => {
                *page = Page::Category(c, p.saturating_sub(1));
                Click::Show
            }
            (Some(Cell::Next), Page::Category(c, p)) => {
                *page = Page::Category(c, p + 1);
                Click::Show
            }
            (Some(Cell::Buy(id, n)), _) => Click::Buy(id.clone(), *n),
            (Some(Cell::Sell(id, n)), _) => Click::Sell(id.clone(), *n),
            (Some(Cell::SellAll(id)), _) => Click::SellAll(id.clone()),
            (Some(Cell::Chart(id, c)), _) => Click::Chart(id.clone(), *c),
            _ => Click::Nothing,
        }
    }

    /// `/market <item>`: straight to its page (back goes to its category).
    pub fn page_of(shop: &ShopConfig, market: &MarketConfig, id: &str) -> Option<Page> {
        if !market.items.contains_key(id) {
            return None;
        }
        let cats = categories(shop, market);
        let c = cats
            .iter()
            .position(|c| c.2.iter().any(|i| i == id))
            .unwrap_or(0);
        let p = cats
            .get(c)
            .and_then(|c| c.2.iter().position(|i| i == id))
            .unwrap_or(0)
            / PER_PAGE;
        Some(Page::Item(id.to_owned(), c, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configs() -> (ShopConfig, MarketConfig) {
        (ShopConfig::default(), MarketConfig::default())
    }

    #[test]
    fn categories_then_items_then_an_items_page_and_back() {
        let (shop, market) = configs();
        let mut m = Menus::default();
        m.views.insert(1, (Some(3), Page::Main));
        let cells = m.layout(&shop, &market, 1);
        assert!(matches!(&cells[10], Cell::Category(_, name, _) if name == "Farming"));
        assert_eq!(cells[NEWS], Cell::News);
        // Mining is the second category.
        assert_eq!(m.click(&shop, &market, 1, 11), Click::Show);
        let cells = m.layout(&shop, &market, 1);
        let at = cells
            .iter()
            .position(|c| *c == Cell::Item("minecraft:diamond".into()))
            .unwrap();
        assert_eq!(m.click(&shop, &market, 1, at), Click::Show);
        let cells = m.layout(&shop, &market, 1);
        assert_eq!(cells[ICON], Cell::Detail("minecraft:diamond".into()));
        assert_eq!(
            m.click(&shop, &market, 1, 21),
            Click::Buy("minecraft:diamond".into(), 64)
        );
        assert_eq!(
            m.click(&shop, &market, 1, 23),
            Click::Sell("minecraft:diamond".into(), 1)
        );
        assert_eq!(
            m.click(&shop, &market, 1, SELL_ALL),
            Click::SellAll("minecraft:diamond".into())
        );
        assert_eq!(
            m.click(&shop, &market, 1, CHART_WEEK),
            Click::Chart("minecraft:diamond".into(), Chart::Week)
        );
        assert_eq!(m.click(&shop, &market, 1, BACK), Click::Show);
        assert_eq!(m.views[&1].1, Page::Category(1, 0));
        assert_eq!(m.click(&shop, &market, 1, BACK), Click::Show);
        assert_eq!(m.views[&1].1, Page::Main);
        assert_eq!(m.click(&shop, &market, 1, CLOSE), Click::Close);
        assert_eq!(m.click(&shop, &market, 1, 0), Click::Nothing);
    }

    #[test]
    fn a_side_the_market_doesnt_trade_has_no_buttons() {
        let (shop, market) = configs();
        let mut m = Menus::default();
        m.views.insert(
            1,
            (
                Some(3),
                Menus::page_of(&shop, &market, "minecraft:bread").unwrap(),
            ),
        );
        let cells = m.layout(&shop, &market, 1);
        assert_eq!(cells[19], Cell::Buy("minecraft:bread".into(), 1));
        assert_eq!(cells[23], Cell::Filler, "the market doesn't buy bread");
        assert_eq!(cells[SELL_ALL], Cell::Filler);
        assert_eq!(
            Menus::page_of(&shop, &market, "minecraft:nether_star"),
            None
        );
    }
}
