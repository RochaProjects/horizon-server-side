// Whether the market's prices let anyone get rich by crafting: buying items,
// crafting or smelting them into others, and selling those for more than
// they cost. The cheapest way to get each item from the market is found over
// the recipes (Bellman-Ford: a recipe lowers its result's cost to what its
// inputs cost, until nothing changes); an item the market buys for more than
// that cost is a loop. Fuel is left out, so a loop is found even where the
// fuel would eat the profit. The vanilla recipes that matter for the default
// prices are written here as facts (`CHAINS`); the tests also walk every
// recipe of the game's own data when it is there. No game types. Added to
// the game as `game::smp_recipes`; also a module of this crate, for its
// tests.

use super::smp_market::Price;
use std::collections::BTreeMap;

/// A recipe: `count` of `result` from the ingredients, each a choice of
/// items (any one of them) and how many of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub result: String,
    pub count: u32,
    pub inputs: Vec<(Vec<String>, u32)>,
}

/// An item the market buys for more than it costs to make from what the
/// market sells.
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub item: String,
    /// What the market pays for one.
    pub sell: i64,
    /// The least one costs, bought or crafted.
    pub cost: f64,
}

/// A written recipe: result, count, [(ingredient, how many)].
type Chain = (&'static str, u32, &'static [(&'static str, u32)]);

/// The compression and food chains around the default prices.
const CHAINS: &[Chain] = &[
    ("iron_block", 1, &[("iron_ingot", 9)]),
    ("iron_ingot", 9, &[("iron_block", 1)]),
    ("iron_nugget", 9, &[("iron_ingot", 1)]),
    ("iron_ingot", 1, &[("iron_nugget", 9)]),
    ("iron_ingot", 1, &[("raw_iron", 1)]),
    ("raw_iron_block", 1, &[("raw_iron", 9)]),
    ("raw_iron", 9, &[("raw_iron_block", 1)]),
    ("gold_block", 1, &[("gold_ingot", 9)]),
    ("gold_ingot", 9, &[("gold_block", 1)]),
    ("gold_nugget", 9, &[("gold_ingot", 1)]),
    ("gold_ingot", 1, &[("gold_nugget", 9)]),
    ("gold_ingot", 1, &[("raw_gold", 1)]),
    ("copper_block", 1, &[("copper_ingot", 9)]),
    ("copper_ingot", 9, &[("copper_block", 1)]),
    ("copper_ingot", 1, &[("raw_copper", 1)]),
    ("diamond_block", 1, &[("diamond", 9)]),
    ("diamond", 9, &[("diamond_block", 1)]),
    ("emerald_block", 1, &[("emerald", 9)]),
    ("emerald", 9, &[("emerald_block", 1)]),
    ("coal_block", 1, &[("coal", 9)]),
    ("coal", 9, &[("coal_block", 1)]),
    ("redstone_block", 1, &[("redstone", 9)]),
    ("redstone", 9, &[("redstone_block", 1)]),
    ("lapis_block", 1, &[("lapis_lazuli", 9)]),
    ("lapis_lazuli", 9, &[("lapis_block", 1)]),
    ("quartz_block", 1, &[("quartz", 4)]),
    ("hay_block", 1, &[("wheat", 9)]),
    ("wheat", 9, &[("hay_block", 1)]),
    ("bread", 1, &[("wheat", 3)]),
    ("melon", 1, &[("melon_slice", 9)]),
    ("melon_seeds", 1, &[("melon_slice", 1)]),
    ("pumpkin_seeds", 4, &[("pumpkin", 1)]),
    ("sugar", 1, &[("sugar_cane", 1)]),
    ("paper", 3, &[("sugar_cane", 3)]),
    ("bone_meal", 3, &[("bone", 1)]),
    ("bone_block", 1, &[("bone_meal", 9)]),
    ("bone_meal", 9, &[("bone_block", 1)]),
    ("slime_block", 1, &[("slime_ball", 9)]),
    ("slime_ball", 9, &[("slime_block", 1)]),
    ("white_wool", 1, &[("string", 4)]),
    ("dried_kelp_block", 1, &[("dried_kelp", 9)]),
    ("oak_planks", 4, &[("oak_log", 1)]),
    ("spruce_planks", 4, &[("spruce_log", 1)]),
    ("birch_planks", 4, &[("birch_log", 1)]),
    ("stick", 4, &[("oak_planks", 2)]),
    ("torch", 4, &[("coal", 1), ("stick", 1)]),
    ("charcoal", 1, &[("oak_log", 1)]),
    ("stone", 1, &[("cobblestone", 1)]),
    ("stone_bricks", 4, &[("stone", 4)]),
    ("glass", 1, &[("sand", 1)]),
    ("cooked_beef", 1, &[("beef", 1)]),
    ("cooked_porkchop", 1, &[("porkchop", 1)]),
    ("cooked_chicken", 1, &[("chicken", 1)]),
    ("baked_potato", 1, &[("potato", 1)]),
    ("golden_carrot", 1, &[("carrot", 1), ("gold_nugget", 8)]),
    ("blaze_powder", 2, &[("blaze_rod", 1)]),
    ("leather", 1, &[("rabbit_hide", 4)]),
    ("tnt", 1, &[("gunpowder", 5), ("sand", 4)]),
];

/// `CHAINS` as recipes.
pub fn chains() -> Vec<Recipe> {
    CHAINS
        .iter()
        .map(|(result, count, inputs)| Recipe {
            result: format!("minecraft:{result}"),
            count: *count,
            inputs: inputs
                .iter()
                .map(|(i, n)| (vec![format!("minecraft:{i}")], *n))
                .collect(),
        })
        .collect()
}

/// The least each item costs from the market: bought, or crafted from what
/// the market sells (absent: no way at all).
pub fn costs(prices: &BTreeMap<String, Price>, recipes: &[Recipe]) -> BTreeMap<String, f64> {
    let mut cost: BTreeMap<String, f64> = prices
        .iter()
        .filter(|(_, p)| p.buy > 0)
        .map(|(id, p)| (id.clone(), p.buy as f64))
        .collect();
    // Each round can only lower a cost; as many rounds as recipes is
    // Bellman-Ford's bound, and the loop stops as soon as nothing changes.
    for _ in 0..=recipes.len() {
        let mut changed = false;
        for r in recipes {
            if r.count == 0 {
                continue;
            }
            let mut total = 0.0;
            let mut makeable = true;
            for (choices, n) in &r.inputs {
                let best = choices
                    .iter()
                    .filter_map(|c| cost.get(c))
                    .fold(f64::INFINITY, |a, &b| a.min(b));
                if !best.is_finite() {
                    makeable = false;
                    break;
                }
                total += best * f64::from(*n);
            }
            if !makeable {
                continue;
            }
            let each = total / f64::from(r.count);
            let old = cost.get(&r.result).copied().unwrap_or(f64::INFINITY);
            if each < old - 1e-9 {
                cost.insert(r.result.clone(), each);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    cost
}

/// The items the market buys for more than they cost to get from it.
pub fn loops(prices: &BTreeMap<String, Price>, recipes: &[Recipe]) -> Vec<Loop> {
    let cost = costs(prices, recipes);
    prices
        .iter()
        .filter(|(_, p)| p.sell > 0)
        .filter_map(|(id, p)| {
            let c = *cost.get(id)?;
            (p.sell as f64 > c + 1e-9).then(|| Loop {
                item: id.clone(),
                sell: p.sell,
                cost: c,
            })
        })
        .collect()
}

#[cfg(test)]
/// An ingredient of a recipe file: an item, a `#tag`, or a list of either,
/// as the items it allows (`tags` maps a tag to its items, tags in tags
/// resolved).
fn choices(v: &serde_json::Value, tags: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    match v {
        serde_json::Value::String(s) => match s.strip_prefix('#') {
            Some(tag) => tags.get(tag).cloned().unwrap_or_default(),
            None => vec![s.clone()],
        },
        serde_json::Value::Array(a) => a.iter().flat_map(|x| choices(x, tags)).collect(),
        serde_json::Value::Object(o) => o
            .get("item")
            .or_else(|| o.get("tag"))
            .map(|x| choices(x, tags))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
/// A vanilla recipe file (`data/minecraft/recipe/*.json`) as a recipe, for
/// the kinds that turn items into items: shaped and shapeless crafting,
/// transmuting, the furnaces and the campfire, the stonecutter and the
/// smithing table. None for the others (dyeing, fireworks...).
pub fn parse(json: &serde_json::Value, tags: &BTreeMap<String, Vec<String>>) -> Option<Recipe> {
    let kind = json.get("type")?.as_str()?;
    let result = json.get("result")?;
    let (result, count) = match result {
        serde_json::Value::String(s) => (s.clone(), 1),
        r => (
            r.get("id")?.as_str()?.to_owned(),
            r.get("count").and_then(|c| c.as_u64()).unwrap_or(1) as u32,
        ),
    };
    let one = |key: &str| json.get(key).map(|v| (choices(v, tags), 1));
    let inputs: Vec<(Vec<String>, u32)> = match kind.strip_prefix("minecraft:")? {
        "crafting_shaped" => {
            let key = json.get("key")?.as_object()?;
            let mut counts: BTreeMap<char, u32> = BTreeMap::new();
            for row in json.get("pattern")?.as_array()? {
                for c in row.as_str()?.chars().filter(|c| *c != ' ') {
                    *counts.entry(c).or_default() += 1;
                }
            }
            counts
                .into_iter()
                .map(|(c, n)| Some((choices(key.get(&c.to_string())?, tags), n)))
                .collect::<Option<_>>()?
        }
        "crafting_shapeless" => json
            .get("ingredients")?
            .as_array()?
            .iter()
            .map(|i| (choices(i, tags), 1))
            .collect(),
        "crafting_transmute" => vec![one("input")?, one("material")?],
        "smelting" | "blasting" | "smoking" | "campfire_cooking" | "stonecutting" => {
            vec![one("ingredient")?]
        }
        "smithing_transform" => vec![one("template")?, one("base")?, one("addition")?],
        _ => return None,
    };
    // An ingredient nothing can fill makes the recipe useless here.
    if inputs.iter().any(|(c, _)| c.is_empty()) {
        return None;
    }
    Some(Recipe {
        result,
        count,
        inputs,
    })
}

#[cfg(test)]
/// Item tags (`data/minecraft/tags/item/*.json`, by name: `logs`) resolved
/// into items, as `minecraft:logs` → items.
pub fn resolve_tags(raw: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
    fn expand(name: &str, raw: &BTreeMap<String, Vec<String>>, depth: u32, out: &mut Vec<String>) {
        let Some(values) = raw.get(name) else {
            return;
        };
        for v in values {
            match v.strip_prefix('#') {
                Some(t) if depth < 16 => expand(t, raw, depth + 1, out),
                Some(_) => {}
                None => out.push(v.clone()),
            }
        }
    }
    raw.keys()
        .map(|name| {
            let mut out = Vec::new();
            expand(name, raw, 0, &mut out);
            (name.clone(), out)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::smp_market::Config;
    use super::*;
    use std::path::PathBuf;

    fn prices(list: &[(&str, i64, i64)]) -> BTreeMap<String, Price> {
        list.iter()
            .map(|(id, buy, sell)| {
                (
                    format!("minecraft:{id}"),
                    Price {
                        buy: *buy,
                        sell: *sell,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn a_block_sold_dearer_than_its_ingots_is_a_loop() {
        // 9 ingots at 30 make a block the market would buy for 300.
        let p = prices(&[("iron_ingot", 30, 8), ("iron_block", 0, 300)]);
        let found = loops(&p, &chains());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].item, "minecraft:iron_block");
        assert!((found[0].cost - 270.0).abs() < 1e-9);
        // The other way: a cheap block uncrafted into ingots the market buys.
        let p = prices(&[("iron_ingot", 30, 8), ("iron_block", 60, 0)]);
        let found = loops(&p, &chains());
        assert_eq!(found[0].item, "minecraft:iron_ingot", "{found:?}");
    }

    #[test]
    fn bread_from_cheap_wheat_and_planks_from_cheap_logs_are_loops() {
        let p = prices(&[("wheat", 1, 0), ("bread", 5, 4)]);
        assert_eq!(loops(&p, &chains())[0].item, "minecraft:bread");
        let p = prices(&[("oak_log", 4, 1), ("oak_planks", 0, 2)]);
        assert_eq!(loops(&p, &chains())[0].item, "minecraft:oak_planks");
        // Through two recipes: logs → planks → sticks.
        let p = prices(&[("oak_log", 4, 1), ("stick", 0, 1)]);
        assert_eq!(loops(&p, &chains())[0].item, "minecraft:stick");
    }

    #[test]
    fn the_default_prices_have_no_loop() {
        let config = Config::default();
        assert_eq!(loops(&config.items, &chains()), []);
        // One price an item (spread 0), or one side closed.
        for (id, p) in &config.items {
            assert!(
                p.buy == 0 || p.sell == 0 || p.sell == p.buy,
                "{id} sells for {} and buys for {}",
                p.sell,
                p.buy
            );
        }
    }

    #[test]
    fn recipe_files_read_as_recipes() {
        let tags = resolve_tags(
            &[
                (
                    "minecraft:planks",
                    vec!["minecraft:oak_planks", "#minecraft:x"],
                ),
                ("minecraft:x", vec![]),
                (
                    "minecraft:oak_logs",
                    vec!["minecraft:oak_log", "minecraft:oak_wood"],
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.into_iter().map(str::to_owned).collect()))
            .collect(),
        );
        let shaped: serde_json::Value = serde_json::from_str(
            r##"{"type":"minecraft:crafting_shaped","key":{"#":"minecraft:iron_ingot","S":"#minecraft:planks"},
            "pattern":["# #","#S#"," # "],"result":{"id":"minecraft:bucket"}}"##,
        )
        .unwrap();
        let r = parse(&shaped, &tags).unwrap();
        assert_eq!(r.result, "minecraft:bucket");
        assert_eq!(
            r.inputs,
            [
                (vec!["minecraft:iron_ingot".to_owned()], 5),
                (vec!["minecraft:oak_planks".to_owned()], 1)
            ]
        );
        let shapeless: serde_json::Value = serde_json::from_str(
            r##"{"type":"minecraft:crafting_shapeless","ingredients":["#minecraft:oak_logs"],"result":{"count":4,"id":"minecraft:oak_planks"}}"##,
        )
        .unwrap();
        let r = parse(&shapeless, &tags).unwrap();
        assert_eq!((r.count, r.inputs[0].0.len()), (4, 2));
        let smelt: serde_json::Value = serde_json::from_str(
            r#"{"type":"minecraft:smelting","ingredient":["minecraft:a","minecraft:b"],"result":{"id":"minecraft:c"}}"#,
        )
        .unwrap();
        assert_eq!(parse(&smelt, &tags).unwrap().inputs[0].0.len(), 2);
        let special: serde_json::Value =
            serde_json::from_str(r#"{"type":"minecraft:crafting_special_repairitem"}"#).unwrap();
        assert_eq!(parse(&special, &tags), None);
    }

    /// The game's own recipe and tag files: `SMP_VANILLA_DATA` (a
    /// `data/minecraft` folder), or a Horizon checkout's prepared resources
    /// beside this repository. None when neither is there (CI).
    fn vanilla_data() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("SMP_VANILLA_DATA") {
            return Some(PathBuf::from(p));
        }
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let p = home.join("code/horizon/.local/horizon-client/26.3/data/minecraft");
        p.join("recipe").is_dir().then_some(p)
    }

    #[test]
    fn the_default_prices_have_no_loop_through_any_vanilla_recipe() {
        let Some(data) = vanilla_data() else {
            eprintln!("no vanilla data (SMP_VANILLA_DATA): only the written chains are checked");
            return;
        };
        let read = |p: &std::path::Path| -> serde_json::Value {
            serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
        };
        // Tags in folders too: `tags/item/enchantable/sword.json` is
        // `minecraft:enchantable/sword`.
        fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "json") {
                    out.push(path);
                }
            }
        }
        let root = data.join("tags/item");
        let mut files = Vec::new();
        walk(&root, &mut files);
        let mut raw = BTreeMap::new();
        for path in files {
            let name = path.strip_prefix(&root).unwrap().with_extension("");
            let values = read(&path)["values"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| {
                    v.as_str()
                        .or_else(|| v.get("id")?.as_str())
                        .map(str::to_owned)
                })
                .collect();
            raw.insert(format!("minecraft:{}", name.to_string_lossy()), values);
        }
        let tags = resolve_tags(&raw);
        let mut recipes = Vec::new();
        for entry in std::fs::read_dir(data.join("recipe")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json")
                && let Some(r) = parse(&read(&path), &tags)
            {
                recipes.push(r);
            }
        }
        assert!(recipes.len() > 1000, "{} recipes", recipes.len());
        let config = Config::default();
        assert_eq!(loops(&config.items, &recipes), []);
        // Nor once prices have moved as far as they may (0.2 to 1.6 times
        // their base): an item sold at the top while what makes it is
        // bought at the bottom, unless both share one price (a family).
        let families = super::super::smp_market::families(&config.items);
        let (lo, hi) = (config.dynamic.min_factor, config.dynamic.max_factor);
        let worst = |items: &BTreeMap<String, Price>, families: &[Vec<String>]| {
            let mut worst = Vec::new();
            for (id, p) in items.iter().filter(|(_, p)| p.sell > 0) {
                let kin = |other: &String| {
                    other == id || families.iter().any(|f| f.contains(id) && f.contains(other))
                };
                let moved: BTreeMap<String, Price> = items
                    .iter()
                    .map(|(other, q)| {
                        let f = if kin(other) { hi } else { lo };
                        // In hundredths, so rounding hides nothing.
                        let scale = |v: i64| (v as f64 * f * 100.0).round() as i64;
                        (
                            other.clone(),
                            Price {
                                buy: scale(q.buy),
                                sell: scale(q.sell),
                            },
                        )
                    })
                    .collect();
                if let Some(c) = costs(&moved, &recipes).get(id)
                    && p.sell as f64 * hi * 100.0 > c + 1e-6
                {
                    worst.push((id.clone(), p.sell as f64 * hi, *c / 100.0));
                }
            }
            worst
        };
        assert_eq!(
            worst(&config.items, &families),
            [],
            "loops once prices move"
        );
        // A gold block sold for its 9 ingots: no loop while they share one
        // price; one once the ingots drift down and the block up.
        let mut blocks = config.items.clone();
        blocks.insert("minecraft:gold_block".into(), Price { buy: 0, sell: 180 });
        assert_eq!(
            worst(&blocks, &super::super::smp_market::families(&blocks)),
            []
        );
        assert_eq!(worst(&blocks, &[])[0].0, "minecraft:gold_block");
        // The checker does find a loop through the real recipes.
        let mut cheap = config.items.clone();
        cheap.insert("minecraft:iron_block".into(), Price { buy: 0, sell: 300 });
        assert_eq!(loops(&cheap, &recipes)[0].item, "minecraft:iron_block");
    }
}
