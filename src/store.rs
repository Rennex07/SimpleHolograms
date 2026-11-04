//! Hologram persistence to `holograms.yml` in the plugin data folder.
//! Writes are synchronous; incomplete loads disable saving to protect existing data.

use std::path::Path;

use crate::data::HologramData;
use crate::yaml::{self, Value};

/// The file name inside the plugin data folder.
pub const FILE_NAME: &str = "holograms.yml";

/// The only version this plugin will load. Anything else loads nothing
/// (`FlatFileHologramStorage.java:128-133`).
pub const CONFIG_VERSION: i32 = 2;

/// Outcome of a load: whatever was read, plus what to tell the operator about.
#[derive(Debug, Default, PartialEq)]
pub struct LoadReport {
    pub holograms: Vec<HologramData>,
    pub warnings: Vec<String>,
    /// Saving is unsafe when any part of an existing file could not be loaded.
    pub save_blocked: bool,
}

/// Reads every hologram in `path`.
///
/// `known_world` decides whether a hologram's world is loaded; one whose world
/// is missing is skipped with a warning, exactly as upstream.
pub fn load(path: &Path, known_world: impl Fn(&str) -> bool) -> LoadReport {
    let mut report = LoadReport::default();

    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return report,
        Err(error) => {
            report
                .warnings
                .push(format!("Could not read {}: {error}", path.display()));
            report.save_blocked = true;
            return report;
        }
    };
    let Ok(doc) = yaml::parse(&source) else {
        report
            .warnings
            .push("holograms.yml could not be parsed".to_string());
        report.save_blocked = true;
        return report;
    };

    let Some(section) = doc.get("holograms").filter(|v| matches!(v, Value::Map(_))) else {
        report
            .warnings
            .push("No holograms section found in config".to_string());
        report.save_blocked = true;
        return report;
    };

    // Upstream logs this twice, once for the version and once for "old config".
    let version = doc
        .get("version")
        .and_then(Value::as_str)
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(1);
    if version != CONFIG_VERSION {
        report
            .warnings
            .push("Config version is not 2, skipping loading holograms".to_string());
        report
            .warnings
            .push("Old config version detected, skipping loading holograms".to_string());
        report.save_blocked = true;
        return report;
    }

    let Value::Map(entries) = section else {
        return report;
    };
    let mut names = std::collections::BTreeSet::new();
    for (name, value) in entries {
        if !names.insert(name.to_lowercase()) {
            report.warnings.push(format!(
                "Duplicate hologram name '{name}' (case-insensitive)"
            ));
            continue;
        }
        if !matches!(value, Value::Map(_)) {
            report
                .warnings
                .push("Could not load hologram section in config".to_string());
            continue;
        }
        if value.get("type").and_then(Value::as_str).is_none() {
            report
                .warnings
                .push("HologramType was not saved".to_string());
            continue;
        }
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            continue;
        };
        if !kind.eq_ignore_ascii_case("TEXT") {
            // Unsupported item and block holograms are reported instead of dropped.
            report.warnings.push(format!(
                "Skipping hologram '{name}': type {kind} is not supported by this port"
            ));
            continue;
        }
        match HologramData::from_yaml(name, value, &known_world) {
            Ok(data) => report.holograms.push(data),
            Err(message) => report.warnings.push(message),
        }
    }
    report.save_blocked = !report.warnings.is_empty();
    report
}

/// Writes `holograms` to `path`, replacing the whole `holograms` section.
///
/// `version` is always written, with the upstream "DO NOT CHANGE" comment.
pub fn save(path: &Path, holograms: &[HologramData]) -> std::io::Result<()> {
    let entries = holograms
        .iter()
        .map(|h| (h.name.clone(), Value::Map(h.to_yaml())))
        .collect();

    let doc = Value::Map(vec![
        (
            "version".to_string(),
            Value::Scalar(CONFIG_VERSION.to_string()),
        ),
        ("holograms".to_string(), Value::Map(entries)),
    ]);
    // Bukkit attaches this as an inline comment on the `version` key, so it
    // has to land on that line rather than at the end of the document.
    let mut rendered = yaml::emit(&doc);
    if let Some(newline) = rendered.find('\n') {
        rendered.insert_str(newline, " # DO NOT CHANGE");
    }
    rendered.push('\n');

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Create a fresh sibling and replace only after the complete contents have
    // reached the filesystem. A failed write or rename leaves the old file intact.
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let (temp, mut file) = loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = path.with_extension(format!("yml.tmp-{id}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = file
        .write_all(rendered.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Location;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "pumpkin-holograms-test-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join(FILE_NAME)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn always(_: &str) -> bool {
        true
    }

    fn sample(name: &str) -> HologramData {
        HologramData::new(name, Location::new("world", 1.0, 2.0, 3.0, 0.0, 0.0))
    }

    #[test]
    fn missing_file_loads_nothing_without_warning() {
        let dir = TempDir::new("missing");
        let report = load(&dir.file(), always);
        assert!(report.holograms.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn empty_holograms_save_and_reload_without_warning() {
        let dir = TempDir::new("empty-roundtrip");
        save(&dir.file(), &[]).unwrap();
        let report = load(&dir.file(), always);
        assert!(report.holograms.is_empty());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn round_trips_holograms() {
        let dir = TempDir::new("roundtrip");
        let mut a = sample("alpha");
        a.text = vec!["hello".into(), "world".into()];
        let b = sample("beta");
        save(&dir.file(), &[a.clone(), b.clone()]).unwrap();

        let report = load(&dir.file(), always);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.holograms.len(), 2);
        assert_eq!(report.holograms[0].name, "alpha");
        assert_eq!(report.holograms[0].text, a.text);
        assert_eq!(report.holograms[0].location, a.location);
        assert_eq!(report.holograms[1].name, "beta");
    }

    #[test]
    fn dimension_survives_file_round_trip() {
        let dir = TempDir::new("dimension");
        let mut hologram = sample("nether");
        hologram.location.dimension = Some("minecraft:the_nether".into());
        save(&dir.file(), &[hologram]).unwrap();
        let report = load(&dir.file(), always);
        assert!(!report.save_blocked);
        assert_eq!(
            report.holograms[0].location.dimension.as_deref(),
            Some("minecraft:the_nether")
        );
    }

    #[test]
    fn saving_twice_does_not_accumulate_removed_holograms() {
        let dir = TempDir::new("replace");
        save(&dir.file(), &[sample("a"), sample("b")]).unwrap();
        save(&dir.file(), &[sample("a")]).unwrap();
        let report = load(&dir.file(), always);
        assert_eq!(report.holograms.len(), 1);
        assert_eq!(report.holograms[0].name, "a");
    }

    #[test]
    fn writes_the_version_marker_with_its_comment() {
        let dir = TempDir::new("version");
        save(&dir.file(), &[sample("a")]).unwrap();
        let raw = std::fs::read_to_string(dir.file()).unwrap();
        assert!(raw.starts_with("version: 2 # DO NOT CHANGE"), "{raw}");
    }

    #[test]
    fn a_wrong_version_loads_nothing_and_warns_twice() {
        let dir = TempDir::new("badversion");
        std::fs::write(dir.file(), "version: 1\nholograms:\n  a:\n    type: TEXT\n").unwrap();
        let report = load(&dir.file(), always);
        assert!(report.holograms.is_empty());
        assert_eq!(report.warnings.len(), 2, "{:?}", report.warnings);
        assert!(report.warnings[0].contains("not 2"));
        assert!(report.warnings[1].contains("Old config"));
    }

    #[test]
    fn a_missing_holograms_section_warns_once() {
        let dir = TempDir::new("nosection");
        std::fs::write(dir.file(), "version: 2\n").unwrap();
        let report = load(&dir.file(), always);
        assert!(report.holograms.is_empty());
        assert_eq!(
            report.warnings,
            vec!["No holograms section found in config"]
        );
    }

    #[test]
    fn a_missing_version_is_treated_as_one() {
        let dir = TempDir::new("nover");
        std::fs::write(dir.file(), "holograms:\n  a:\n    type: TEXT\n").unwrap();
        let report = load(&dir.file(), always);
        assert!(report.holograms.is_empty());
        assert_eq!(report.warnings.len(), 2);
    }

    #[test]
    fn a_missing_type_is_skipped_with_a_warning() {
        let dir = TempDir::new("notype");
        std::fs::write(
            dir.file(),
            "version: 2\nholograms:\n  a:\n    text:\n    - hi\n",
        )
        .unwrap();
        let report = load(&dir.file(), always);
        assert!(report.holograms.is_empty());
        assert_eq!(report.warnings, vec!["HologramType was not saved"]);
    }

    #[test]
    fn a_non_text_type_is_skipped_and_named() {
        let dir = TempDir::new("blocktype");
        std::fs::write(
            dir.file(),
            "version: 2\nholograms:\n  a:\n    type: BLOCK\n  b:\n    type: TEXT\n",
        )
        .unwrap();
        let report = load(&dir.file(), always);
        assert_eq!(report.holograms.len(), 1);
        assert_eq!(report.holograms[0].name, "b");
        assert!(report.warnings[0].contains("'a'"), "{:?}", report.warnings);
    }

    #[test]
    fn an_unloaded_world_skips_only_that_hologram() {
        let dir = TempDir::new("worlds");
        let raw = "\
version: 2
holograms:
  here:
    type: TEXT
    location:
      world: world
  gone:
    type: TEXT
    location:
      world: nether
";
        std::fs::write(dir.file(), raw).unwrap();
        let report = load(&dir.file(), |w| w == "world");
        assert_eq!(report.holograms.len(), 1);
        assert!(report.save_blocked);
        assert_eq!(report.holograms[0].name, "here");
        assert!(
            report.warnings[0].contains("'gone'"),
            "{:?}",
            report.warnings
        );
        assert!(
            report.warnings[0].contains("nether"),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn unsafe_loads_are_distinguished_from_missing_or_valid_files() {
        let dir = TempDir::new("unsafe-loads");
        assert!(!load(&dir.file(), always).save_blocked);
        // Reading a directory is an I/O error, not a missing store.
        assert!(load(&dir.0, always).save_blocked);
        for raw in [
            "not yaml",
            "version: 1\nholograms: {}\n",
            "version: 2\nholograms:\n  bad:\n    type: BLOCK\n",
        ] {
            std::fs::write(dir.file(), raw).unwrap();
            assert!(load(&dir.file(), always).save_blocked, "{raw}");
            assert_eq!(std::fs::read_to_string(dir.file()).unwrap(), raw);
        }
        std::fs::write(
            dir.file(),
            "version: 2\nholograms:\n  a:\n    type: TEXT\n  A:\n    type: TEXT\n",
        )
        .unwrap();
        assert!(load(&dir.file(), always).save_blocked);
        save(&dir.file(), &[sample("valid")]).unwrap();
        assert!(!load(&dir.file(), always).save_blocked);
    }

    #[test]
    fn failed_atomic_replacement_keeps_existing_destination() {
        let dir = TempDir::new("atomic-failure");
        let destination = dir.0.join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("keep"), "original").unwrap();
        assert!(save(&destination, &[sample("a")]).is_err());
        assert_eq!(
            std::fs::read_to_string(destination.join("keep")).unwrap(),
            "original"
        );
    }

    #[test]
    fn text_survives_a_save_and_reload_unchanged() {
        let dir = TempDir::new("text");
        let mut a = sample("fancy");
        let expected = vec![
            "&aGreen and plain".to_string(),
            "100% of: these".to_string(),
            "trailing space ".to_string(),
            String::new(),
        ];
        a.text = expected.clone();
        save(&dir.file(), &[a]).unwrap();
        let report = load(&dir.file(), always);
        assert_eq!(report.holograms[0].text, expected);
    }
}
