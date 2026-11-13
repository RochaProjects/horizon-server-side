// Chat pings: a chat line that names an online player (a whole word, any
// case) plays the experience orb pickup sound to that player, and so does
// every `/msg`, `/tell` and `/w` to its recipient. Never the sender; at most
// one ping each 3 s per player; `/pingsound off` turns them off for oneself,
// kept in `pings.json`. Names are matched when a line is sent, never on a
// tick with nothing said. No game types. Added to the game as
// `game::smp_ping`; also a module of this crate, for its tests.

use super::smp_economy::Out;
use super::smp_store;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// The sound a ping plays.
pub const SOUND: &str = "minecraft:entity.experience_orb.pickup";
/// The shortest time between two pings of one player, in milliseconds.
pub const GAP_MS: i64 = 3_000;
/// The commands that ping their recipient.
pub const WHISPERS: [&str; 3] = ["msg", "tell", "w"];

/// `pings.json`.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Data {
    /// The players who turned pings off, by UUID.
    off: BTreeSet<String>,
}

pub struct Pings {
    path: PathBuf,
    data: Data,
    /// `data.off`, parsed.
    off: BTreeSet<u128>,
    /// When each player was last pinged (this run).
    last: HashMap<u128, i64>,
}

/// What was said: a chat line, or a whisper to the player named.
#[derive(Clone, Debug, PartialEq)]
pub enum Said {
    Chat(String),
    Whisper(String),
}

/// The whisper in a command line (no slash): its recipient's name, when the
/// line is `/msg <name> <message>` (or `/tell`, `/w`, `minecraft:` ones).
pub fn whisper_to(command: &str) -> Option<&str> {
    let mut words = command.split_whitespace();
    let verb = words.next()?;
    let verb = verb.strip_prefix("minecraft:").unwrap_or(verb);
    if !WHISPERS.iter().any(|w| w.eq_ignore_ascii_case(verb)) {
        return None;
    }
    let name = words.next()?;
    words.next()?;
    Some(name)
}

/// The players a line names: each `(uuid, name)` of `online` whose name is a
/// whole word of `line`, any case. Words are runs of letters, digits and `_`
/// (the characters of a name), so `@Alex`, `Alex!` and `alex,` all name Alex.
pub fn named(line: &str, online: &[(u128, String)]) -> Vec<u128> {
    let words: BTreeSet<String> = line
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| (3..=16).contains(&w.len()))
        .map(str::to_ascii_lowercase)
        .collect();
    online
        .iter()
        .filter(|(_, n)| words.contains(&n.to_ascii_lowercase()))
        .map(|&(u, _)| u)
        .collect()
}

impl Pings {
    /// `pings.json` in `dir`; an error when it exists but can't be read.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let path = dir.join("pings.json");
        let data: Data = smp_store::read_json(&path)?;
        let off = data
            .off
            .iter()
            .filter_map(|u| smp_store::parse_uuid(u))
            .collect();
        Ok(Self {
            path,
            data,
            off,
            last: HashMap::new(),
        })
    }

    /// Who `sender` pings by saying `said`: online players (`online`: UUID
    /// and name) named in it, not the sender, with pings on and none in the
    /// last 3 s. Counts them as pinged at `now`.
    pub fn pinged(
        &mut self,
        sender: u128,
        said: &Said,
        online: &[(u128, String)],
        now: i64,
    ) -> Vec<u128> {
        let targets = match said {
            Said::Chat(line) => named(line, online),
            Said::Whisper(name) => online
                .iter()
                .filter(|(_, n)| n.eq_ignore_ascii_case(name))
                .map(|&(u, _)| u)
                .collect(),
        };
        let mut out = Vec::new();
        for u in targets {
            if u == sender || self.off.contains(&u) || out.contains(&u) {
                continue;
            }
            if self.last.get(&u).is_some_and(|&t| now - t < GAP_MS) {
                continue;
            }
            self.last.insert(u, now);
            out.push(u);
        }
        out
    }

    /// `/pingsound`, `/pingsound on|off`; None for another command.
    pub fn command(&mut self, uuid: u128, line: &str) -> Option<Vec<Out>> {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.first() != Some(&"pingsound") {
            return None;
        }
        let key = smp_store::uuid_string(uuid);
        Some(match words.get(1..) {
            Some(["on"]) => {
                self.off.remove(&uuid);
                self.data.off.remove(&key);
                if let Err(e) = self.write() {
                    return Some(vec![Out::Error(e)]);
                }
                vec![Out::Reply(
                    "Ping sound on: you hear a ding when someone names you in chat or /msg's you."
                        .into(),
                )]
            }
            Some(["off"]) => {
                self.off.insert(uuid);
                self.data.off.insert(key);
                if let Err(e) = self.write() {
                    return Some(vec![Out::Error(e)]);
                }
                vec![Out::Reply(
                    "Ping sound off. /pingsound on turns it back on.".into(),
                )]
            }
            Some([]) => vec![Out::Reply(format!(
                "Ping sound is {}. /pingsound on|off",
                if self.off.contains(&uuid) {
                    "off"
                } else {
                    "on"
                }
            ))],
            _ => vec![Out::Error("Usage: /pingsound on|off".into())],
        })
    }

    fn write(&self) -> Result<(), String> {
        smp_store::save_json(&self.path, &self.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u128 = 1;
    const B: u128 = 2;
    const C: u128 = 3;

    fn online() -> Vec<(u128, String)> {
        vec![
            (A, "Alex".into()),
            (B, "Steve_99".into()),
            (C, "jaszka".into()),
        ]
    }

    #[test]
    fn a_name_pings_as_a_whole_word_in_any_case() {
        assert_eq!(named("hey @alex, look", &online()), vec![A]);
        assert_eq!(named("STEVE_99 and Jaszka!", &online()), vec![B, C]);
        assert!(named("alexander steve jaszkas", &online()).is_empty());
        assert_eq!(whisper_to("msg Steve_99 hi there"), Some("Steve_99"));
        assert_eq!(whisper_to("minecraft:w alex hi"), Some("alex"));
        assert_eq!(
            whisper_to("msg alex"),
            None,
            "no message: the command fails"
        );
        assert_eq!(whisper_to("tp alex"), None);
    }

    #[test]
    fn pings_skip_the_sender_wait_3_s_and_turn_off() {
        let dir = std::env::temp_dir().join(format!("smp-ping-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Pings::open(&dir).unwrap();
        let chat = |s: &str| Said::Chat(s.into());
        assert_eq!(p.pinged(A, &chat("alex steve_99"), &online(), 0), vec![B]);
        // Steve again within 3 s: nothing; after: pinged.
        assert!(p.pinged(C, &chat("steve_99?"), &online(), 2_999).is_empty());
        assert_eq!(
            p.pinged(C, &Said::Whisper("STEVE_99".into()), &online(), 3_000),
            vec![B]
        );
        // Off, then kept across a restart.
        assert!(matches!(
            &p.command(B, "pingsound off").unwrap()[0],
            Out::Reply(_)
        ));
        assert!(p.pinged(A, &chat("steve_99"), &online(), 60_000).is_empty());
        let mut again = Pings::open(&dir).unwrap();
        assert!(again.pinged(A, &chat("steve_99"), &online(), 0).is_empty());
        again.command(B, "pingsound on").unwrap();
        assert_eq!(again.pinged(A, &chat("steve_99"), &online(), 0), vec![B]);
        assert!(matches!(
            &again.command(B, "pingsound maybe").unwrap()[0],
            Out::Error(_)
        ));
        assert!(again.command(B, "ping").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
