// `/report <player> <reason>`: any player tells the staff about another one.
// The report is appended to `reports.log` (one JSON object a line), every
// operator of `LEVEL` on reads it at once, and an operator who joins later
// is told how many came in the last day; `/reports [page]` lists them. A
// reason is required, and a player reports at most once every
// `cooldown_seconds` (120). No game types. Added to the game as
// `game::smp_reports`; also a module of this crate, for its tests.

use super::smp_economy::{Out, Player};
use super::smp_store::{self, Audit};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

/// The operator level that reads reports.
pub const LEVEL: u8 = 3;

pub const COMMANDS: [&str; 2] = ["report", "reports"];

/// Reports kept in memory for `/reports` (the file keeps them all).
const KEEP: usize = 100;
/// `/reports` lines a page.
const PAGE: usize = 8;
/// The end of `reports.log` read at the start.
const TAIL: u64 = 256 * 1024;
const DAY_MS: i64 = 86_400_000;

/// `config.json`'s `reports`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    /// Between two reports of one player.
    pub cooldown_seconds: i64,
    /// Longer reasons are cut to this many characters.
    pub max_reason: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            cooldown_seconds: 120,
            max_reason: 200,
        }
    }
}

/// One report, as `reports.log` keeps it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    /// When (ms since 1970).
    pub time: i64,
    /// Who reported (UUID, name).
    pub by: String,
    pub by_name: String,
    /// Who was reported (UUID, name).
    pub player: String,
    pub player_name: String,
    pub reason: String,
    /// Where the reporter stood, and the reported player if on.
    pub by_at: String,
    pub player_at: String,
}

pub struct Reports {
    pub config: Config,
    file: Audit,
    recent: VecDeque<Entry>,
    /// The last report of each player (this run).
    last: HashMap<u128, i64>,
    /// The operators on at the last look, so one who joins is told once.
    staff_on: HashSet<u128>,
    /// Lines for the server's log (each report), taken by the glue.
    pub logged: Vec<String>,
}

impl Reports {
    /// `reports.log` in `dir`: its last reports are read back for `/reports`.
    pub fn open(dir: &Path, config: Config) -> Self {
        let path = dir.join("reports.log");
        let mut recent: VecDeque<Entry> = smp_store::tail(&path, TAIL)
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        while recent.len() > KEEP {
            recent.pop_front();
        }
        Self {
            config,
            file: Audit::new(path),
            recent,
            last: HashMap::new(),
            staff_on: HashSet::new(),
            logged: Vec::new(),
        }
    }

    /// Writes what was appended to disk.
    pub fn save(&mut self) {
        self.file.sync();
    }

    /// `/report <player> <reason>` and `/reports [page]`; None for another
    /// command. `find`: a player who has played here, by name (UUID, name);
    /// `at`: where a player on stands; `staff`: the operators of `LEVEL` on.
    pub fn command(
        &mut self,
        me: &Player,
        find: &dyn Fn(&str) -> Option<(u128, String)>,
        at: &dyn Fn(u128) -> Option<String>,
        staff: &[u128],
        now: i64,
        line: &str,
    ) -> Option<Vec<Out>> {
        let mut words = line.split_whitespace();
        match words.next()? {
            "report" => {}
            "reports" => return Some(self.list(me, words.next())),
            _ => return None,
        }
        if !self.config.enabled {
            return Some(vec![Out::Error(
                "/report is turned off on this server.".into(),
            )]);
        }
        let Some(name) = words.next() else {
            return Some(vec![Out::Error("Usage: /report <player> <reason>".into())]);
        };
        let reason: String = words
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(self.config.max_reason.max(1))
            .collect();
        if reason.is_empty() {
            return Some(vec![Out::Error(
                "Say why: /report <player> <reason>".into(),
            )]);
        }
        let Some((player, player_name)) = find(name) else {
            return Some(vec![Out::Error("No player was found".into())]);
        };
        if player == me.uuid {
            return Some(vec![Out::Error("You can't report yourself.".into())]);
        }
        if let Some(&t) = self.last.get(&me.uuid) {
            let wait = t + self.config.cooldown_seconds * 1000 - now;
            if wait > 0 {
                return Some(vec![Out::Error(format!(
                    "Wait {} before your next report.",
                    wait_text(wait)
                ))]);
            }
        }
        let entry = Entry {
            time: now,
            by: smp_store::uuid_string(me.uuid),
            by_name: me.name.clone(),
            player: smp_store::uuid_string(player),
            player_name: player_name.clone(),
            reason,
            by_at: at(me.uuid).unwrap_or_default(),
            player_at: at(player).unwrap_or_default(),
        };
        let json = serde_json::to_value(&entry).unwrap_or_default();
        if let Err(e) = self.file.append(&json) {
            self.logged
                .push(format!("a report couldn't be written: {e}"));
            return Some(vec![Out::Error(
                "Your report couldn't be saved: please tell a staff member.".into(),
            )]);
        }
        self.last.insert(me.uuid, now);
        self.logged.push(format!(
            "report by {} on {}: {} ({})",
            entry.by_name,
            entry.player_name,
            entry.reason,
            if entry.by_at.is_empty() {
                "?"
            } else {
                &entry.by_at
            }
        ));
        let mut out = vec![Out::Reply(format!(
            "Thanks, your report on {player_name} was sent to the staff."
        ))];
        let others: Vec<u128> = staff.iter().copied().filter(|&u| u != me.uuid).collect();
        if others.is_empty() {
            out.push(Out::Reply(
                "No staff member is on right now: they will read it when they are.".into(),
            ));
        }
        for u in others {
            out.push(Out::Tell(u, format!("[Report] {}", row(&entry))));
        }
        self.recent.push_back(entry);
        if self.recent.len() > KEEP {
            self.recent.pop_front();
        }
        Some(out)
    }

    /// `/reports [page]`: the newest first.
    fn list(&self, me: &Player, page: Option<&str>) -> Vec<Out> {
        if me.level < LEVEL {
            return vec![Out::Error("Only operators can read reports.".into())];
        }
        if self.recent.is_empty() {
            return vec![Out::Reply("No reports.".into())];
        }
        let pages = self.recent.len().div_ceil(PAGE);
        let page = page
            .and_then(|p| p.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, pages);
        let mut out = vec![Out::Reply(format!(
            "Reports, newest first (page {page} of {pages}):"
        ))];
        for e in self.recent.iter().rev().skip((page - 1) * PAGE).take(PAGE) {
            out.push(Out::Reply(format!(
                "{} {}",
                smp_store::utc_minute(e.time),
                row(e)
            )));
        }
        out
    }

    /// Once a second, the players on with their operator level: each
    /// operator who came on is told the reports of the last day.
    pub fn staff_joined(&mut self, on: &[(u128, u8)], now: i64) -> Vec<Out> {
        let staff: HashSet<u128> = on.iter().filter(|p| p.1 >= LEVEL).map(|p| p.0).collect();
        let day = self.recent.iter().filter(|e| e.time > now - DAY_MS).count();
        let out = staff
            .iter()
            .filter(|u| !self.staff_on.contains(u) && day > 0)
            .map(|&u| {
                let s = if day == 1 { "" } else { "s" };
                Out::Tell(
                    u,
                    format!("{day} player report{s} in the last 24 hours: /reports"),
                )
            })
            .collect();
        self.staff_on = staff;
        out
    }
}

/// `Alex reported Steve: griefing (overworld 12 64 -30)`.
fn row(e: &Entry) -> String {
    let at = if e.player_at.is_empty() {
        &e.by_at
    } else {
        &e.player_at
    };
    let at = if at.is_empty() {
        String::new()
    } else {
        format!(" ({at})")
    };
    format!("{} reported {}: {}{at}", e.by_name, e.player_name, e.reason)
}

/// `1 min 40 s`, `40 s`.
fn wait_text(ms: i64) -> String {
    let s = (ms + 999) / 1000;
    match (s / 60, s % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    const ALEX: u128 = 1;
    const STEVE: u128 = 2;
    const OP: u128 = 3;

    fn dir() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("smp-reports-{}-{n}", std::process::id()))
    }
    fn player(uuid: u128, name: &str, level: u8) -> Player {
        Player {
            uuid,
            name: name.into(),
            level,
        }
    }
    fn find(name: &str) -> Option<(u128, String)> {
        [(ALEX, "Alex"), (STEVE, "Steve"), (OP, "Op")]
            .into_iter()
            .find(|p| p.1.eq_ignore_ascii_case(name))
            .map(|(u, n)| (u, n.to_owned()))
    }
    fn at(uuid: u128) -> Option<String> {
        (uuid == STEVE).then(|| "minecraft:overworld 12 64 -30".to_owned())
    }

    #[test]
    fn a_report_reaches_the_staff_on_and_the_file() {
        let d = dir();
        let mut r = Reports::open(&d, Config::default());
        let out = r
            .command(
                &player(ALEX, "Alex", 0),
                &find,
                &at,
                &[OP],
                1_000,
                "report steve griefing my  house",
            )
            .unwrap();
        assert_eq!(
            out,
            vec![
                Out::Reply("Thanks, your report on Steve was sent to the staff.".into()),
                Out::Tell(
                    OP,
                    "[Report] Alex reported Steve: griefing my house (minecraft:overworld 12 64 -30)".into()
                ),
            ]
        );
        assert_eq!(r.logged.len(), 1);
        r.save();
        let saved: Entry = serde_json::from_str(
            std::fs::read_to_string(d.join("reports.log"))
                .unwrap()
                .trim(),
        )
        .unwrap();
        assert_eq!(
            (
                saved.by_name.as_str(),
                saved.player.as_str(),
                saved.reason.as_str(),
                saved.time
            ),
            (
                "Alex",
                "00000000-0000-0000-0000-000000000002",
                "griefing my house",
                1_000
            )
        );
        // Read back after a restart, for /reports.
        let again = Reports::open(&d, Config::default());
        let out = again.list(&player(OP, "Op", 3), None);
        assert_eq!(out.len(), 2, "{out:?}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_reason_a_known_player_and_two_minutes_between_reports_are_needed() {
        let d = dir();
        let mut r = Reports::open(&d, Config::default());
        let alex = player(ALEX, "Alex", 0);
        let mut run = |now, line: &str| r.command(&alex, &find, &at, &[], now, line).unwrap();
        assert_eq!(
            run(0, "report"),
            vec![Out::Error("Usage: /report <player> <reason>".into())]
        );
        assert_eq!(
            run(0, "report Steve"),
            vec![Out::Error("Say why: /report <player> <reason>".into())]
        );
        assert_eq!(
            run(0, "report Nobody spam"),
            vec![Out::Error("No player was found".into())]
        );
        assert_eq!(
            run(0, "report alex spam"),
            vec![Out::Error("You can't report yourself.".into())]
        );
        // With no operator on, the reporter is told so.
        let out = run(0, "report Steve spam");
        assert!(
            matches!(&out[1], Out::Reply(s) if s.starts_with("No staff member")),
            "{out:?}"
        );
        assert_eq!(
            run(20_000, "report Op abuse"),
            vec![Out::Error(
                "Wait 1 min 40 s before your next report.".into()
            )]
        );
        assert!(matches!(&run(120_000, "report Op abuse")[0], Out::Reply(_)));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn only_operators_list_reports_and_one_who_joins_is_told_once() {
        let d = dir();
        let mut r = Reports::open(&d, Config::default());
        assert_eq!(
            r.command(&player(ALEX, "Alex", 0), &find, &at, &[], 0, "reports")
                .unwrap(),
            vec![Out::Error("Only operators can read reports.".into())]
        );
        assert!(r.staff_joined(&[(OP, 4)], 0).is_empty(), "nothing to tell");
        r.staff_joined(&[], 0);
        for i in 0..10 {
            let who = player(100 + i, &format!("P{i}"), 0);
            r.command(&who, &find, &at, &[], 1_000, "report Steve x")
                .unwrap();
        }
        // On already: not told again; coming on: told once.
        assert!(r.staff_joined(&[(ALEX, 0)], 2_000).is_empty());
        assert_eq!(
            r.staff_joined(&[(ALEX, 0), (OP, 4)], 3_000),
            vec![Out::Tell(
                OP,
                "10 player reports in the last 24 hours: /reports".into()
            )]
        );
        assert!(r.staff_joined(&[(OP, 4)], 4_000).is_empty());
        let op = player(OP, "Op", 4);
        let page2 = r.command(&op, &find, &at, &[], 5_000, "reports 2").unwrap();
        assert_eq!(
            page2[0],
            Out::Reply("Reports, newest first (page 2 of 2):".into())
        );
        assert_eq!(page2.len(), 3, "10 reports, 8 a page: {page2:?}");
        assert!(
            matches!(&page2[1], Out::Reply(s) if s.contains("P1 reported Steve")),
            "{page2:?}"
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
