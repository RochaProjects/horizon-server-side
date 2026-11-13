// Render and manage the bundled Java AuraNPC models as persistent item
// displays, with one invisible interaction volume for configured commands.
use super::{smp_npcs::{self, Npc}, smp_store};
use crate::game::{Game, native_server::NativeServerPlayer};
use horizon_client::{items::ItemStack, text::Text};
use horizon_world::nbt::{Compound, Tag};

const MAX_CUBES: u32 = 300;
const INTERACTION_PART: u32 = u32::MAX - 1;
const NAME_PART: u32 = u32::MAX - 2;

fn f32_tag(value: f64) -> Tag { Tag::Float((value as f32).to_bits()) }
fn float_list(values: &[f64]) -> Tag { Tag::List(values.iter().map(|v| f32_tag(*v)).collect()) }
fn vec3(value: &serde_json::Value) -> [f64; 3] {
    let a = value.as_array();
    [0, 1, 2].map(|i| a.and_then(|a| a.get(i)).and_then(serde_json::Value::as_f64).unwrap_or(0.0))
}
fn bedrock_pos(mut v: [f64; 3]) -> [f64; 3] { v[0] = -v[0]; v }
fn bedrock_angles(v: [f64; 3]) -> [f64; 3] { [-v[0].to_radians(), -v[1].to_radians(), v[2].to_radians()] }

type Quat = [f64; 4];
fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[3]*b[0] + a[0]*b[3] + a[1]*b[2] - a[2]*b[1],
        a[3]*b[1] - a[0]*b[2] + a[1]*b[3] + a[2]*b[0],
        a[3]*b[2] + a[0]*b[1] - a[1]*b[0] + a[2]*b[3],
        a[3]*b[3] - a[0]*b[0] - a[1]*b[1] - a[2]*b[2],
    ]
}
fn qxyz(a: [f64; 3]) -> Quat {
    let (sx, cx) = (a[0] / 2.0).sin_cos();
    let (sy, cy) = (a[1] / 2.0).sin_cos();
    let (sz, cz) = (a[2] / 2.0).sin_cos();
    qmul(qmul([sx, 0.0, 0.0, cx], [0.0, sy, 0.0, cy]), [0.0, 0.0, sz, cz])
}
fn qrotate(q: Quat, v: [f64; 3]) -> [f64; 3] {
    let u = [q[0], q[1], q[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]];
    let uv = cross(u, v);
    let uuv = cross(u, uv);
    [v[0] + 2.0 * (q[3]*uv[0] + uuv[0]), v[1] + 2.0 * (q[3]*uv[1] + uuv[1]), v[2] + 2.0 * (q[3]*uv[2] + uuv[2])]
}
fn bone_rotation(geometry: &serde_json::Value, name: &str, depth: usize) -> Quat {
    if depth > 64 { return [0.0, 0.0, 0.0, 1.0]; }
    let bones = geometry.get("bones").and_then(serde_json::Value::as_array);
    let Some(bone) = bones.and_then(|bones| bones.iter().find(|b| b.get("name").and_then(serde_json::Value::as_str) == Some(name))) else {
        return [0.0, 0.0, 0.0, 1.0];
    };
    let local = qxyz(bedrock_angles(vec3(&bone["rotation"])));
    let parent = bone.get("parent").and_then(serde_json::Value::as_str).unwrap_or_default();
    if parent.is_empty() { local } else { qmul(bone_rotation(geometry, parent, depth + 1), local) }
}
fn bone_point(geometry: &serde_json::Value, name: &str, point: [f64; 3], depth: usize) -> [f64; 3] {
    if depth > 64 { return point; }
    let bones = geometry.get("bones").and_then(serde_json::Value::as_array);
    let Some(bone) = bones.and_then(|bones| bones.iter().find(|b| b.get("name").and_then(serde_json::Value::as_str) == Some(name))) else {
        return point;
    };
    let pivot = bedrock_pos(vec3(&bone["pivot"]));
    let q = qxyz(bedrock_angles(vec3(&bone["rotation"])));
    let local = qrotate(q, [point[0]-pivot[0], point[1]-pivot[1], point[2]-pivot[2]]);
    let transformed = [pivot[0]+local[0], pivot[1]+local[1], pivot[2]+local[2]];
    let parent = bone.get("parent").and_then(serde_json::Value::as_str).unwrap_or_default();
    if parent.is_empty() { transformed } else { bone_point(geometry, parent, transformed, depth + 1) }
}

fn cubes(model: &smp_npcs::Model) -> Vec<(String, serde_json::Value)> {
    let mut out = Vec::new();
    let Some(bones) = model.data.pointer("/geometryData/bones").and_then(serde_json::Value::as_array) else { return out };
    for bone in bones {
        let name = bone.get("name").and_then(serde_json::Value::as_str).unwrap_or_default();
        let Some(parts) = bone.get("cubes").and_then(serde_json::Value::as_array) else { continue };
        for cube in parts {
            let size = vec3(&cube["size"]);
            let uv = &cube["uv"];
            if !uv.is_object() && !uv.is_array() { continue; }
            if size.iter().all(|v| *v == 0.0) { continue; }
            out.push((name.to_owned(), cube.clone()));
            if out.len() > MAX_CUBES as usize { return Vec::new(); }
        }
    }
    out
}

fn display_transform(rotation: Quat, scale: f32) -> Compound {
    let mut t = Compound::new();
    t.insert("translation".into(), float_list(&[0.0; 3]));
    t.insert("left_rotation".into(), float_list(&rotation));
    t.insert("scale".into(), float_list(&[f64::from(scale); 3]));
    t.insert("right_rotation".into(), float_list(&[0.0, 0.0, 0.0, 1.0]));
    t
}

fn display_item(model: &smp_npcs::Model, cube_index: usize) -> Option<Compound> {
    let mut stack = ItemStack { item: "minecraft:paper".into(), count: 1, damage: 0, extra: None };
    let model_id = format!("aura:npc/{}/cube_{cube_index}", model.key);
    stack.data_mut().set_other("minecraft:item_model", Some(Tag::String(model_id.as_str().into())));
    match horizon_items::item_nbt::item_to_nbt(&stack) { Tag::Compound(c) => Some(c), _ => None }
}

fn summon_id(game: &mut Game<'_>, npc: &Npc, part: u32, kind: &str, pos: [f64; 3], nbt: Compound) -> bool {
    game.native_summon_with_uuid(&npc.dimension, kind, pos, nbt, smp_npcs::entity_uuid(&npc.id, part)).is_some()
}

fn remove_rendered(game: &mut Game<'_>, npc: &Npc) {
    // A model can be edited to one with fewer cubes. Clear the full reserved
    // range so no old cube remains floating beside the new model.
    for part in 0..MAX_CUBES {
        if let Some((dimension, entity, _)) = game.native_entity_by_uuid(smp_npcs::entity_uuid(&npc.id, part)) {
            game.native_remove_entity(&dimension, entity);
        }
    }
    for part in [INTERACTION_PART, NAME_PART] {
        if let Some((dimension, entity, _)) = game.native_entity_by_uuid(smp_npcs::entity_uuid(&npc.id, part)) {
            game.native_remove_entity(&dimension, entity);
        }
    }
}

pub fn render(game: &mut Game<'_>, npc: &Npc) -> Result<usize, String> {
    let model = smp_npcs::model(&npc.model).ok_or_else(|| format!("Modelo AuraNPC não encontrado: {}", npc.model))?;
    let cubes = cubes(&model);
    if cubes.is_empty() || cubes.len() > MAX_CUBES as usize { return Err("O modelo não tem cubos renderizáveis ou excede 300 partes.".into()); }
    remove_rendered(game, npc);
    let geometry = &model.data["geometryData"];
    let body = qxyz([0.0, -f64::from(npc.yaw).to_radians(), 0.0]);
    let mut spawned = 0;
    for (index, (bone_name, cube)) in cubes.iter().enumerate() {
        let origin = vec3(&cube["origin"]);
        let size = vec3(&cube["size"]);
        let center = bedrock_pos([origin[0]+size[0]*0.5, origin[1]+size[1]*0.5, origin[2]+size[2]*0.5]);
        let pivot = bedrock_pos(if cube.get("pivot").is_some() { vec3(&cube["pivot"]) } else { [origin[0]+size[0]*0.5, origin[1]+size[1]*0.5, origin[2]+size[2]*0.5] });
        let local_rot = qxyz(bedrock_angles(vec3(&cube["rotation"])));
        let delta = qrotate(local_rot, [center[0]-pivot[0], center[1]-pivot[1], center[2]-pivot[2]]);
        let transformed = bone_point(geometry, bone_name, [pivot[0]+delta[0], pivot[1]+delta[1], pivot[2]+delta[2]], 0);
        let offset = qrotate(body, transformed).map(|v| v / 16.0);
        let pos = [npc.position[0]+offset[0], npc.position[1]+offset[1], npc.position[2]+offset[2]];
        let display_rotation = qmul(qmul(qmul(body, bone_rotation(geometry, bone_name, 0)), local_rot), [0.0, 1.0, 0.0, 0.0]);
        let mut nbt = Compound::new();
        nbt.insert("item".into(), Tag::Compound(display_item(&model, index).ok_or("Stack do modelo inválido")?));
        nbt.insert("item_display".into(), Tag::String("fixed".into()));
        nbt.insert("transformation".into(), Tag::Compound(display_transform(display_rotation, npc.scale)));
        nbt.insert("view_range".into(), f32_tag(0.8));
        nbt.insert("shadow_radius".into(), f32_tag(0.0));
        nbt.insert("shadow_strength".into(), f32_tag(0.0));
        nbt.insert("no_gravity".into(), Tag::Byte(1));
        nbt.insert("invulnerable".into(), Tag::Byte(1));
        nbt.insert("silent".into(), Tag::Byte(1));
        nbt.insert("interpolation_duration".into(), Tag::Int(0));
        nbt.insert("teleport_duration".into(), Tag::Int(0));
        if summon_id(game, npc, index as u32, "minecraft:item_display", pos, nbt) {
            spawned += 1;
        } else {
            remove_rendered(game, npc);
            return Err(format!("Não foi possível criar a parte {} do modelo. A dimensão precisa estar carregada.", index + 1));
        }
    }
    let mut hitbox = Compound::new();
    hitbox.insert("width".into(), f32_tag(f64::from(npc.hitbox_width)));
    hitbox.insert("height".into(), f32_tag(f64::from(npc.hitbox_height)));
    hitbox.insert("response".into(), Tag::Byte(1));
    if !summon_id(game, npc, INTERACTION_PART, "minecraft:interaction", npc.position, hitbox) {
        remove_rendered(game, npc);
        return Err("Não foi possível criar a área de clique do NPC. Confira se a dimensão está carregada.".into());
    }
    if npc.name_visible && !npc.name.is_empty() {
        let mut text = Compound::new();
        let serialized_name = serde_json::json!({"text":npc.name,"italic":false,"color":"white"}).to_string();
        text.insert("text".into(), Tag::String(serialized_name.as_str().into()));
        text.insert("line_width".into(), Tag::Int(240));
        text.insert("alignment".into(), Tag::String("center".into()));
        text.insert("billboard".into(), Tag::String("center".into()));
        text.insert("background".into(), Tag::Int(0));
        text.insert("default_background".into(), Tag::Byte(0));
        text.insert("see_through".into(), Tag::Byte(1));
        if !summon_id(game, npc, NAME_PART, "minecraft:text_display", [npc.position[0], npc.position[1]+f64::from(npc.hitbox_height)+0.15, npc.position[2]], text) {
            remove_rendered(game, npc);
            return Err("Não foi possível criar o nome visível do NPC.".into());
        }
    }
    Ok(spawned)
}

fn remove(game: &mut Game<'_>, npc: &Npc) { remove_rendered(game, npc); }

fn tell(game: &mut Game<'_>, player: u128, text: impl Into<String>) {
    game.native_tell(player, Text::literal(text.into()));
}

pub fn command(game: &mut Game<'_>, player: &NativeServerPlayer, line: &str) -> bool {
    if !smp_npcs::COMMANDS.contains(&line.split_whitespace().next().unwrap_or_default()) { return false; }
    let Some(world) = game.native_world_dir() else { return true };
    let dir = world.join(smp_store::DIR);
    let mut data = match smp_npcs::Data::open(&dir) {
        Ok(data) => data,
        Err(error) => { tell(game, player.uuid, format!("NPCs desativados: {error}")); return true; }
    };
    let words: Vec<&str> = line.split_whitespace().collect();
    let action = words.get(1).copied().unwrap_or("listar");
    let mut save = false;
    let reply = match action {
        "modelos" | "models" | "modelos-listar" => format!("Modelos AuraNPC: {}", smp_npcs::models().iter().map(|m| m.id.clone()).collect::<Vec<_>>().join(", ")),
        "listar" | "lista" | "list" => {
            if data.npcs.is_empty() { "Nenhum NPC criado.".into() }
            else { format!("NPCs: {}", data.npcs.values().map(|n| format!("{} [{}] @ {} ({:.1}, {:.1}, {:.1})", n.id, n.model, n.dimension, n.position[0], n.position[1], n.position[2])).collect::<Vec<_>>().join("; ")) }
        }
        "info" => {
            let Some(id) = words.get(2) else { tell(game, player.uuid, "Uso: /npc info <id>"); return true };
            let Some(npc) = data.npcs.get(&id.to_ascii_lowercase()) else { tell(game, player.uuid, "NPC não encontrado."); return true };
            format!("{} | modelo={} | nome={} | dimensão={} | posição={:.2} {:.2} {:.2} | yaw={} | escala={} | hitbox={}x{} | ações={}", npc.id, npc.model, npc.name, npc.dimension, npc.position[0], npc.position[1], npc.position[2], npc.yaw, npc.scale, npc.hitbox_width, npc.hitbox_height, npc.commands.len())
        }
        "criar" | "create" => {
            let mut p = line.splitn(4, ' '); let _ = p.next(); let _ = p.next();
            let (Some(id), Some(tail)) = (p.next(), p.next()) else { tell(game, player.uuid, "Uso: /npc criar <id> <modelo> <nome>"); return true };
            let mut t = tail.splitn(2, ' ');
            let (Some(model_arg), Some(name)) = (t.next(), t.next()) else { tell(game, player.uuid, "Informe o modelo e o nome visível."); return true };
            let id = id.to_ascii_lowercase();
            if !smp_npcs::valid_id(&id) { tell(game, player.uuid, "ID inválido: use até 32 letras, números, _ ou -."); return true; }
            if data.npcs.contains_key(&id) { tell(game, player.uuid, "Já existe um NPC com esse ID."); return true; }
            let Some(model) = smp_npcs::model(model_arg) else { tell(game, player.uuid, format!("Modelo não encontrado. Use /npc modelos. Entrada: {model_arg}")); return true };
            let npc = Npc { id: id.clone(), model: model.id, name: name.trim().to_owned(), dimension: player.dimension.clone(), position: player.position, yaw: player.yaw, ..Npc::default() };
            match render(game, &npc) {
                Ok(parts) => { data.npcs.insert(id.clone(), npc); save = true; format!("NPC {id} criado com {parts} partes 3D.") }
                Err(error) => { tell(game, player.uuid, error); return true; }
            }
        }
        "remover" | "remove" | "delete" => {
            let Some(id) = words.get(2) else { tell(game, player.uuid, "Uso: /npc remover <id>"); return true };
            if let Some(npc) = data.npcs.remove(&id.to_ascii_lowercase()) { remove(game, &npc); save = true; format!("NPC {} removido.", npc.id) }
            else { "NPC não encontrado.".into() }
        }
        "editar" | "edit" => {
            let (Some(id), Some(field)) = (words.get(2), words.get(3)) else { tell(game, player.uuid, "Uso: /npc editar <id> <campo> <valor>"); return true };
            let Some(npc) = data.npcs.get_mut(&id.to_ascii_lowercase()) else { tell(game, player.uuid, "NPC não encontrado."); return true };
            let mut rerender = true;
            let value = line.splitn(4, ' ').nth(3).unwrap_or_default();
            let value = value.split_once(' ').map(|(_, v)| v).unwrap_or_default().trim();
            match *field {
                "nome" | "name" => { npc.name = value.replace("\\n", "\n").replace('|', "\n"); }
                "modelo" | "model" => {
                    let Some(model) = smp_npcs::model(value) else { tell(game, player.uuid, "Modelo não encontrado. Use /npc modelos."); return true };
                    npc.model = model.id;
                }
                "dimensao" | "dimension" => {
                    if !value.contains(':') || value.len() > 128 { tell(game, player.uuid, "Informe um ID de dimensão válido, como auraessentials:mina."); return true; }
                    npc.dimension = value.to_owned();
                }
                "pos" | "posicao" | "position" => {
                    if value.is_empty() { npc.dimension = player.dimension.clone(); npc.position = player.position; npc.yaw = player.yaw; }
                    else {
                        let xyz: Vec<_> = value.split_whitespace().filter_map(|n| n.parse::<f64>().ok()).collect();
                        if xyz.len() != 3 || !xyz.iter().all(|n| n.is_finite() && n.abs() <= 30_000_000.0) { tell(game, player.uuid, "Use /npc editar <id> pos <x> <y> <z>."); return true; }
                        npc.dimension = player.dimension.clone(); npc.position = [xyz[0], xyz[1], xyz[2]];
                    }
                }
                "yaw" | "rotacao" => {
                    let Ok(yaw) = value.parse::<f32>() else { tell(game, player.uuid, "Yaw inválido."); return true };
                    if !yaw.is_finite() { tell(game, player.uuid, "Yaw inválido."); return true; }
                    npc.yaw = yaw.rem_euclid(360.0);
                }
                "escala" | "scale" => {
                    let Ok(scale) = value.parse::<f32>() else { tell(game, player.uuid, "Escala inválida."); return true };
                    if !scale.is_finite() || !(0.1..=3.0).contains(&scale) { tell(game, player.uuid, "A escala deve ficar entre 0.1 e 3.0."); return true; }
                    npc.scale = scale;
                }
                "hitbox" => {
                    let values: Vec<_> = value.split_whitespace().filter_map(|n| n.parse::<f32>().ok()).collect();
                    if values.len() != 2 || values.iter().any(|n| !n.is_finite() || !(0.1..=8.0).contains(n)) { tell(game, player.uuid, "Use largura e altura entre 0.1 e 8.0."); return true; }
                    npc.hitbox_width = values[0]; npc.hitbox_height = values[1];
                }
                "nomevisivel" | "namevisible" => {
                    match value { "true" | "sim" => npc.name_visible = true, "false" | "nao" | "não" => npc.name_visible = false, _ => { tell(game, player.uuid, "Use true ou false."); return true; } }
                }
                "comando" | "acao" | "action" => {
                    let tail = line.splitn(5, ' ').nth(4).unwrap_or_default().trim();
                    if tail == "limpar" { npc.commands.clear(); rerender = false; }
                    else if tail == "listar" { tell(game, player.uuid, format!("Ações de {}: {}", npc.id, if npc.commands.is_empty() { "nenhuma".into() } else { npc.commands.join(" | ") })); return true; }
                    else if let Some(index) = tail.strip_prefix("remover ").and_then(|v| v.parse::<usize>().ok()) {
                        if index == 0 || index > npc.commands.len() { tell(game, player.uuid, "Índice de ação inválido."); return true; }
                        npc.commands.remove(index - 1); rerender = false;
                    } else if let Some(command) = tail.strip_prefix("adicionar ") {
                        let command = command.trim().trim_start_matches('/');
                        if command.is_empty() { tell(game, player.uuid, "Informe o comando da ação."); return true; }
                        npc.commands.push(command.to_owned()); rerender = false;
                    } else {
                        let command = tail.trim_start_matches('/');
                        if command.is_empty() { tell(game, player.uuid, "Informe o comando, por exemplo: /kits."); return true; }
                        npc.commands = vec![command.to_owned()]; rerender = false;
                    }
                }
                _ => { tell(game, player.uuid, "Campos: nome, modelo, dimensao, pos, yaw, escala, hitbox, nomevisivel e comando."); return true; }
            }
            if rerender {
                let old = npc.clone();
                match render(game, &old) { Ok(_) => {}, Err(error) => { tell(game, player.uuid, error); return true; } }
            }
            save = true;
            format!("NPC {} atualizado.", npc.id)
        }
        _ => "Use /npc listar, modelos, criar, info, editar ou remover.".into(),
    };
    if save && let Err(error) = data.save(&dir) { tell(game, player.uuid, format!("Falha ao salvar NPCs: {error}")); return true; }
    tell(game, player.uuid, reply);
    true
}

pub fn interact_entity(game: &mut Game<'_>, entity: i32, kind: &str, claimed: &mut bool) {
    if *claimed || kind != "minecraft:interaction" || !game.native_on(smp_glue_id()) { return; }
    let Some(player) = game.native_acting_player() else { return };
    let Some(world) = game.native_world_dir() else { return };
    let Ok(data) = smp_npcs::Data::open(&world.join(smp_store::DIR)) else { return };
    for npc in data.npcs.values() {
        let Some((dimension, npc_entity, _)) = game.native_entity_by_uuid(smp_npcs::entity_uuid(&npc.id, INTERACTION_PART)) else { continue };
        if Some(npc_entity) != u32::try_from(entity).ok() || dimension != player.dimension { continue; }
        *claimed = true;
        if npc.commands.is_empty() { tell(game, player.uuid, &npc.name); return; }
        for line in &npc.commands {
            let line = line
                .replace("{player}", &player.name)
                .replace("%player%", &player.name)
                .replace("{npc}", &npc.name);
            let _ = game.native_run_command_as(player.uuid, line.trim_start_matches('/'));
        }
        return;
    }
}

fn smp_glue_id() -> &'static str { "aura_essentials" }
