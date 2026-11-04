//! Pumpkin entity and persistence integration.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::config::Config;
use crate::data::{Billboard, HologramData, LINE_WIDTH};
use crate::text as textmod;
use pumpkin_pie::PieClient;
use pumpkin_pie::protocol::ResolvedLine;
use pumpkin_plugin_api::display::TextAlignment as WitTextAlignment;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{
    BillboardMode, DisplayTransformation, Entity, EntityDisplayExt, EntityType, Quaternionf,
    Server, Vector3f, World,
};

/// Hidden, persistent entity marker used to remove leftovers after an unclean shutdown.
const MARKER: &str = "[pumpkin-holograms]";

/// The whole mutable plugin state, behind one lock.
pub struct State {
    pub config: Config,
    pub data_folder: PathBuf,
    /// Keyed by lowercased name, matching `HologramManagerImpl` (`:87`, `:96`).
    pub holograms: BTreeMap<String, HologramData>,
    /// The live entity for each spawned hologram.
    pub entities: BTreeMap<String, Entity>,
    /// Last periodically resolved text; unchanged values do not resend metadata.
    rendered: BTreeMap<String, String>,
    pub save_blocked: bool,
    pub dirty: bool,
}

impl State {
    pub fn new(config: Config, data_folder: PathBuf) -> Self {
        Self {
            config,
            data_folder,
            holograms: BTreeMap::new(),
            entities: BTreeMap::new(),
            rendered: BTreeMap::new(),
            save_blocked: false,
            dirty: false,
        }
    }

    /// The key a name is stored under — lowercased (`HologramManagerImpl:87`).
    pub fn key(name: &str) -> String {
        name.to_lowercase()
    }

    pub fn get(&self, name: &str) -> Option<&HologramData> {
        self.holograms.get(&Self::key(name))
    }

    /// Path of `holograms.yml` in the data folder.
    pub fn holograms_file(&self) -> PathBuf {
        self.data_folder.join(crate::store::FILE_NAME)
    }
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

/// Installs the state on load. Called once per load.
pub fn init(config: Config, data_folder: PathBuf) {
    let _ = STATE.set(Mutex::new(State::new(config, data_folder)));
}

/// Runs `f` with the state lock, recovering from poisoning. The lock is not reentrant.
pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let cell = STATE.get_or_init(|| Mutex::new(State::new(Config::default(), PathBuf::from("."))));
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// Resolves a world by name. Returns `None` when it is not loaded, which the
/// original treats as "cannot be created" (`Hologram1_20_4.create:61-63`).
pub fn world(server: &Server, name: &str) -> Option<World> {
    server.get_world_by_name(name)
}

/// Removes every entity this plugin owns.
///
/// Called on unload and before a reload replays `holograms.yml`, so the world
/// is not left with duplicates.
pub fn remove_all_entities() {
    let owned = with_state(|s| {
        s.rendered.clear();
        std::mem::take(&mut s.entities)
            .into_values()
            .collect::<Vec<_>>()
    });
    for entity in owned {
        entity.remove();
    }
}

/// Removes any entity carrying this plugin's marker, in every world.
///
/// Catches entities left behind by a crash or an unclean shutdown, which
/// [`remove_all_entities`] cannot see because it only knows about entities it
/// created in this session.
pub fn sweep_marked_entities(server: &Server) -> usize {
    let mut removed = 0;
    for world in server.get_all_worlds() {
        for entity in world.get_entities() {
            if entity
                .get_custom_name()
                .is_some_and(|name| name.get_text() == MARKER)
            {
                entity.remove();
                removed += 1;
            }
        }
    }
    removed
}

/// Spawns the entity for `data` and applies every descriptor.
pub fn spawn(server: &Server, data: &HologramData, default_visibility: i32) -> Option<Entity> {
    let world = location_world(server, &data.location)?;
    let entity = world.spawn_entity(EntityType::TextDisplay, data.location.position());

    // Real entities tick, unlike upstream's packet-only displays.
    entity.set_has_gravity(false);
    entity.set_custom_name(TextComponent::text(MARKER));
    entity.set_custom_name_visible(false);

    apply(server, data, &entity, default_visibility);
    Some(entity)
}

fn location_world(server: &Server, location: &crate::data::Location) -> Option<World> {
    match &location.dimension {
        Some(dimension) => server
            .get_all_worlds()
            .into_iter()
            .find(|world| world.get_id() == location.world && world.get_dimension() == *dimension),
        None => world(server, &location.world),
    }
}

/// Pushes every descriptor value from `data` onto `entity`.
///
/// Mirrors `Hologram1_20_4.update` (`:89-188`) for the text-display subset.
fn apply(server: &Server, data: &HologramData, entity: &Entity, default_visibility: i32) {
    let (Some(display), Some(text_display)) = (entity.as_display(), entity.as_text_display())
    else {
        tracing::warn!(
            "hologram '{}' is not a text display; skipping update",
            data.name
        );
        return;
    };

    // The original always sends an identity interpolation start and its own
    // duration (`Hologram1_20_4.java:74-78`). A duration of 0 means no
    // interpolation, so a moved hologram snaps rather than sliding.
    display.set_interpolation_duration(0);
    display.set_interpolation_start(0);

    display.set_billboard(match data.billboard {
        Billboard::Fixed => BillboardMode::Fixed,
        Billboard::Vertical => BillboardMode::Vertical,
        Billboard::Horizontal => BillboardMode::Horizontal,
        Billboard::Center => BillboardMode::Center,
    });

    display.set_transformation(DisplayTransformation {
        translation: Vector3f {
            x: data.translation.0,
            y: data.translation.1,
            z: data.translation.2,
        },
        scale: Vector3f {
            x: data.scale.0,
            y: data.scale.1,
            z: data.scale.2,
        },
        // Two identity quaternions, as the original passes
        // (`Hologram1_20_4.java:174-179`) — i.e. no display rotation.
        left_rotation: Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        right_rotation: Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
    });

    display.set_shadow_radius(data.shadow_radius);
    display.set_shadow_strength(data.shadow_strength);

    // View range is a client multiplier, not a per-player distance gate.
    display.set_view_range(data.effective_visibility_distance(default_visibility) as f32);

    if let Some(brightness) = data.brightness_argb() {
        display.set_brightness(brightness);
    }

    // Position and rotation. Upstream uses `setPosRaw`/`setYRot`/`setXRot`
    // (`Hologram1_20_4:101-103`); on a real entity that is `teleport`.
    if let Some(world) = location_world(server, &data.location) {
        entity.teleport(data.location.position(), world);
    }
    entity.set_rotation(data.location.yaw, data.location.pitch);

    // Text (`Hologram1_20_4.update:106-148`, `Hologram.getShownText:350-358`).
    text_display.set_line_width(LINE_WIDTH);
    text_display.set_background(data.background_argb() as i32);
    text_display.set_shadow(data.text_shadow);
    text_display.set_see_through(data.see_through);
    text_display.set_alignment(match data.text_alignment {
        crate::data::TextAlignment::Left => WitTextAlignment::Left,
        crate::data::TextAlignment::Right => WitTextAlignment::Right,
        crate::data::TextAlignment::Center => WitTextAlignment::Center,
    });

    let joined = textmod::legacy_ampersand_to_section(&textmod::join_lines(&data.text));
    let pie = PieClient::new();
    let resolved = pie
        .set_placeholders(None, &joined)
        .unwrap_or_else(|_| ResolvedLine {
            text: joined.clone(),
            unresolved: vec![],
        });
    text_display.set_text(TextComponent::from_legacy_string(&resolved.text));
    if joined.to_ascii_lowercase().contains("%player_") {
        tracing::warn!(
            "Hologram '{}' uses a player placeholder, but shared holograms have no viewer. Use server-wide placeholders; per-viewer text is not supported.",
            data.name
        );
    }
    with_state(|s| {
        s.rendered.insert(State::key(&data.name), resolved.text);
    });
}

/// Brings the world in line with `data`: spawns the entity if there is none,
/// otherwise re-applies every descriptor.
///
/// The single entry point every command path uses after mutating data.
pub fn sync(server: &Server, data: &HologramData) {
    let key = State::key(&data.name);
    let default_visibility = with_state(|s| s.config.visibility_distance);

    let needs_spawn = with_state(|s| !s.entities.contains_key(&key));
    if needs_spawn {
        match spawn(server, data, default_visibility) {
            Some(entity) => {
                with_state(|s| s.entities.insert(key, entity));
            }
            None => tracing::warn!(
                "Could not create hologram '{}': world '{}' is not loaded",
                data.name,
                data.location.world
            ),
        }
        return;
    }

    let entity = with_state(|s| s.entities.remove(&key));
    if let Some(entity) = entity {
        apply(server, data, &entity, default_visibility);
        with_state(|s| s.entities.insert(key, entity));
    }
}

/// Re-resolve shared placeholders after plugins have loaded and as values change.
pub fn refresh_placeholders() {
    let data = with_state(|s| {
        s.holograms
            .values()
            .filter(|hologram| hologram.text.iter().any(|line| line.contains('%')))
            .cloned()
            .collect::<Vec<_>>()
    });
    for hologram in data {
        let joined = textmod::legacy_ampersand_to_section(&textmod::join_lines(&hologram.text));
        let Ok(resolved) = PieClient::new().set_placeholders(None, &joined) else {
            continue;
        };
        let key = State::key(&hologram.name);
        let entity = with_state(|s| {
            if s.rendered.get(&key) == Some(&resolved.text) {
                None
            } else {
                s.entities.remove(&key)
            }
        });
        if let Some(entity) = entity {
            if let Some(display) = entity.as_text_display() {
                display.set_text(TextComponent::from_legacy_string(&resolved.text));
            }
            with_state(|s| {
                s.rendered.insert(key.clone(), resolved.text);
                s.entities.insert(key, entity);
            });
        }
    }
}

/// Despawns a hologram's entity and forgets it.
pub fn despawn(name: &str) {
    let entity = with_state(|s| {
        s.rendered.remove(&State::key(name));
        s.entities.remove(&State::key(name))
    });
    if let Some(entity) = entity {
        entity.remove();
    }
}

/// Re-reads `holograms.yml`, spawning an entity for each hologram.
///
/// Reload validates the complete file before removing existing entities.
pub fn load_from_disk(server: &Server, initial: bool) -> bool {
    let (mut report, path) = with_state(|s| {
        let path = s.holograms_file();
        let world_names = server
            .get_all_worlds()
            .iter()
            .map(|w| w.get_id())
            .collect::<Vec<_>>();
        (
            crate::store::load(&path, move |w| world_names.iter().any(|n| n == w)),
            path,
        )
    });

    for data in &report.holograms {
        if location_world(server, &data.location).is_none() {
            report.warnings.push(format!(
                "Could not load hologram '{}': world or dimension is not loaded",
                data.name
            ));
            report.save_blocked = true;
        }
    }

    for warning in &report.warnings {
        tracing::warn!("{warning}");
    }
    if report.save_blocked && !initial {
        with_state(|s| s.save_blocked = true);
        tracing::warn!(
            "Reload refused: holograms.yml was not fully loaded; existing state was retained"
        );
        return false;
    }
    if report.holograms.is_empty() && report.warnings.is_empty() {
        tracing::debug!("No holograms to load from {}", path.display());
    }

    if !initial {
        remove_all_entities();
        sweep_marked_entities(server);
    }
    with_state(|s| {
        s.holograms.clear();
        s.save_blocked = report.save_blocked;
        s.dirty = false;
    });
    let default_visibility = with_state(|s| s.config.visibility_distance);
    for data in report.holograms {
        let key = State::key(&data.name);
        match spawn(server, &data, default_visibility) {
            Some(entity) => with_state(|s| {
                s.entities.insert(key.clone(), entity);
                s.holograms.insert(key, data);
            }),
            None => {
                tracing::warn!(
                    "Could not create hologram '{}': world '{}' is not loaded",
                    data.name,
                    data.location.world
                );
                with_state(|s| s.save_blocked = true);
            }
        }
    }
    let count = with_state(|s| s.holograms.len());
    tracing::info!("Loaded {count} holograms");
    if with_state(|s| s.save_blocked) {
        tracing::warn!(
            "Hologram changes and saving are disabled until the storage problem is corrected and reloaded"
        );
    }
    true
}

/// Writes every hologram to `holograms.yml`.
pub fn save_to_disk() -> bool {
    if with_state(|s| s.save_blocked) {
        tracing::warn!("Refusing to overwrite an incompletely loaded holograms.yml");
        return false;
    }
    let (holograms, path) = with_state(|s| {
        (
            s.holograms.values().cloned().collect::<Vec<_>>(),
            s.holograms_file(),
        )
    });
    match crate::store::save(&path, &holograms) {
        Ok(()) => {
            with_state(|s| s.dirty = false);
            tracing::debug!("Saved {} holograms to {}", holograms.len(), path.display());
            true
        }
        Err(error) => {
            tracing::warn!("Could not save holograms to {}: {error}", path.display());
            false
        }
    }
}

/// Replaces the config and rewrites `config.yml` when it was missing.
pub fn load_config(data_folder: &std::path::Path) -> Config {
    let path = data_folder.join("config.yml");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| crate::config::DEFAULT_CONFIG.to_string());
    let config = Config::parse(&source);
    if !path.exists() {
        let _ = std::fs::create_dir_all(data_folder);
        if let Err(error) = std::fs::write(&path, crate::config::DEFAULT_CONFIG) {
            tracing::warn!("Could not write {}: {error}", path.display());
        }
    }
    config
}

/// Writes the embedded default config. Used when a reload finds no file.
pub fn write_default_config(data_folder: &std::path::Path) {
    let path = data_folder.join("config.yml");
    if !path.exists() {
        let _ = std::fs::create_dir_all(data_folder);
        let _ = std::fs::write(&path, crate::config::DEFAULT_CONFIG);
    }
}
