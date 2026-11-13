//! Pacote de recursos Java do AuraEssentials. URL e SHA ficam num JSON editável
//! na pasta de dados do mod; trocar o pack não exige recompilar o engine.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub url: String,
    /// O protocolo Java do Minecraft usa SHA-1 (40 caracteres hexadecimais).
    pub sha1: String,
    pub required: bool,
    pub prompt: String,
    pub additional_packs: Vec<AdditionalPack>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdditionalPack {
    pub id: String,
    pub url: String,
    pub sha1: String,
    pub required: bool,
    pub prompt: String,
}

impl Default for AdditionalPack {
    fn default() -> Self {
        Self {
            id: "aura_npc".into(),
            url: "https://gitlab.com/rochaproject/archive/-/raw/main/auraessentials/AuraNPC.zip?ref_type=heads".into(),
            sha1: "5ea0e0f8252d6c4f62097cc97bac447749b44032".into(),
            required: false,
            prompt: "Pacote de modelos AuraNPC".into(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            url: "https://github.com/RochaProjects/AuraEssentials/releases/latest/download/kingdoms-glyph-pack-1.21.10.zip".into(),
            sha1: "707aaf51bfaeed6bbeee44e58fb66a176de8c439".into(),
            required: true,
            prompt: "AuraEssentials".into(),
            additional_packs: vec![AdditionalPack::default()],
        }
    }
}

/// UUID fixo: uma atualização do pack substitui a versão anterior no cliente.
pub const PACK_ID: u128 = 0xa0a0_e55e_0000_4000_8000_0000_0001;

pub fn additional_pack_id(name: &str) -> u128 {
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    0xa0a0_e55e_0001_0000_8000_0000_0000 | u128::from(hash)
}

pub fn read(dir: &Path) -> Result<Config, String> {
    let path = dir.join("resource-pack.json");
    let exists = path.is_file();
    let mut config = super::smp_store::read_json::<Config>(&path)?;
    config.url = config.url.trim().to_owned();
    config.sha1 = config.sha1.trim().to_ascii_lowercase();
    config.prompt = config.prompt.trim().to_owned();
    if !exists {
        super::smp_store::write_json(&path, &config)?;
    }
    if config.enabled {
        validate_pack(&config.url, &config.sha1, "pacote principal")?;
    }
    for pack in &mut config.additional_packs {
        pack.url = pack.url.trim().to_owned();
        pack.sha1 = pack.sha1.trim().to_ascii_lowercase();
        pack.id = pack.id.trim().to_ascii_lowercase();
        if !pack.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            || pack.id.is_empty()
        {
            return Err("resource-pack.json: id de pacote adicional deve usar letras, números, _ ou -".into());
        }
        validate_pack(&pack.url, &pack.sha1, &pack.id)?;
    }
    Ok(config)
}

fn validate_pack(url: &str, sha1: &str, label: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("resource-pack.json: use URL HTTPS em {label}"));
    }
    if !valid_sha1(sha1) {
        return Err(format!("resource-pack.json: SHA-1 inválido em {label}; informe 40 caracteres hexadecimais"));
    }
    Ok(())
}

pub fn valid_sha1(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
