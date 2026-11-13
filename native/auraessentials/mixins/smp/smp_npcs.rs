//! NPCs montados como cubos `item_display` dos modelos AuraNPC. Não cria
//! jogador falso, armor stand ou skin de jogador.
use super::smp_store;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

pub const COMMANDS: [&str; 1] = ["npc"];
pub const USAGES: [&str; 25] = [
    "npc",
    "npc listar",
    "npc list",
    "npc modelos",
    "npc models",
    "npc criar <id:word> <modelo:word> <nome:text>",
    "npc create <id:word> <modelo:word> <nome:text>",
    "npc remover <id:word>",
    "npc remove <id:word>",
    "npc editar <id:word> <campo:word> <valor:text>",
    "npc edit <id:word> <campo:word> <valor:text>",
    "npc editar <id:word> nome <texto:text>",
    "npc editar <id:word> modelo <modelo:word>",
    "npc editar <id:word> dimensao <dimensao:word>",
    "npc editar <id:word> pos",
    "npc editar <id:word> pos <x:word> <y:word> <z:word>",
    "npc editar <id:word> yaw <graus:word>",
    "npc editar <id:word> escala <valor:word>",
    "npc editar <id:word> hitbox <largura:word> <altura:word>",
    "npc editar <id:word> nomevisivel <true|false:word>",
    "npc editar <id:word> comando <comando:text>",
    "npc editar <id:word> comando adicionar <comando:text>",
    "npc editar <id:word> comando limpar",
    "npc editar <id:word> comando listar",
    "npc editar <id:word> comando remover <indice:integer>",
    "npc info <id:word>",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Npc {
    pub id: String,
    pub model: String,
    pub name: String,
    pub dimension: String,
    pub position: [f64; 3],
    pub yaw: f32,
    pub scale: f32,
    pub hitbox_width: f32,
    pub hitbox_height: f32,
    pub name_visible: bool,
    pub commands: Vec<String>,
}

impl Default for Npc {
    fn default() -> Self {
        Self {
            id: String::new(), model: String::new(), name: String::new(),
            dimension: "minecraft:overworld".into(), position: [0.0; 3],
            yaw: 0.0, scale: 1.0, hitbox_width: 0.8, hitbox_height: 1.95,
            name_visible: true, commands: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Data { pub npcs: BTreeMap<String, Npc> }

impl Data {
    pub fn open(dir: &Path) -> Result<Self, String> { smp_store::read_json(&dir.join("npcs.json")) }
    pub fn save(&self, dir: &Path) -> Result<(), String> { smp_store::save_json(&dir.join("npcs.json"), self) }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub id: String,
    pub key: String,
    pub data: serde_json::Value,
}

const MODEL_SOURCES: &[&str] = &[
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_ascendant_statue-d1f8854e5fdc.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_axolotl_mascot-c59f218b697e.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_cyberpunk-97e91fc6a052.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_default-e952a9eac37e.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_illager-f601b4be9350.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_pirate-7db1ee9d2d3c.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_scientist-d0ecc2c8afa4.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_trade_o_tron-fb99a4af0478.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_valkyrie-504490ff128c.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_wicked_witch-a7bffaeb6124.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_bedwa_wizard-9099dfe068a1.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_custom_server-0716a288b4d2.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_locker-aac55109d855.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_plus-e77562783a4f.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_postbox-d2609664e95a.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_questmaster-4555f8e97d83.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_region-c9fac68f1e91.json"),
    include_str!("../../assets/npc_models/aura_npc_hive_store-6ad1cd0d3153.json"),
];

pub fn models() -> Vec<Model> {
    MODEL_SOURCES.iter().filter_map(|source| {
        let data: serde_json::Value = serde_json::from_str(source).ok()?;
        let id = data.get("entity")?.as_str()?.to_owned();
        let key = safe_id(&id);
        Some(Model { id, key, data })
    }).collect()
}

pub fn model(id: &str) -> Option<Model> {
    let normalized = id.to_ascii_lowercase();
    models().into_iter().find(|model| model.id.eq_ignore_ascii_case(id) || model.key == normalized)
}

pub fn safe_id(value: &str) -> String {
    value.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect()
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

pub fn entity_uuid(id: &str, part: u32) -> u128 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in id.bytes().chain(part.to_le_bytes()) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    0xa00a_0000_0001_4000_8000_0000_0000_0000_u128 | u128::from(hash)
}
