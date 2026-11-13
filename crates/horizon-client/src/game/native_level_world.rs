//! A level's world settings for native server mods, through a `LevelRef`
//! (docs/native-mods.md, "Entity and level handles", #7387): the seed, the
//! difficulty, game rules and weather, and the setters of its time, game rules
//! and weather. The readers return `None` on a client or for a handle of an
//! earlier open; the setters return false.
use super::Game;

#[cfg(test)]
mod native_level_setters_tests;
use horizon_client::command::vanilla::DIFFICULTIES;
use horizon_client::gamerules;
use horizon_client::mod_api::{GameRuleValue, LevelRef};

/// `Level.isRaining` and `isThundering` from the rain and thunder levels.
fn weather_flags((rain, thunder): (f32, f32)) -> (bool, bool) {
    (rain > 0.2, thunder > 0.9)
}

impl Game<'_> {
    /// The world's seed (`WorldOptions.seed`), as `/seed` shows it. API
    /// @API:level_world@.
    pub(super) fn native_level_seed(&self, level: LevelRef) -> Option<i64> {
        self.native_level_world(level)?;
        Some(self.level.seed)
    }
    /// The difficulty (`Level.getDifficulty`): `peaceful`, `easy`, `normal`
    /// or `hard`, as `/difficulty` names it. API @API:level_world@.
    pub(super) fn native_level_difficulty(&self, level: LevelRef) -> Option<&'static str> {
        self.native_level_world(level)?;
        DIFFICULTIES.get(usize::from(self.ui.difficulty)).copied()
    }
    /// Game rule `rule` (`minecraft:keep_inventory` or `keep_inventory`), as
    /// `/gamerule` queries it: None for an unknown rule. API @API:level_world@.
    pub(super) fn native_level_game_rule(
        &self,
        level: LevelRef,
        rule: &str,
    ) -> Option<GameRuleValue> {
        self.native_level_world(level)?;
        let r = horizon_client::gamerules::rule(rule)?;
        if level.dimension() == self.dim.id() {
            self.ui.rules.get(r.name)
        } else {
            self.effective_rules_for_dimension(level.dimension()).get(r.name)
        }
    }
    /// The rain and thunder levels, each 0.0 to 1.0 (`Level.getRainLevel`,
    /// `getThunderLevel`): both 0 in a level with no weather, as the Nether
    /// and the End. API @API:level_world@.
    pub(super) fn native_level_weather(&self, level: LevelRef) -> Option<(f32, f32)> {
        let world = self.native_level_world(level)?;
        if level.dimension() != super::Dim::Overworld.id() {
            return Some((0.0, 0.0));
        }
        Some(world.weather.levels(1.0))
    }
    /// Whether it rains (`Level.isRaining`: a rain level above 0.2). API
    /// @API:level_world@.
    pub(super) fn native_level_is_raining(&self, level: LevelRef) -> Option<bool> {
        Some(weather_flags(self.native_level_weather(level)?).0)
    }
    /// Whether it thunders (`Level.isThundering`: a thunder level above 0.9).
    /// API @API:level_world@.
    pub(super) fn native_level_is_thundering(&self, level: LevelRef) -> Option<bool> {
        Some(weather_flags(self.native_level_weather(level)?).1)
    }
    /// Sets the time of day, as `/time set <ticks>` does on the overworld clock: mods hear
    /// `time_skip` (cause `mod`) first and can keep the old time, and the clock's fraction of a
    /// tick restarts. False for a negative time, for a handle that isn't this open's on the
    /// server, or when a mod kept the old time; a time the clock already shows is true and
    /// changes nothing. API @API:level_world_setters@.
    pub(super) fn native_level_set_day_time(&mut self, level: LevelRef, time: i64) -> bool {
        if time < 0 || self.native_level_world(level).is_none() {
            return false;
        }
        let clock = "minecraft:overworld";
        let total = self.command_clock_time(clock);
        total == time || self.jump_clock(clock, total, time, "mod")
    }
    /// Sets game rule `rule` (`minecraft:advance_time` or `advance_time`) as `/gamerule` does,
    /// so `advance_time` and `advance_weather` freeze and unfreeze the day clock and the weather
    /// cycle. False for an unknown rule, a value of the wrong kind or out of the rule's range,
    /// or a handle that isn't this open's on the server. API @API:level_world_setters@.
    pub(super) fn native_level_set_game_rule(
        &mut self,
        level: LevelRef,
        rule: &str,
        value: GameRuleValue,
    ) -> bool {
        let Some(rule) = gamerules::rule(rule) else {
            return false;
        };
        if self.native_level_world(level).is_none() || !self.ui.global_rules.clone().set(rule.name, value)
        {
            return false;
        }
        self.set_game_rule(rule, value);
        true
    }
    /// Reads a custom dimension's effective rule without making it active.
    pub(super) fn native_dimension_game_rule(&self, dimension: &str, rule: &str) -> Option<GameRuleValue> {
        if !self.native_custom_dimension_dir(dimension).is_some_and(|p| p.is_dir()) {
            return None;
        }
        self.effective_rules_for_dimension(dimension).get(rule)
    }
    pub(super) fn native_dimension_has_game_rule_override(&self, dimension: &str, rule: &str) -> bool {
        let Some(rule) = gamerules::rule(rule) else { return false };
        self.ui.dimension_rule_overrides.get(dimension).is_some_and(|values| values.contains_key(rule.name))
    }
    fn native_custom_dimension_dir(&self, dimension: &str) -> Option<std::path::PathBuf> {
        if dimension.starts_with("minecraft:") { return None; }
        let key = horizon_core::LevelKey::parse(dimension).ok()?;
        let namespace = key.namespace();
        let path = key.path();
        if namespace == "minecraft" || path.is_empty() || path.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..") {
            return None;
        }
        Some(self.world_root.join("dimensions").join(namespace).join(path))
    }
    /// Adds, changes, or clears (`None`) a custom-dimension rule override.
    pub(super) fn native_dimension_set_game_rule(&mut self, dimension: &str, rule: &str, value: Option<GameRuleValue>) -> bool {
        if !self.native_custom_dimension_dir(dimension).is_some_and(|p| p.is_dir()) {
            return false;
        }
        let Some(definition) = gamerules::rule(rule) else { return false };
        if value.is_some() && matches!(definition.name, "advance_time" | "advance_weather") {
            return false;
        }
        if let Some(value) = value {
            if !self.ui.global_rules.clone().set(definition.name, value) { return false; }
            self.ui.dimension_rule_overrides.entry(dimension.to_owned()).or_default().insert(definition.name.to_owned(), value);
        } else if let Some(overrides) = self.ui.dimension_rule_overrides.get_mut(dimension) {
            overrides.remove(definition.name);
            if overrides.is_empty() { self.ui.dimension_rule_overrides.remove(dimension); }
        }
        if let Err(error) = self.save_dimension_rule_overrides() {
            log::error!("Horizon: could not save dimension game-rule overrides: {error}");
            return false;
        }
        if dimension == self.dim.id() {
            self.ui.rules = self.effective_rules_for_dimension(dimension);
            self.sync_rules();
        }
        true
    }
    pub(super) fn native_dimension_clear_game_rule_overrides(&mut self, dimension: &str) -> bool {
        self.ui.dimension_rule_overrides.remove(dimension);
        if let Err(error) = self.save_dimension_rule_overrides() {
            log::error!("Horizon: could not save dimension game-rule overrides: {error}");
            return false;
        }
        if dimension == self.dim.id() {
            self.ui.rules = self.effective_rules_for_dimension(dimension);
            self.sync_rules();
        }
        true
    }
    pub(super) fn native_dimension_rename_game_rule_overrides(&mut self, old: &str, new: &str) -> bool {
        if let Some(values) = self.ui.dimension_rule_overrides.remove(old) {
            self.ui.dimension_rule_overrides.insert(new.to_owned(), values);
            if let Err(error) = self.save_dimension_rule_overrides() {
                log::error!("Horizon: could not save renamed dimension game-rule overrides: {error}");
                return false;
            }
        }
        true
    }
    /// Sets the weather as `/weather` does (`ServerLevel.setWeatherParameters`): `clear_time`
    /// ticks of clear weather, then `weather_time` ticks of the new state (rain, thunder or
    /// neither) as `raining` and `thundering` say. Mods hear `weather_change` and
    /// `thunder_change` first and can keep the old state. False for a negative time, a level that
    /// can't have weather (the Nether, the End) or isn't the one in place, a handle that isn't
    /// this open's on the server, or when a mod kept the old state. API
    /// @API:level_world_setters@.
    pub(super) fn native_level_set_weather(
        &mut self,
        level: LevelRef,
        clear_time: i32,
        weather_time: i32,
        raining: bool,
        thundering: bool,
    ) -> bool {
        if clear_time < 0
            || weather_time < 0
            || level.dimension() != self.dim.id()
            || self.native_level_world(level).is_none()
            || !self.can_have_weather()
        {
            return false;
        }
        self.apply_weather(clear_time, weather_time, raining, thundering)
    }
}

// The clock and weather changes `/time set` and `/weather` make, shared with the setters above.
impl Game<'_> {
    /// `/time set` on `clock`: the mods hear `time_skip` first and can keep `from`; the clock's
    /// fraction of a tick restarts. False when they did.
    pub(super) fn jump_clock(&mut self, clock: &str, from: i64, to: i64, cause: &str) -> bool {
        let mut cancel = false;
        self.native_time_skip(self.dim.id(), from, to, cause, &mut cancel);
        if !cancel {
            self.set_command_clock_time(clock, to);
            self.command_clock_state(clock).partial = 0.0;
        }
        !cancel
    }
    /// `ServerLevel.setWeatherParameters`, as `/weather` and the mods' weather setter call it: the
    /// mods hear `weather_change` and `thunder_change` first and can keep the old state. False when
    /// they did.
    pub(super) fn apply_weather(
        &mut self,
        clear: i32,
        time: i32,
        raining: bool,
        thundering: bool,
    ) -> bool {
        let previous = (
            self.world.weather.clear_time,
            self.world.weather.rain_time,
            self.world.weather.thunder_time,
            self.world.weather.raining,
            self.world.weather.thundering,
        );
        let mut cancel = false;
        self.native_weather_change(self.dim.id(), raining, thundering, &mut cancel);
        let old_thundering = self.world.weather.thundering;
        let old_thunder_time = self.world.weather.thunder_time;
        let mut cancel_thunder = false;
        self.native_thunder_change(self.dim.id(), thundering, &mut cancel_thunder);
        if cancel {
            let w = &mut self.world.weather;
            (
                w.clear_time,
                w.rain_time,
                w.thunder_time,
                w.raining,
                w.thundering,
            ) = previous;
        } else {
            // ServerLevel.setWeatherParameters.
            let w = &mut self.world.weather;
            w.clear_time = clear;
            w.rain_time = time;
            w.thunder_time = time;
            w.raining = raining;
            w.thundering = thundering;
            if cancel_thunder {
                w.thunder_time = old_thunder_time;
                w.thundering = old_thundering;
            }
        }
        !cancel
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rain_and_thunder_start_above_vanillas_thresholds() {
        assert_eq!(super::weather_flags((0.2, 0.0)), (false, false));
        assert_eq!(super::weather_flags((0.21, 0.9)), (true, false));
        assert_eq!(super::weather_flags((1.0, 0.95)), (true, true));
    }
}
