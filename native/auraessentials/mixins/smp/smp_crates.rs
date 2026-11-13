// Caixas e chaves AuraEssentials. As cinco chaves têm IDs distintos e os
// prêmios usam a serialização NBT dos ItemStacks do Horizon para preservar
// componentes.
use super::smp_store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const COMMANDS: [&str; 1] = ["caixa"];
pub const USAGES: [&str; 22] = [
    "caixa",
    "caixa menu",
    "caixa preview <tipo:word>",
    "caixa prever <tipo:word>",
    "caixa listar",
    "caixa lista",
    "caixa definir <tipo:word>",
    "caixa definir <tipo:word> <x:integer> <y:integer> <z:integer>",
    "caixa remover",
    "caixa remover <dimensao:word> <x:integer> <y:integer> <z:integer>",
    "caixa chave <tipo:word> <jogador:word> [quantidade:integer]",
    "caixa premio <tipo:word> [peso:word]",
    "caixa addpremio <tipo:word> [peso:word]",
    "caixa addreward <tipo:word> [peso:word]",
    "caixa removerpremio <tipo:word> <id:word>",
    "caixa removereward <tipo:word> <id:word>",
    "caixa premios <tipo:word>",
    "caixa limparpremios <tipo:word>",
    "caixa animacao <tipo:word> <animacao:word>",
    "caixa efeito <tipo:word> <efeito:word>",
    "caixa set <tipo:word>",
    "caixa remove",
];


#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind { Voto, Membro, Bronze, Ouro, Diamante }

impl Kind {
    pub fn id(self) -> &'static str { match self { Self::Voto=>"voto",Self::Membro=>"membro",Self::Bronze=>"bronze",Self::Ouro=>"ouro",Self::Diamante=>"diamante" } }
    pub fn parse(s: &str) -> Option<Self> { match s.to_ascii_lowercase().as_str() { "voto"|"vote"=>Some(Self::Voto),"membro"|"member"=>Some(Self::Membro),"bronze"=>Some(Self::Bronze),"ouro"|"gold"=>Some(Self::Ouro),"diamante"|"diamond"=>Some(Self::Diamante),_=>None } }
    pub fn item(self) -> &'static str { match self { Self::Voto=>"minecraft:lime_candle",Self::Membro=>"minecraft:blue_candle",Self::Bronze=>"minecraft:orange_candle",Self::Ouro=>"minecraft:yellow_candle",Self::Diamante=>"minecraft:light_blue_candle" } }
    pub fn key_name(self) -> &'static str { match self { Self::Voto=>"Chave de Voto",Self::Membro=>"Chave de Membro",Self::Bronze=>"Chave de Bronze",Self::Ouro=>"Chave de Ouro",Self::Diamante=>"Chave de Diamante" } }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reward { pub id: String, pub item_nbt_hex: String, pub weight: f64, #[serde(default)] pub commands: Vec<String>, #[serde(default)] pub broadcast: bool }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrateBlock { pub dimension: String, pub x: i32, pub y: i32, pub z: i32, pub kind: Kind, #[serde(default)] pub key_id: Option<u32> }
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Data { pub blocks: Vec<CrateBlock>, pub rewards: BTreeMap<Kind, Vec<Reward>>, pub animations: BTreeMap<Kind,String>, pub effects: BTreeMap<Kind,String> }

impl Data {
    pub fn open(dir: &std::path::Path) -> Result<Self,String> { smp_store::read_json(&dir.join("crates.json")) }
    pub fn save(&self, dir: &std::path::Path) -> Result<(),String> { smp_store::save_json(&dir.join("crates.json"),self) }
    pub fn add_block(&mut self, block: CrateBlock) { self.blocks.retain(|b| (b.dimension.as_str(),b.x,b.y,b.z)!=(block.dimension.as_str(),block.x,block.y,block.z)); self.blocks.push(block); }
    pub fn reward(&self, kind: Kind, roll: f64) -> Option<&Reward> {
        let rewards = self.rewards.get(&kind)?;
        let total: f64 = rewards.iter().map(|r| r.weight.max(0.0)).sum();
        if rewards.is_empty() { return None; }
        if !total.is_finite() || total <= 0.0 { return None; }
        let mut cursor = roll.clamp(0.0,0.999_999_999) * total;
        for reward in rewards { cursor -= reward.weight.max(0.0); if cursor <= 0.0 { return Some(reward); } }
        rewards.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn aura_key_names_and_aliases_are_stable() {
        assert_eq!(Kind::parse("diamond"),Some(Kind::Diamante));
        assert_eq!(Kind::Ouro.key_name(),"Chave de Ouro");
        assert_eq!([Kind::Voto.id(), Kind::Membro.id(), Kind::Bronze.id(), Kind::Ouro.id(), Kind::Diamante.id()], ["voto","membro","bronze","ouro","diamante"]);
    }
}
