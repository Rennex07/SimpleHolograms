//! Plugin configuration, with upstream defaults for missing or invalid keys.

use crate::yaml::{self, Value};

/// The defaults in `FancyHologramsConfiguration.setOptions` (`:144-160`).
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub autosave_enabled: bool,
    pub autosave_interval: i32,
    pub save_on_changed: bool,
    pub log_level: String,
    pub log_on_world_load: bool,
    /// Retained for file compatibility; update checks are not implemented.
    pub version_notifications: bool,
    pub visibility_distance: i32,
    pub register_commands: bool,
    /// Ticks between visibility sweeps upstream. Unused here — the server
    /// tracks entities — but kept so the key is not silently dropped.
    pub update_visibility_interval: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            autosave_enabled: true,
            autosave_interval: 15,
            save_on_changed: true,
            log_level: "INFO".to_string(),
            log_on_world_load: true,
            version_notifications: true,
            visibility_distance: 20,
            register_commands: true,
            update_visibility_interval: 100,
        }
    }
}

impl Config {
    /// Parses a config document, using the default for any key that is absent
    /// or unparseable. Never fails: a broken config degrades to defaults
    /// rather than preventing the plugin from starting.
    pub fn parse(source: &str) -> Self {
        let doc = yaml::parse(source).unwrap_or(Value::Map(Vec::new()));
        let d = Self::default();
        let saving = doc.get("saving");
        let autosave = saving.and_then(|s| s.get("autosave"));
        let logging = doc.get("logging");

        Self {
            autosave_enabled: flag(autosave, "enabled", d.autosave_enabled),
            autosave_interval: number(autosave, "interval", d.autosave_interval),
            save_on_changed: flag(saving, "save_on_changed", d.save_on_changed),
            log_level: logging
                .and_then(|l| l.get("log_level"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(&d.log_level)
                .to_string(),
            log_on_world_load: flag(logging, "log_on_world_load", d.log_on_world_load),
            version_notifications: flag(logging, "version_notifications", d.version_notifications),
            visibility_distance: number(Some(&doc), "visibility_distance", d.visibility_distance),
            register_commands: flag(Some(&doc), "register_commands", d.register_commands),
            update_visibility_interval: number(
                Some(&doc),
                "update_visibility_interval",
                d.update_visibility_interval,
            ),
        }
    }
}

fn flag(node: Option<&Value>, key: &str, default: bool) -> bool {
    node.and_then(|n| n.get(key))
        .and_then(Value::as_str)
        .and_then(crate::text::parse_bool)
        .unwrap_or(default)
}

fn number(node: Option<&Value>, key: &str, default: i32) -> i32 {
    node.and_then(|n| n.get(key))
        .and_then(Value::as_str)
        .and_then(crate::text::parse_double)
        .map_or(default, |v| v as i32)
}

/// The default `config.yml`, written on first start.
///
/// Key names and defaults are the original's (`FancyHologramsConfiguration`).
/// Comments carrying meaning upstream are kept.
pub const DEFAULT_CONFIG: &str = "\
# FancyHolograms configuration.
#
# Inert keys, kept so an existing config still loads. This is a WebAssembly
# component with no network access, so the update check cannot run:
#   logging.version_notifications
# Unused here, because the server tracks hologram entities itself:
#   update_visibility_interval

config_version: 1 # Config version, do not modify.

saving:
  autosave:
    enabled: true # Whether autosave is enabled.
    interval: 15 # The interval at which autosave is performed in minutes.
  save_on_changed: true # Whether the plugin should save holograms when they are changed.

logging:
  log_level: INFO # The log level for the plugin (DEBUG, INFO, WARN, ERROR).
  # Disable this if you load worlds dynamically to prevent console spam.
  log_on_world_load: true
  version_notifications: true

# The default visibility distance for holograms.
visibility_distance: 20

# Whether the plugin should register its commands.
register_commands: true

# The interval at which hologram visibility is updated in ticks.
update_visibility_interval: 100
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_yields_the_original_defaults() {
        assert_eq!(Config::parse(""), Config::default());
    }

    #[test]
    fn reads_every_key() {
        let src = "\
saving:
  autosave:
    enabled: false
    interval: 5
  save_on_changed: false
logging:
  log_level: DEBUG
  log_on_world_load: false
  version_notifications: false
visibility_distance: 32
register_commands: false
update_visibility_interval: 40
";
        let c = Config::parse(src);
        assert!(!c.autosave_enabled);
        assert_eq!(c.autosave_interval, 5);
        assert!(!c.save_on_changed);
        assert_eq!(c.log_level, "DEBUG");
        assert!(!c.log_on_world_load);
        assert!(!c.version_notifications);
        assert_eq!(c.visibility_distance, 32);
        assert!(!c.register_commands);
        assert_eq!(c.update_visibility_interval, 40);
    }

    #[test]
    fn a_partial_config_keeps_defaults_for_missing_keys() {
        let c = Config::parse("visibility_distance: 64\n");
        assert_eq!(c.visibility_distance, 64);
        assert_eq!(c.autosave_interval, 15);
        assert_eq!(c.log_level, "INFO");
        assert!(c.register_commands);
    }

    #[test]
    fn malformed_yaml_degrades_to_defaults_rather_than_failing() {
        let c = Config::parse("\tnot: [valid\n  yaml: at all\n");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn unparseable_values_fall_back_individually() {
        let c =
            Config::parse("visibility_distance: soon\nsaving:\n  autosave:\n    interval: later\n");
        assert_eq!(c.visibility_distance, 20);
        assert_eq!(c.autosave_interval, 15);
    }

    #[test]
    fn the_shipped_default_file_parses_to_the_default_config() {
        assert_eq!(Config::parse(DEFAULT_CONFIG), Config::default());
    }
}
