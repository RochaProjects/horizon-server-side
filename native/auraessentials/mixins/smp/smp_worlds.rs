// Dimension management for Horizon 26.x worlds and safe player-record resets.
use super::{smp_economy::Out, smp_store};
use crate::game::{Game, native_server::NativeServerPlayer};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const COMMANDS: [&str; 1] = ["mundo"];
pub const USAGES: [&str; 16] = [
    "mundo",
    "mundo listar",
    "mundo criar <nome:word>",
    "mundo info <nome:word>",
    "mundo tp <nome:word>",
    "mundo spawn [nome:word]",
    "mundo setspawn",
    "mundo principal <nome:word>",
    "mundo principal limpar",
    "mundo descarregar <nome:word>",
    "mundo carregar <nome:word>",
    "mundo renomear <atual:word> <novo:word> confirmar",
    "mundo deletar <nome:word> confirmar [backup:word]",
    "mundo resetar <nome:word> confirmar [backup:word]",
    "mundo dados <tipo:word> [jogador:word] [confirmar] [backup:word]",
    "mundo gamerule <dimensao:word> <regra:word> [valor:word]",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Registry {
    primary: Option<String>,
    worlds: BTreeMap<String, WorldRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct WorldRecord {
    name: String,
    spawn: Option<[f64; 3]>,
    created_at: u64,
}
impl Default for WorldRecord {
    fn default() -> Self {
        Self { name: String::new(), spawn: None, created_at: 0 }
    }
}

fn registry_path(world: &Path) -> PathBuf {
    world.join(smp_store::DIR).join("worlds.json")
}
fn load_registry(world: &Path) -> Result<Registry, String> {
    smp_store::read_json(&registry_path(world))
}
fn save_registry(world: &Path, registry: &Registry) -> Result<(), String> {
    smp_store::write_json(&registry_path(world), registry)
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        && !name.contains("..")
}
fn dimension(name: &str) -> String {
    format!("auraessentials:{name}")
}
fn dimension_path(world: &Path, name: &str) -> PathBuf {
    world.join("dimensions").join("auraessentials").join(name)
}
fn safe_dimension_path(world: &Path, name: &str) -> Option<PathBuf> {
    let path = dimension_path(world, name);
    for component in [world.join("dimensions"), world.join("dimensions/auraessentials"), path.clone()] {
        if fs::symlink_metadata(component).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return None;
        }
    }
    Some(path)
}
fn known_names(world: &Path, registry: &Registry) -> Vec<String> {
    let mut names: Vec<String> = registry.worlds.values().map(|w| w.name.clone()).collect();
    if let Ok(entries) = fs::read_dir(world.join("dimensions/auraessentials")) {
        for entry in entries.flatten().filter(|e| e.path().is_dir()) {
            if let Some(name) = entry.file_name().to_str()
                && valid_name(name)
                && !names.iter().any(|n| n.eq_ignore_ascii_case(name))
            {
                names.push(name.to_owned());
            }
        }
    }
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names
}
fn parse_backup(arg: Option<&str>) -> Result<bool, String> {
    match arg.map(str::to_ascii_lowercase).as_deref() {
        None | Some("nao") | Some("não") | Some("false") => Ok(false),
        Some("backup") | Some("sim") | Some("true") => Ok(true),
        _ => Err("Use `backup` para guardar uma cópia antes da operação.".into()),
    }
}
fn backup_path(world: &Path, name: &str) -> PathBuf {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    world.join("backups/auraessentials").join(format!("{name}-{stamp}"))
}
fn backup_player_file(world: &Path, folder: &str, uuid: &str) -> Result<(), String> {
    let source = safe_player_file(world, folder, uuid)?;
    if !source.exists() { return Ok(()); }
    if fs::symlink_metadata(&source).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(format!("Backup recusado: {} é um link simbólico.", source.display()));
    }
    let target = backup_path(world, "playerdata").join(folder).join(format!("{uuid}.json"));
    if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    fs::copy(&source, &target).map(|_| ()).map_err(|e| format!("Backup falhou para {}: {e}", source.display()))
}
fn safe_player_file(world: &Path, folder: &str, uuid: &str) -> Result<PathBuf, String> {
    let base = world.join("players");
    let dir = base.join(folder);
    let file = dir.join(format!("{uuid}.json"));
    for path in [&base, &dir, &file] {
        if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(format!("Operação recusada: {} é um link simbólico.", path.display()));
        }
    }
    Ok(file)
}
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "backup source contains a symbolic link"));
        }
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
fn ensure_idle(game: &mut Game<'_>, players: &[NativeServerPlayer], name: &str) -> Result<(), String> {
    let id = dimension(name);
    if players.iter().any(|p| p.dimension == id) {
        return Err("Saia dessa dimensão com todos os jogadores antes de alterar ou apagar os dados.".into());
    }
    if !game.native_unload_dimension(&id) {
        return Err("A dimensão ainda está em uso. Aguarde o descarregamento automático e tente novamente.".into());
    }
    Ok(())
}
fn say(s: impl Into<String>) -> Out { Out::Reply(s.into()) }
fn fail(s: impl Into<String>) -> Out { Out::Error(s.into()) }

pub fn command(
    game: &mut Game<'_>,
    actor: &NativeServerPlayer,
    players: &[NativeServerPlayer],
    line: &str,
) -> Vec<Out> {
    if actor.permission < 3 {
        return vec![fail("Você não tem permissão para gerenciar dimensões.")];
    }
    let Some(world) = game.native_world_dir() else { return vec![fail("Nenhum mundo está aberto.")] };
    let args: Vec<&str> = line.split_whitespace().collect();
    let mut registry = match load_registry(&world) {
        Ok(v) => v,
        Err(e) => return vec![fail(format!("Não foi possível ler worlds.json: {e}"))],
    };
    match args.get(1).copied().unwrap_or("ajuda") {
        "ajuda" => vec![say("/mundo listar | criar <nome> | info <nome> | tp <nome> | spawn [nome] | setspawn | principal <nome|limpar> | descarregar/carregar <nome> | renomear <atual> <novo> confirmar | resetar/deletar <nome> confirmar [backup] | dados <estatisticas|conquistas|tudo> [jogador|todos] [confirmar] [backup] | gamerule <dimensão-customizada> <regra> [valor|global]")],
        "listar" | "list" => {
            let names = known_names(&world, &registry);
            if names.is_empty() { return vec![say("Ainda não há dimensões AuraEssentials.")]; }
            vec![say(format!("Dimensões: {}{}", names.iter().map(|n| format!("{}{}", n, if registry.primary.as_deref() == Some(n.as_str()) { " (principal)" } else { "" })).collect::<Vec<_>>().join(", "), if names.iter().any(|n| dimension(n) == actor.dimension) { " · você está em uma delas" } else { "" }))]
        }
        "criar" | "create" => {
            let Some(name) = args.get(2).copied().map(str::to_ascii_lowercase) else { return vec![fail("Uso: /mundo criar <nome>")]; };
            if !valid_name(&name) { return vec![fail("Nome inválido. Use até 40 letras minúsculas, números, _ ou -.")]; }
            let Some(path) = safe_dimension_path(&world, &name) else { return vec![fail("O caminho da dimensão contém um link simbólico e foi recusado.")]; };
            if path.exists() { return vec![fail("Essa dimensão já existe.")]; }
            if let Err(e) = fs::create_dir_all(&path) { return vec![fail(format!("Não foi possível criar a dimensão: {e}"))]; }
            registry.worlds.insert(name.clone(), WorldRecord { name: name.clone(), created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(), ..Default::default() });
            if let Err(e) = save_registry(&world, &registry) {
                let _ = fs::remove_dir_all(&path);
                return vec![fail(format!("Não foi possível registrar a dimensão; criação desfeita: {e}"))];
            }
            vec![say(format!("Dimensão {name} criada. Use /mundo tp {name} para abri-la; o engine gera os chunks ao entrar."))]
        }
        "info" => {
            let Some(name) = args.get(2).copied().map(str::to_ascii_lowercase) else { return vec![fail("Uso: /mundo info <nome>")]; };
            if !valid_name(&name) || !safe_dimension_path(&world, &name).is_some_and(|p| p.is_dir()) { return vec![fail("Dimensão não encontrada.")]; }
            let record = registry.worlds.get(&name).cloned().unwrap_or_default();
            vec![say(format!("{} · {} · spawn {}", name, dimension(&name), record.spawn.map(|p| format!("{:.1} {:.1} {:.1}", p[0], p[1], p[2])).unwrap_or_else(|| "0.5 64 0.5 (padrão)".into())))]
        }
        "tp" | "teleportar" => {
            let Some(name) = args.get(2).copied().map(str::to_ascii_lowercase) else { return vec![fail("Uso: /mundo tp <nome>")]; };
            if !valid_name(&name) || !safe_dimension_path(&world, &name).is_some_and(|p| p.is_dir()) { return vec![fail("Dimensão não encontrada. Crie-a com /mundo criar <nome>.")]; }
            let id = dimension(&name);
            let pos = registry.worlds.get(&name).and_then(|r| r.spawn).unwrap_or([0.5, 64.0, 0.5]);
            if game.native_teleport_player(actor.uuid, &id, pos, actor.yaw, actor.pitch) { vec![say(format!("Indo para {name}…"))] } else { vec![fail("O Horizon não aceitou o teleporte para essa dimensão.")] }
        }
        "spawn" => {
            let name = args.get(2).copied().map(str::to_ascii_lowercase).or_else(|| registry.primary.clone());
            let Some(name) = name else { return vec![fail("Informe uma dimensão ou defina a principal.")]; };
            if !valid_name(&name) || !safe_dimension_path(&world, &name).is_some_and(|p| p.is_dir()) { return vec![fail("Dimensão não encontrada.")]; }
            let pos = registry.worlds.get(&name).and_then(|r| r.spawn).unwrap_or([0.5, 64.0, 0.5]);
            if game.native_teleport_player(actor.uuid, &dimension(&name), pos, actor.yaw, actor.pitch) { vec![say(format!("Indo ao spawn de {name}…"))] } else { vec![fail("Não foi possível teleportar para essa dimensão.")] }
        }
        "setspawn" | "definirspawn" => {
            let Some(name) = actor.dimension.strip_prefix("auraessentials:").map(str::to_owned) else { return vec![fail("Você precisa estar em uma dimensão AuraEssentials.")]; };
            if !valid_name(&name) || !safe_dimension_path(&world, &name).is_some_and(|p| p.is_dir()) { return vec![fail("ID da dimensão inválido ou diretório ausente.")]; }
            let record = registry.worlds.entry(name.clone()).or_insert_with(|| WorldRecord { name: name.clone(), ..Default::default() });
            record.spawn = Some(actor.position);
            if let Err(e) = save_registry(&world, &registry) { return vec![fail(format!("Não foi possível salvar o spawn: {e}"))]; }
            vec![say(format!("Spawn de {name} definido na sua posição."))]
        }
        "principal" | "setprimary" => {
            let Some(name) = args.get(2).copied() else { return vec![fail("Uso: /mundo principal <nome|limpar>")]; };
            if name == "limpar" { registry.primary = None; }
            else {
                let name = name.to_ascii_lowercase();
                if !valid_name(&name) || !safe_dimension_path(&world, &name).is_some_and(|p| p.is_dir()) { return vec![fail("Dimensão não encontrada.")]; }
                registry.primary = Some(name.clone());
            }
            if let Err(e) = save_registry(&world, &registry) { return vec![fail(format!("Não foi possível salvar worlds.json: {e}"))]; }
            vec![say("Dimensão principal atualizada.")]
        }
        "descarregar" | "unload" => {
            let Some(name) = args.get(2).copied().map(str::to_ascii_lowercase) else { return vec![fail("Uso: /mundo descarregar <nome>")]; };
            if !valid_name(&name) || !safe_dimension_path(&world, &name).is_some_and(|p| p.is_dir()) || !game.native_unload_dimension(&dimension(&name)) { return vec![fail("Não foi possível descarregar: a dimensão está ativa ou não existe.")]; }
            vec![say(format!("Dimensão {name} salva e descarregada."))]
        }
        "carregar" | "load" => {
            let Some(name) = args.get(2).copied().map(str::to_ascii_lowercase) else { return vec![fail("Uso: /mundo carregar <nome>")]; };
            if !valid_name(&name) || !safe_dimension_path(&world, &name).is_some_and(|p| p.is_dir()) { return vec![fail("Dimensão não encontrada.")]; }
            let pos = registry.worlds.get(&name).and_then(|r| r.spawn).unwrap_or([0.5, 64.0, 0.5]);
            if game.native_teleport_player(actor.uuid, &dimension(&name), pos, actor.yaw, actor.pitch) { vec![say(format!("Dimensão {name} carregada; você foi enviado ao spawn."))] } else { vec![fail("Não foi possível carregar a dimensão.")] }
        }
        "renomear" | "rename" => {
            let (Some(old), Some(new)) = (args.get(2).copied().map(str::to_ascii_lowercase), args.get(3).copied().map(str::to_ascii_lowercase)) else { return vec![fail("Uso: /mundo renomear <atual> <novo> confirmar")]; };
            if args.get(4) != Some(&"confirmar") { return vec![fail("Escreva confirmar para renomear a dimensão.")]; }
            let (Some(old_path), Some(new_path)) = (safe_dimension_path(&world, &old), safe_dimension_path(&world, &new)) else { return vec![fail("O caminho da dimensão contém um link simbólico e foi recusado.")]; };
            if !valid_name(&old) || !valid_name(&new) || !old_path.is_dir() || new_path.exists() { return vec![fail("Nome inválido, dimensão inexistente ou nome novo já usado.")]; }
            if let Err(e) = ensure_idle(game, players, &old) { return vec![fail(e)]; }
            if let Err(e) = fs::rename(&old_path, &new_path) { return vec![fail(format!("Não foi possível renomear: {e}"))]; }
            if let Some(mut record) = registry.worlds.remove(&old) { record.name = new.clone(); registry.worlds.insert(new.clone(), record); }
            let old_id = dimension(&old);
            let new_id = dimension(&new);
            if !game.native_dimension_rename_game_rule_overrides(&old_id, &new_id) {
                return vec![fail("Dimensão renomeada, mas os overrides de gamerule não foram salvos.")];
            }
            if registry.primary.as_deref() == Some(old.as_str()) { registry.primary = Some(new.clone()); }
            if let Err(e) = save_registry(&world, &registry) { return vec![fail(format!("Dimensão renomeada, mas worlds.json não foi atualizado: {e}"))]; }
            vec![say(format!("Dimensão {old} renomeada para {new}."))]
        }
        "resetar" | "reset" | "deletar" | "delete" => {
            let deleting = matches!(args[1], "deletar" | "delete");
            let Some(name) = args.get(2).copied().map(str::to_ascii_lowercase) else { return vec![fail("Informe o nome e escreva confirmar para prosseguir.")]; };
            if args.get(3) != Some(&"confirmar") { return vec![fail("Operação destrutiva: use `confirmar` no final do comando.")]; }
            let backup = match parse_backup(args.get(4).copied()) { Ok(v) => v, Err(e) => return vec![fail(e)] };
            let Some(path) = safe_dimension_path(&world, &name) else { return vec![fail("O caminho da dimensão contém um link simbólico e foi recusado.")]; };
            if !valid_name(&name) || !path.is_dir() { return vec![fail("Dimensão não encontrada.")]; }
            if let Err(e) = ensure_idle(game, players, &name) { return vec![fail(e)]; }
            if backup {
                let dest = backup_path(&world, &name);
                if let Err(e) = copy_dir(&path, &dest) { return vec![fail(format!("Backup falhou; dimensão preservada: {e}"))]; }
            }
            if let Err(e) = fs::remove_dir_all(&path) { return vec![fail(format!("Não foi possível remover os dados: {e}"))]; }
            if deleting {
                registry.worlds.remove(&name);
                let _ = game.native_dimension_clear_game_rule_overrides(&dimension(&name));
                if registry.primary.as_deref() == Some(name.as_str()) { registry.primary = None; }
                if let Err(e) = save_registry(&world, &registry) { return vec![fail(format!("Dados removidos, mas worlds.json não foi atualizado: {e}"))]; }
                vec![say(format!("Dimensão {name} apagada{}.", if backup { "; backup guardado em backups/auraessentials" } else { "" }))]
            } else {
                if let Err(e) = fs::create_dir_all(&path) { return vec![fail(format!("Dados limpos, mas não foi possível recriar a dimensão: {e}"))]; }
                vec![say(format!("Dados e chunks de {name} resetados{}.", if backup { "; backup guardado em backups/auraessentials" } else { "" }))]
            }
        }
        "dados" | "data" => reset_records(game, actor, players, &world, &args[2..]),
        "gamerule" | "gr" => {
            let (Some(id), Some(rule)) = (args.get(2).copied(), args.get(3).copied()) else {
                return vec![fail("Uso: /mundo gamerule <dimensao-customizada> <regra> [valor|global]")];
            };
            if id.starts_with("minecraft:") {
                return vec![fail("Esse comando é para dimensões customizadas; use /gamerule para as regras globais do servidor.")];
            }
            if let Some(value) = args.get(4).copied() {
                if matches!(rule, "advance_time" | "advance_weather") && value != "global" {
                    return vec![fail("Esse override não pode ser por dimensão: o Horizon ainda compartilha o relógio e o clima entre dimensões.")];
                }
                let parsed = if value == "global" { None } else {
                    let Some(current) = game.native_dimension_game_rule(id, rule) else { return vec![fail("Dimensão ou gamerule não encontrada.")]; };
                    match current {
                        horizon_client::mod_api::GameRuleValue::Bool(_) => match value.parse::<bool>() {
                            Ok(v) => Some(horizon_client::mod_api::GameRuleValue::Bool(v)),
                            Err(_) => return vec![fail("Valor inválido: use true/false ou global.")],
                        },
                        horizon_client::mod_api::GameRuleValue::Int(_) => match value.parse::<i32>() {
                            Ok(v) => Some(horizon_client::mod_api::GameRuleValue::Int(v)),
                            Err(_) => return vec![fail("Valor inválido: use um número inteiro ou global.")],
                        },
                    }
                };
                if !game.native_dimension_set_game_rule(id, rule, parsed) {
                    return vec![fail("Não foi possível salvar o override; confira dimensão, gamerule, suporte por dimensão e faixa permitida.")];
                }
                let shown = game.native_dimension_game_rule(id, rule).map(|v| v.to_string()).unwrap_or_else(|| "global".into());
                vec![say(format!("[{id}] {rule} = {shown}."))]
            } else {
                let Some(value) = game.native_dimension_game_rule(id, rule) else { return vec![fail("Dimensão ou gamerule não encontrada.")]; };
                let source = if game.native_dimension_has_game_rule_override(id, rule) { "override" } else { "global" };
                vec![say(format!("[{id}] {rule} = {value} ({source})."))]
            }
        }
        _ => vec![fail("Subcomando desconhecido. Use /mundo para ver a ajuda.")],
    }
}

fn reset_records(game: &mut Game<'_>, actor: &NativeServerPlayer, players: &[NativeServerPlayer], world: &Path, args: &[&str]) -> Vec<Out> {
    let Some(kind) = args.first().copied() else { return vec![fail("Uso: /mundo dados <estatisticas|conquistas|tudo> [jogador]")]; };
    let backup = match parse_backup(args.last().copied().filter(|s| *s == "backup")) { Ok(v) => v, Err(e) => return vec![fail(e)] };
    let target = args.get(1).copied().filter(|s| *s != "backup");
    let stats = matches!(kind, "estatisticas" | "estatísticas" | "statistics" | "stats" | "tudo");
    let advancements = matches!(kind, "conquistas" | "advancements" | "tudo");
    if !stats && !advancements { return vec![fail("Tipo inválido. Use estatisticas, conquistas ou tudo.")]; }
    if target.is_some_and(|t| matches!(t, "todos" | "all")) {
        if args.get(2) != Some(&"confirmar") && args.get(1) != Some(&"confirmar") {
            return vec![fail("Reset global destrutivo: use `/mundo dados <tipo> todos confirmar [backup]`.")];
        }
        if !players.iter().any(|p| p.uuid == actor.uuid) {
            return vec![fail("O Horizon não disponibilizou a lista completa de jogadores neste comando; o reset global foi cancelado.")];
        }
        if players.iter().any(|p| p.uuid != actor.uuid) { return vec![fail("Para resetar todos, desconecte os outros jogadores primeiro; os dados deles podem estar em memória.")]; }
        let backup_root = backup.then(|| backup_path(world, "todos-jogadores"));
        if let Some(backup_root) = &backup_root {
            if stats {
                let Some(data) = game.native_player_stats(actor.uuid) else { return vec![fail("Não foi possível salvar no backup as estatísticas ainda em memória.")]; };
                if let Err(e) = write_record_backup(backup_root, "stats", actor.uuid, &data) { return vec![fail(e)]; }
            }
            if advancements {
                let Some(data) = game.native_player_advancements(actor.uuid) else { return vec![fail("Não foi possível salvar no backup as conquistas ainda em memória.")]; };
                if let Err(e) = write_record_backup(backup_root, "advancements", actor.uuid, &data) { return vec![fail(e)]; }
            }
        }
        for folder in [if stats { Some("stats") } else { None }, if advancements { Some("advancements") } else { None }].into_iter().flatten() {
            let dir = world.join("players").join(folder);
            let player_root = world.join("players");
            if [&player_root, &dir].iter().any(|p| fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())) {
                return vec![fail(format!("Reset recusado: {} contém um link simbólico.", dir.display()))];
            }
            if let Some(dest) = &backup_root {
                if dir.exists() { if let Err(e) = copy_dir(&dir, &dest.join(folder)) { return vec![fail(format!("Backup falhou; nenhum arquivo foi apagado: {e}"))]; } }
            }
            if dir.exists() {
                let Ok(entries) = fs::read_dir(&dir) else { return vec![fail(format!("Não foi possível ler {}.", dir.display()))]; };
                for entry in entries.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")) {
                    if entry.file_type().is_ok_and(|t| t.is_symlink()) { return vec![fail(format!("Reset recusado: {} é um link simbólico.", entry.path().display()))]; }
                    if let Err(e) = fs::remove_file(entry.path()) { return vec![fail(format!("Não foi possível apagar {}: {e}", entry.path().display()))]; }
                }
            }
        }
        if stats && !game.native_set_player_stats(actor.uuid, &serde_json::json!({"stats": {}, "DataVersion": horizon_world::DATA_VERSION})) { return vec![fail("Não foi possível limpar as estatísticas do jogador conectado.")]; }
        if advancements && !game.native_set_player_advancements(actor.uuid, &serde_json::json!({"DataVersion": horizon_world::DATA_VERSION})) { return vec![fail("Não foi possível limpar as conquistas do jogador conectado.")]; }
        return vec![say(format!("Dados {} resetados para todos os jogadores{}.", if stats && advancements { "de estatísticas e conquistas" } else if stats { "de estatísticas" } else { "de conquistas" }, if backup { "; backup guardado em backups/auraessentials" } else { "" }))];
    }
    let target_uuid = if let Some(target) = target {
        if let Some(p) = players.iter().find(|p| p.name.eq_ignore_ascii_case(target)) { p.uuid }
        else if let Some(id) = parse_uuid(target) { id }
        else if let Some(id) = find_usercache(world, target) { id }
        else { return vec![fail("Jogador não encontrado por nome ou UUID.")] }
    } else { actor.uuid };
    let online = players.iter().any(|p| p.uuid == target_uuid);
    if online && target_uuid != actor.uuid {
        return vec![fail("Por segurança, dados de outro jogador online não podem ser apagados. Peça para ele sair e tente novamente.")];
    }
    if target_uuid == actor.uuid {
        if backup {
            let backup_root = backup_path(world, "playerdata");
            if stats {
                let Some(data) = game.native_player_stats(actor.uuid) else { return vec![fail("Não foi possível ler as estatísticas atuais para o backup.")]; };
                if let Err(e) = write_record_backup(&backup_root, "stats", actor.uuid, &data) { return vec![fail(e)]; }
            }
            if advancements {
                let Some(data) = game.native_player_advancements(actor.uuid) else { return vec![fail("Não foi possível ler as conquistas atuais para o backup.")]; };
                if let Err(e) = write_record_backup(&backup_root, "advancements", actor.uuid, &data) { return vec![fail(e)]; }
            }
        }
        if stats && !game.native_set_player_stats(actor.uuid, &serde_json::json!({"stats": {}, "DataVersion": horizon_world::DATA_VERSION})) { return vec![fail("Não foi possível limpar as estatísticas.")]; }
        if advancements && !game.native_set_player_advancements(actor.uuid, &serde_json::json!({"DataVersion": horizon_world::DATA_VERSION})) { return vec![fail("Não foi possível limpar as conquistas.")]; }
    } else {
        let uuid = uuid_string(target_uuid);
        if stats { if backup { if let Err(e) = backup_player_file(world, "stats", &uuid) { return vec![fail(e)]; } } if let Err(e) = remove_player_file(world, "stats", &uuid) { return vec![fail(e)]; } }
        if advancements { if backup { if let Err(e) = backup_player_file(world, "advancements", &uuid) { return vec![fail(e)]; } } if let Err(e) = remove_player_file(world, "advancements", &uuid) { return vec![fail(e)]; } }
    }
    vec![say(format!("Dados {} resetados para {}.", match (stats, advancements) { (true, true) => "de estatísticas e conquistas", (true, false) => "de estatísticas", _ => "de conquistas" }, if target_uuid == actor.uuid { "você" } else { target.unwrap_or("UUID informado") }))]
}

fn write_record_backup(root: &Path, folder: &str, uuid: u128, value: &serde_json::Value) -> Result<(), String> {
    let path = root.join(folder).join(format!("{}.json", uuid_string(uuid)));
    smp_store::write_json(&path, value).map_err(|e| format!("Backup falhou: {e}"))
}

fn remove_player_file(world: &Path, folder: &str, uuid: &str) -> Result<(), String> {
    let path = safe_player_file(world, folder, uuid)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Não foi possível remover {}: {e}", path.display())),
    }
}
fn parse_uuid(value: &str) -> Option<u128> {
    let compact: String = value.chars().filter(|c| *c != '-').collect();
    (compact.len() == 32).then(|| u128::from_str_radix(&compact, 16).ok()).flatten()
}
fn uuid_string(uuid: u128) -> String {
    let s = format!("{uuid:032x}");
    format!("{}-{}-{}-{}-{}", &s[0..8], &s[8..12], &s[12..16], &s[16..20], &s[20..32])
}
fn find_usercache(world: &Path, name: &str) -> Option<u128> {
    let cache = world.parent()?.join("usercache.json");
    let entries: serde_json::Value = serde_json::from_slice(&fs::read(cache).ok()?).ok()?;
    entries.as_array()?.iter().find(|entry| entry["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .and_then(|entry| entry["uuid"].as_str()).and_then(parse_uuid)
}
