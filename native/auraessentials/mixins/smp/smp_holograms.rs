//! Hologramas persistentes com quebra de linha e opções de fundo por linha.
use super::smp_store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const COMMANDS: [&str; 1] = ["holograma"];
pub const USAGES: [&str; 7] = [
    "holograma listar",
    "holograma criar <id:word> <texto:text>",
    "holograma editar <id:word> <texto:text>",
    "holograma mover <id:word>",
    "holograma remover <id:word>",
    "holograma fundo <id:word> <modo:word>",
    "holograma largura <id:word> <px:integer>",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Hologram {
    pub id: String,
    pub dimension: String,
    pub position: [f64; 3],
    pub title: String,
    pub description: String,
    pub split_background: bool,
    pub line_width: u16,
    pub wrap_chars: u16,
    pub excluded_players: Vec<String>,
}
impl Default for Hologram {
    fn default() -> Self { Self { id:String::new(),dimension:"minecraft:overworld".into(),position:[0.0,65.2,0.0],title:String::new(),description:String::new(),split_background:false,line_width:220,wrap_chars:0,excluded_players:Vec::new() } }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Data { pub holograms: BTreeMap<String,Hologram>, pub background_enabled: bool }
impl Data {
    pub fn open(dir:&std::path::Path)->Result<Self,String> { smp_store::read_json(&dir.join("holograms.json")) }
    pub fn save(&self,dir:&std::path::Path)->Result<(),String> { smp_store::save_json(&dir.join("holograms.json"),self) }
    pub fn wrap(text:&str, limit:usize)->Vec<String> {
        if limit==0 { return text.split('\n').map(str::to_owned).collect(); }
        let mut lines=Vec::new();
        for explicit in text.split('\n') {
            let mut line=String::new();
            for word in explicit.split_whitespace() {
                if !line.is_empty() && line.chars().count()+1+word.chars().count()>limit { lines.push(std::mem::take(&mut line)); }
                if !line.is_empty() { line.push(' '); }
                line.push_str(word);
            }
            lines.push(line);
        }
        lines
    }

    pub fn lines(hologram: &Hologram) -> Vec<String> {
        let text = if hologram.description.is_empty() {
            hologram.title.clone()
        } else if hologram.title.is_empty() {
            hologram.description.clone()
        } else {
            format!("{}\n{}", hologram.title, hologram.description)
        };
        Self::wrap(&text, usize::from(hologram.wrap_chars))
    }
}

#[cfg(test)]
mod tests { use super::*; #[test] fn preserves_explicit_and_automatic_line_breaks() { assert_eq!(Data::wrap("um dois tres\nquatro",7),["um dois","tres","quatro"]); } }
