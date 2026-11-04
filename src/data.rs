//! The hologram data model and its YAML representation.

use crate::text;
use crate::yaml::{self, Value};

/// `Hologram.LINE_WIDTH` (`Hologram.java:33`) — forced on every update.
pub const LINE_WIDTH: i32 = 1000;

/// Billboard rendering constraints, matching `org.bukkit.entity.Display.Billboard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Billboard {
    #[default]
    Center,
    Fixed,
    Vertical,
    Horizontal,
}

impl Billboard {
    /// Parses the stored form. Unknown values fall back to `Center`, as in
    /// `DisplayHologramData.read` (`:151-157`).
    pub fn parse(raw: &str) -> Self {
        match raw.to_lowercase().as_str() {
            "fixed" => Self::Fixed,
            "vertical" => Self::Vertical,
            "horizontal" => Self::Horizontal,
            _ => Self::Center,
        }
    }

    /// The canonical stored form, lowercased (`DisplayHologramData.write:189`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Fixed => "fixed",
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
        }
    }
}

/// `org.bukkit.entity.TextDisplay.TextAlignment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlignment {
    Left,
    #[default]
    Center,
    Right,
}

impl TextAlignment {
    pub fn parse(raw: &str) -> Self {
        match raw.to_lowercase().as_str() {
            "left" => Self::Left,
            "right" => Self::Right,
            _ => Self::Center,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Center => "CENTER",
            Self::Right => "RIGHT",
        }
    }

    /// The stored form, lowercased (`TextHologramData.write:171`).
    pub fn stored(self) -> String {
        self.name().to_lowercase()
    }
}

/// A world position with rotation, the subset of `org.bukkit.Location` used here.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub world: String,
    /// Older files omit this and retain their historical world-name lookup.
    pub dimension: Option<String>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl Location {
    pub fn new(world: impl Into<String>, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> Self {
        Self {
            world: world.into(),
            dimension: None,
            x,
            y,
            z,
            yaw,
            pitch,
        }
    }

    pub fn in_dimension(mut self, dimension: String) -> Self {
        self.dimension = Some(dimension);
        self
    }

    fn with_optional_dimension(mut self, dimension: Option<String>) -> Self {
        self.dimension = dimension;
        self
    }

    pub fn position(&self) -> (f64, f64, f64) {
        (self.x, self.y, self.z)
    }
}

/// One text hologram's complete state.
#[derive(Debug, Clone, PartialEq)]
pub struct HologramData {
    pub name: String,
    pub location: Location,
    /// Stored raw. `<= 0` means "use the configured default"
    /// (`HologramData.getVisibilityDistance`, `:76-82`).
    pub visibility_distance: i32,
    /// Kept for file compatibility; per-player visibility is not supported.
    pub visibility: String,
    pub persistent: bool,
    pub linked_npc: Option<String>,

    pub scale: (f32, f32, f32),
    pub translation: (f32, f32, f32),
    pub shadow_radius: f32,
    pub shadow_strength: f32,
    /// `(block_light, sky_light)`, each clamped to 0..=15 on read
    /// (`DisplayHologramData.read:159-167`).
    pub brightness: Option<(i32, i32)>,
    pub billboard: Billboard,

    pub text: Vec<String>,
    pub text_shadow: bool,
    pub see_through: bool,
    pub text_alignment: TextAlignment,
    pub update_text_interval: i32,
    /// `None` = no override; [`TRANSPARENT`] = fully transparent.
    pub background: Option<u32>,
}

impl HologramData {
    /// A new text hologram with the original's defaults.
    ///
    /// The single default line is `TextHologramData`'s constructor
    /// (`:37`): `"Edit this line with /hologram edit <name>"`.
    pub fn new(name: impl Into<String>, location: Location) -> Self {
        let name = name.into();
        Self {
            text: vec![format!("Edit this line with /hologram edit {name}")],
            name,
            location,
            visibility_distance: -1,
            visibility: "ALL".to_string(),
            persistent: true,
            linked_npc: None,
            scale: (1.0, 1.0, 1.0),
            translation: (0.0, 0.0, 0.0),
            shadow_radius: 0.0,
            shadow_strength: 1.0,
            brightness: None,
            billboard: Billboard::Center,
            text_shadow: false,
            see_through: false,
            text_alignment: TextAlignment::Center,
            update_text_interval: -1,
            background: None,
        }
    }

    /// The configured visibility distance, or the config default when the
    /// stored value is not positive.
    pub fn effective_visibility_distance(&self, default: i32) -> i32 {
        if self.visibility_distance > 0 {
            self.visibility_distance
        } else {
            default
        }
    }

    /// The ARGB value to push to the display entity.
    ///
    /// `None` -> the vanilla default background; [`TRANSPARENT`] -> fully
    /// transparent (`Hologram1_20_4.java:114-121`).
    pub fn background_argb(&self) -> u32 {
        self.background.unwrap_or(text::INITIAL_BACKGROUND)
    }

    /// The packed brightness override, or `None` when unset.
    ///
    /// Packed the way vanilla stores it: block light in the high nibble, sky
    /// light four bits above that (`HologramImpl.java:113`).
    pub fn brightness_argb(&self) -> Option<i32> {
        self.brightness
            .map(|(block, sky)| (block << 4) | (sky << 20))
    }

    /// Serialises one hologram to its YAML section.
    ///
    /// `billboard` is written as null when `Center`, because that is what the
    /// original does (`DisplayHologramData.java:189`) — centre is the implicit
    /// default and the key is simply absent.
    pub fn to_yaml(&self) -> Vec<(String, Value)> {
        let s = |v: &str| Value::Scalar(v.to_string());
        let b = |v: bool| Value::Scalar(v.to_string());
        let n = |v: i32| Value::Scalar(v.to_string());

        let mut out: Vec<(String, Value)> = vec![("type".into(), s("TEXT"))];
        out.push((
            "location".into(),
            Value::Map(vec![
                ("world".into(), s(&self.location.world)),
                ("x".into(), Value::Scalar(yaml::fmt_f64(self.location.x))),
                ("y".into(), Value::Scalar(yaml::fmt_f64(self.location.y))),
                ("z".into(), Value::Scalar(yaml::fmt_f64(self.location.z))),
                (
                    "yaw".into(),
                    Value::Scalar(yaml::fmt_f64(f64::from(self.location.yaw))),
                ),
                (
                    "pitch".into(),
                    Value::Scalar(yaml::fmt_f64(f64::from(self.location.pitch))),
                ),
            ]),
        ));
        if let (Some(dimension), Some((_, Value::Map(location)))) = (
            &self.location.dimension,
            out.iter_mut().find(|(key, _)| key == "location"),
        ) {
            location.push(("dimension".into(), s(dimension)));
        }
        out.push(("visibility_distance".into(), n(self.visibility_distance)));
        out.push(("visibility".into(), s(&self.visibility)));
        out.push(("persistent".into(), b(self.persistent)));
        out.push((
            "linkedNpc".into(),
            self.linked_npc
                .as_ref()
                .map_or(Value::Scalar(String::new()), |v| s(v)),
        ));
        out.push((
            "scale_x".into(),
            Value::Scalar(yaml::fmt_f64(f64::from(self.scale.0))),
        ));
        out.push((
            "scale_y".into(),
            Value::Scalar(yaml::fmt_f64(f64::from(self.scale.1))),
        ));
        out.push((
            "scale_z".into(),
            Value::Scalar(yaml::fmt_f64(f64::from(self.scale.2))),
        ));
        out.push((
            "translation_x".into(),
            Value::Scalar(yaml::fmt_f64(f64::from(self.translation.0))),
        ));
        out.push((
            "translation_y".into(),
            Value::Scalar(yaml::fmt_f64(f64::from(self.translation.1))),
        ));
        out.push((
            "translation_z".into(),
            Value::Scalar(yaml::fmt_f64(f64::from(self.translation.2))),
        ));
        out.push((
            "shadow_radius".into(),
            Value::Scalar(yaml::fmt_f64(f64::from(self.shadow_radius))),
        ));
        out.push((
            "shadow_strength".into(),
            Value::Scalar(yaml::fmt_f64(f64::from(self.shadow_strength))),
        ));
        if let Some((block, sky)) = self.brightness {
            out.push(("block_brightness".into(), n(block)));
            out.push(("sky_brightness".into(), n(sky)));
        }
        out.push((
            "billboard".into(),
            if self.billboard == Billboard::Center {
                Value::Scalar(String::new())
            } else {
                s(self.billboard.as_str())
            },
        ));
        out.push((
            "text".into(),
            Value::List(self.text.iter().map(|l| s(l)).collect()),
        ));
        out.push(("text_shadow".into(), b(self.text_shadow)));
        out.push(("see_through".into(), b(self.see_through)));
        out.push((
            "text_alignment".into(),
            Value::Scalar(self.text_alignment.stored()),
        ));
        out.push(("update_text_interval".into(), n(self.update_text_interval)));
        out.push((
            "background".into(),
            text::format_background(self.background)
                .map_or(Value::Scalar(String::new()), |c| s(&c)),
        ));
        out
    }

    /// Reads one hologram from its YAML section.
    ///
    /// Returns `Err` when the world is not loaded, matching
    /// `HologramData.read` (`:146-151`) — the caller logs and skips.
    ///
    /// Coordinates go through `f32` on the way in, as they do upstream
    /// (`:140-144`), so a stored `0.1` comes back as `0.1f32 as f64`.
    pub fn from_yaml(
        name: &str,
        section: &Value,
        known_world: impl Fn(&str) -> bool,
    ) -> Result<Self, String> {
        let loc = section.get("location");
        let world = loc
            .and_then(|l| l.get("world"))
            .and_then(Value::as_str)
            .unwrap_or("world")
            .to_string();
        if !known_world(&world) {
            return Err(format!(
                "Could not load hologram '{name}', because the world '{world}' is not loaded"
            ));
        }

        // `location.*` holds the coordinates; every other key sits at the top
        // level of the section.
        let coord = |key: &str, default: f64| -> f64 {
            loc.and_then(|l| l.get(key))
                .and_then(Value::as_str)
                .and_then(text::parse_double)
                .unwrap_or(default)
        };
        let d = |key: &str, default: f64| -> f64 {
            section
                .get(key)
                .and_then(Value::as_str)
                .and_then(text::parse_double)
                .unwrap_or(default)
        };
        let i = |key: &str, default: i32| -> i32 {
            section
                .get(key)
                .and_then(Value::as_str)
                .and_then(text::parse_double)
                .map_or(default, |v| v as i32)
        };
        let b = |key: &str, default: bool| -> bool {
            section
                .get(key)
                .and_then(Value::as_str)
                .and_then(text::parse_bool)
                .unwrap_or(default)
        };

        let mut text_lines: Vec<String> = section
            .get("text")
            .and_then(Value::as_list)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if text_lines.is_empty() {
            // `TextHologramData.read:132-135`, including its own TODO about
            // whether this should be a failure.
            text_lines = vec!["Could not load hologram text".to_string()];
        }

        // `Math.min(15, ...)` on each channel; either channel being set makes
        // brightness present, and each is floored at 0.
        let block = i("block_brightness", -1).min(15);
        let sky = i("sky_brightness", -1).min(15);
        let brightness = if block > -1 || sky > -1 {
            Some((block.max(0), sky.max(0)))
        } else {
            None
        };

        let visibility = section
            .get("visibility")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map_or("ALL".to_string(), str::to_string);

        Ok(Self {
            name: name.to_string(),
            location: Location::new(
                world,
                coord("x", 0.0) as f32 as f64,
                coord("y", 0.0) as f32 as f64,
                coord("z", 0.0) as f32 as f64,
                coord("yaw", 0.0) as f32,
                coord("pitch", 0.0) as f32,
            )
            .with_optional_dimension(
                loc.and_then(|l| l.get("dimension"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            ),
            visibility_distance: i("visibility_distance", -1),
            visibility,
            persistent: b("persistent", true),
            linked_npc: section
                .get("linkedNpc")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            scale: (
                d("scale_x", 1.0) as f32,
                d("scale_y", 1.0) as f32,
                d("scale_z", 1.0) as f32,
            ),
            translation: (
                d("translation_x", 0.0) as f32,
                d("translation_y", 0.0) as f32,
                d("translation_z", 0.0) as f32,
            ),
            shadow_radius: d("shadow_radius", 0.0) as f32,
            shadow_strength: d("shadow_strength", 1.0) as f32,
            brightness,
            billboard: section
                .get("billboard")
                .and_then(Value::as_str)
                .map_or(Billboard::Center, Billboard::parse),
            text: text_lines,
            text_shadow: b("text_shadow", false),
            see_through: b("see_through", false),
            text_alignment: section
                .get("text_alignment")
                .and_then(Value::as_str)
                .map_or(TextAlignment::Center, TextAlignment::parse),
            update_text_interval: i("update_text_interval", -1),
            background: text::parse_background(
                section
                    .get("background")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty()),
            ),
        })
    }
}

/// Resolves one coordinate argument, honouring the `~` / `~~` relative forms.
///
/// Port of `MoveHereCMD.calculateCoordinate` (`:57-70`):
///
/// * `12`    — absolute
/// * `~5`    — 5 more than the hologram's current value
/// * `~~5`   — 5 more than the caller's value
/// * `~`     — the hologram's current value
/// * `~~`    — the caller's value
///
/// Anything else is `None`, which the caller reports as "Could not parse
/// position".
///
/// Like upstream, extra leading tildes are stripped; two or more use the caller as the base.
pub fn calculate_coordinate(text: &str, origin: f64, caller: f64) -> Option<f64> {
    let stripped = text.trim_start_matches('~');
    let base = if text.starts_with("~~") {
        Some(caller)
    } else if text.starts_with('~') {
        Some(origin)
    } else {
        None
    };

    let Some(number) = crate::text::parse_double(stripped) else {
        // A bare `~` or `~~`: the value itself, with nothing to add.
        return if stripped.is_empty() {
            base.filter(|value| value.is_finite())
        } else {
            None
        };
    };
    if !number.is_finite() {
        return None;
    }
    let coordinate = match base {
        Some(base) => number + base,
        None => number,
    };
    coordinate.is_finite().then_some(coordinate)
}

/// Returns the zero-based starting offset and number of pages for a list page.
pub fn page_bounds(count: usize, page: usize) -> Option<(usize, usize)> {
    let pages = count.div_ceil(10);
    if page == 0 || page > pages {
        None
    } else {
        Some(((page - 1) * 10, pages))
    }
}

/// Splits a command tail into its first word and the rest of the line.
///
/// The first word is the hologram's name and the rest is the text or the
/// coordinates, which is how Bukkit's plain `Command` splits its arguments: on
/// whitespace alone, with no restriction on the characters a word may hold.
///
/// It is a function rather than a command-tree argument because Brigadier's
/// unquoted-word charset is `[0-9A-Za-z_.+-]`: a `single-word` argument cannot
/// hold `&4welcome` or `spawn:hologram`, and a name may hold either.
pub fn split_first(tail: &str) -> (Option<String>, String) {
    let tail = tail.trim_start();
    match tail.find(char::is_whitespace) {
        Some(index) => (
            Some(tail[..index].to_string()),
            tail[index..].trim_start().to_string(),
        ),
        None if tail.is_empty() => (None, String::new()),
        None => (Some(tail.to_string()), String::new()),
    }
}

/// One of `/hologram edit`'s text-line operations.
///
/// Upstream exposes these through its `edit` dispatcher (`HologramCMD:292`)
/// rather than as commands of their own, so they share this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEdit {
    Add,
    Set,
    Remove,
    InsertBefore,
    InsertAfter,
}

impl LineEdit {
    /// Parses the option word. Upstream lowercases it (`HologramCMD:293`).
    pub fn parse(option: &str) -> Option<Self> {
        Some(match option.to_lowercase().as_str() {
            "addline" => Self::Add,
            "setline" => Self::Set,
            "removeline" => Self::Remove,
            "insertbefore" => Self::InsertBefore,
            "insertafter" => Self::InsertAfter,
            _ => return None,
        })
    }
}

/// Why a line operation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEditError {
    /// A 1-based line number outside the range the operation accepts.
    InvalidIndex,
    /// No text was supplied where one is required.
    MissingText,
}

/// Applies one text-line operation to `lines`.
///
/// `number` is the 1-based line and is ignored by [`LineEdit::Add`]. Returns the
/// 1-based line the success message names, which is the line that now holds the
/// text rather than always the number typed: `setline` past the end appends, and
/// removing the last line names the new last one, as `SetLineCMD:55` does.
///
/// The index rules are the original's (`SetLineCMD:34-40`,
/// `InsertBeforeCMD:44-64`, `InsertAfterCMD:44-62`):
///
/// * `setline` past the end appends.
/// * `insertbefore` and `insertafter` clamp to the end.
/// * `insertafter 0` inserts at the top; the original only rejects a negative
///   index there, while `insertbefore` rejects it.
///
/// Unlike upstream, `removeline` past the end is refused rather than appending.
pub fn edit_line(
    lines: &mut Vec<String>,
    edit: LineEdit,
    number: i32,
    text: &str,
) -> Result<usize, LineEditError> {
    // `insertafter 0` is the only operation that accepts a zero.
    let minimum = if edit == LineEdit::InsertAfter { 0 } else { 1 };
    if edit != LineEdit::Add && number < minimum {
        return Err(LineEditError::InvalidIndex);
    }

    match edit {
        LineEdit::Add => {
            if text.is_empty() {
                return Err(LineEditError::MissingText);
            }
            lines.push(text.to_string());
            Ok(lines.len())
        }
        LineEdit::Set => {
            if text.is_empty() {
                return Err(LineEditError::MissingText);
            }
            let index = number as usize - 1;
            if index >= lines.len() {
                lines.push(text.to_string());
                Ok(lines.len())
            } else {
                lines[index] = text.to_string();
                Ok(index + 1)
            }
        }
        LineEdit::Remove => {
            let index = number as usize - 1;
            if index >= lines.len() {
                return Err(LineEditError::InvalidIndex);
            }
            lines.remove(index);
            Ok(index.min(lines.len().saturating_sub(1)) + 1)
        }
        LineEdit::InsertBefore | LineEdit::InsertAfter => {
            if text.is_empty() {
                return Err(LineEditError::MissingText);
            }
            let index = if edit == LineEdit::InsertAfter {
                number as usize
            } else {
                number as usize - 1
            };
            let index = index.min(lines.len());
            lines.insert(index, text.to_string());
            Ok(index + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(_: &str) -> bool {
        true
    }

    fn lines() -> Vec<String> {
        vec!["first".to_string(), "second".to_string()]
    }

    #[test]
    fn edit_option_names_are_case_insensitive() {
        assert_eq!(LineEdit::parse("addLine"), Some(LineEdit::Add));
        assert_eq!(LineEdit::parse("SETLINE"), Some(LineEdit::Set));
        assert_eq!(LineEdit::parse("removeLine"), Some(LineEdit::Remove));
        assert_eq!(
            LineEdit::parse("insertBefore"),
            Some(LineEdit::InsertBefore)
        );
        assert_eq!(LineEdit::parse("insertAfter"), Some(LineEdit::InsertAfter));
        assert_eq!(LineEdit::parse("scale"), None);
    }

    #[test]
    fn addline_appends_and_names_the_new_line() {
        let mut list = lines();
        assert_eq!(edit_line(&mut list, LineEdit::Add, 0, "third"), Ok(3));
        assert_eq!(list, ["first", "second", "third"]);
    }

    #[test]
    fn addline_needs_text() {
        let mut list = lines();
        assert_eq!(
            edit_line(&mut list, LineEdit::Add, 0, ""),
            Err(LineEditError::MissingText)
        );
        assert_eq!(list, ["first", "second"]);
    }

    #[test]
    fn setline_replaces_in_place() {
        let mut list = lines();
        assert_eq!(edit_line(&mut list, LineEdit::Set, 1, "changed"), Ok(1));
        assert_eq!(list, ["changed", "second"]);
    }

    /// `SetLineCMD` treats an index past the end as an append.
    #[test]
    fn setline_past_the_end_appends() {
        let mut list = lines();
        assert_eq!(edit_line(&mut list, LineEdit::Set, 9, "third"), Ok(3));
        assert_eq!(list, ["first", "second", "third"]);
    }

    #[test]
    fn setline_rejects_zero_and_needs_text() {
        let mut list = lines();
        assert_eq!(
            edit_line(&mut list, LineEdit::Set, 0, "x"),
            Err(LineEditError::InvalidIndex)
        );
        assert_eq!(
            edit_line(&mut list, LineEdit::Set, 1, ""),
            Err(LineEditError::MissingText)
        );
        assert_eq!(list, ["first", "second"]);
    }

    #[test]
    fn removeline_removes_and_names_the_line_now_in_its_slot() {
        let mut list = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(edit_line(&mut list, LineEdit::Remove, 2, ""), Ok(2));
        assert_eq!(list, ["a", "c"]);
        // Removing the last line names the new last line, as upstream does.
        assert_eq!(edit_line(&mut list, LineEdit::Remove, 2, ""), Ok(1));
        assert_eq!(list, ["a"]);
    }

    /// Upstream shares the append branch here and adds a blank line; this
    /// refuses instead, because a typo must not silently grow the hologram.
    #[test]
    fn removeline_past_the_end_is_refused_without_touching_the_list() {
        let mut list = lines();
        assert_eq!(
            edit_line(&mut list, LineEdit::Remove, 3, ""),
            Err(LineEditError::InvalidIndex)
        );
        assert_eq!(list, ["first", "second"]);
    }

    #[test]
    fn insertbefore_puts_the_line_at_the_requested_position() {
        let mut list = lines();
        assert_eq!(
            edit_line(&mut list, LineEdit::InsertBefore, 2, "new"),
            Ok(2)
        );
        assert_eq!(list, ["first", "new", "second"]);
        assert_eq!(
            edit_line(&mut list, LineEdit::InsertBefore, 1, "top"),
            Ok(1)
        );
        assert_eq!(list, ["top", "first", "new", "second"]);
    }

    #[test]
    fn insertbefore_clamps_past_the_end_and_rejects_zero() {
        let mut list = lines();
        assert_eq!(
            edit_line(&mut list, LineEdit::InsertBefore, 0, "x"),
            Err(LineEditError::InvalidIndex)
        );
        assert_eq!(
            edit_line(&mut list, LineEdit::InsertBefore, 99, "end"),
            Ok(3)
        );
        assert_eq!(list, ["first", "second", "end"]);
    }

    #[test]
    fn insertafter_inserts_after_the_named_line() {
        let mut list = lines();
        assert_eq!(edit_line(&mut list, LineEdit::InsertAfter, 1, "new"), Ok(2));
        assert_eq!(list, ["first", "new", "second"]);
        // `InsertAfterCMD` only rejects a negative index, so zero means "top".
        assert_eq!(edit_line(&mut list, LineEdit::InsertAfter, 0, "top"), Ok(1));
        assert_eq!(list, ["top", "first", "new", "second"]);
        assert_eq!(
            edit_line(&mut list, LineEdit::InsertAfter, 99, "end"),
            Ok(5)
        );
        assert_eq!(list, ["top", "first", "new", "second", "end"]);
    }

    #[test]
    fn inserts_need_text() {
        let mut list = lines();
        assert_eq!(
            edit_line(&mut list, LineEdit::InsertBefore, 1, ""),
            Err(LineEditError::MissingText)
        );
        assert_eq!(
            edit_line(&mut list, LineEdit::InsertAfter, 1, ""),
            Err(LineEditError::MissingText)
        );
        assert_eq!(list, ["first", "second"]);
    }

    fn sample() -> HologramData {
        HologramData::new(
            "welcome",
            Location::new("world", 1.0, 64.0, -3.5, 90.0, 10.0),
        )
    }

    #[test]
    fn a_tail_splits_into_its_first_word_and_the_rest() {
        assert_eq!(
            split_first("&4welcome Hello there"),
            (Some("&4welcome".to_string()), "Hello there".to_string())
        );
        assert_eq!(
            split_first("welcome 10 64 20"),
            (Some("welcome".to_string()), "10 64 20".to_string())
        );
        assert_eq!(
            split_first("welcome"),
            (Some("welcome".to_string()), String::new())
        );
        assert_eq!(split_first(""), (None, String::new()));
    }

    #[test]
    fn new_hologram_has_the_original_default_line() {
        let d = sample();
        assert_eq!(d.text, vec!["Edit this line with /hologram edit welcome"]);
        assert_eq!(d.scale, (1.0, 1.0, 1.0));
        assert_eq!(d.billboard, Billboard::Center);
        assert_eq!(d.shadow_strength, 1.0);
        assert_eq!(d.update_text_interval, -1);
        assert!(d.persistent);
    }

    #[test]
    fn round_trips_through_yaml() {
        let mut d = sample();
        d.location.dimension = Some("minecraft:the_nether".into());
        d.text = vec!["line one".into(), "it's got a quote".into()];
        d.billboard = Billboard::Fixed;
        d.text_alignment = TextAlignment::Right;
        d.text_shadow = true;
        d.see_through = true;
        d.brightness = Some((15, 7));
        d.scale = (2.0, 2.0, 3.0);
        d.shadow_radius = 4.5;
        d.visibility_distance = 32;

        let section = Value::Map(d.to_yaml());
        let mut back = HologramData::from_yaml("welcome", &section, known).unwrap();
        back.name = d.name.clone();
        assert_eq!(back.text, d.text);
        assert_eq!(back.billboard, Billboard::Fixed);
        assert_eq!(back.text_alignment, TextAlignment::Right);
        assert!(back.text_shadow);
        assert!(back.see_through);
        assert_eq!(back.brightness, Some((15, 7)));
        assert_eq!(back.scale, d.scale);
        assert_eq!(back.shadow_radius, d.shadow_radius);
        assert_eq!(back.visibility_distance, 32);
        assert_eq!(back.location.world, "world");
        assert_eq!(back.location.dimension, d.location.dimension);
    }

    #[test]
    fn centre_billboard_is_written_as_null() {
        let section = Value::Map(sample().to_yaml());
        assert!(section.get("billboard").unwrap().is_null());

        let mut d = sample();
        d.billboard = Billboard::Vertical;
        let section = Value::Map(d.to_yaml());
        assert_eq!(section.get("billboard").unwrap().as_str(), Some("vertical"));
    }

    #[test]
    fn missing_section_fields_take_the_original_defaults() {
        let section = Value::Map(vec![("type".into(), Value::Scalar("TEXT".into()))]);
        let d = HologramData::from_yaml("welcome", &section, known).unwrap();
        assert_eq!(d.scale, (1.0, 1.0, 1.0));
        assert_eq!(d.shadow_strength, 1.0);
        assert_eq!(d.visibility_distance, -1);
        assert_eq!(d.visibility, "ALL");
        assert!(d.persistent);
        assert_eq!(d.billboard, Billboard::Center);
    }

    #[test]
    fn empty_text_falls_back_to_the_originals_placeholder() {
        let section = Value::Map(vec![("text".into(), Value::List(vec![]))]);
        let d = HologramData::from_yaml("welcome", &section, known).unwrap();
        assert_eq!(d.text, vec!["Could not load hologram text"]);
    }

    #[test]
    fn unloaded_world_is_an_error_not_a_default() {
        let section = Value::Map(vec![(
            "location".into(),
            Value::Map(vec![("world".into(), Value::Scalar("nether".into()))]),
        )]);
        assert!(HologramData::from_yaml("welcome", &section, |w| w == "world").is_err());
    }

    #[test]
    fn brightness_is_clamped_and_detected() {
        let mk = |block: &str, sky: &str| {
            Value::Map(vec![
                ("block_brightness".into(), Value::Scalar(block.into())),
                ("sky_brightness".into(), Value::Scalar(sky.into())),
            ])
        };
        assert_eq!(
            HologramData::from_yaml("welcome", &mk("20", "99"), known)
                .unwrap()
                .brightness,
            Some((15, 15))
        );
        // Both channels absent stays off; `Math.max(0, ..)` only applies to a
        // channel that is absent while the other is set
        // (`DisplayHologramData.read:159-167`).
        assert_eq!(
            HologramData::from_yaml("welcome", &mk("-5", "-5"), known)
                .unwrap()
                .brightness,
            None
        );
        // Either channel alone is enough to switch brightness on.
        assert_eq!(
            HologramData::from_yaml("welcome", &mk("5", "-1"), known)
                .unwrap()
                .brightness,
            Some((5, 0))
        );
        assert_eq!(
            HologramData::from_yaml("welcome", &mk("-9", "3"), known)
                .unwrap()
                .brightness,
            Some((0, 3))
        );
        assert_eq!(
            HologramData::from_yaml("welcome", &mk("-1", "-1"), known)
                .unwrap()
                .brightness,
            None
        );
    }

    #[test]
    fn visibility_distance_falls_back_to_the_config_default() {
        let d = sample();
        assert_eq!(d.effective_visibility_distance(20), 20);
        let mut d2 = d.clone();
        d2.visibility_distance = 50;
        assert_eq!(d2.effective_visibility_distance(20), 50);
        d2.visibility_distance = 0;
        assert_eq!(d2.effective_visibility_distance(20), 20);
    }

    #[test]
    fn background_argb_defaults_and_transparent() {
        let mut d = sample();
        assert_eq!(d.background_argb(), 1_073_741_824);
        d.background = Some(crate::text::TRANSPARENT);
        assert_eq!(d.background_argb(), 0);
    }

    #[test]
    fn coordinates_go_through_f32_on_read_as_upstream_does() {
        let section = Value::Map(vec![(
            "location".into(),
            Value::Map(vec![("x".into(), Value::Scalar("0.1".into()))]),
        )]);
        let d = HologramData::from_yaml("welcome", &section, known).unwrap();
        // 0.1 is not representable in f32; upstream widens it the same way.
        assert_eq!(d.location.x, 0.1f32 as f64);
        assert_ne!(d.location.x, 0.1);
    }

    #[test]
    fn billboard_parses_case_insensitively_with_a_centre_fallback() {
        assert_eq!(Billboard::parse("FIXED"), Billboard::Fixed);
        assert_eq!(Billboard::parse("horizontal"), Billboard::Horizontal);
        assert_eq!(Billboard::parse("nonsense"), Billboard::Center);
    }

    #[test]
    fn absolute_coordinates() {
        assert_eq!(calculate_coordinate("10", 1.0, 2.0), Some(10.0));
        assert_eq!(calculate_coordinate("-3.5", 1.0, 2.0), Some(-3.5));
        assert_eq!(calculate_coordinate("0", 1.0, 2.0), Some(0.0));
    }

    #[test]
    fn page_bounds_cover_all_items_without_empty_extra_pages() {
        assert_eq!(page_bounds(0, 1), None);
        assert_eq!(page_bounds(10, 1), Some((0, 1)));
        assert_eq!(page_bounds(10, 2), None);
        assert_eq!(page_bounds(11, 2), Some((10, 2)));
        assert_eq!(page_bounds(20, 2), Some((10, 2)));
        assert_eq!(page_bounds(20, 3), None);
        assert_eq!(page_bounds(20, usize::MAX), None);
    }

    #[test]
    fn single_tilde_is_relative_to_the_hologram() {
        assert_eq!(calculate_coordinate("~5", 100.0, 2.0), Some(105.0));
        assert_eq!(calculate_coordinate("~-5", 100.0, 2.0), Some(95.0));
    }

    #[test]
    fn double_tilde_is_relative_to_the_caller() {
        assert_eq!(calculate_coordinate("~~5", 100.0, 2.0), Some(7.0));
    }

    #[test]
    fn a_bare_tilde_resolves_to_the_base_itself() {
        assert_eq!(calculate_coordinate("~", 100.0, 2.0), Some(100.0));
        assert_eq!(calculate_coordinate("~~", 100.0, 2.0), Some(2.0));
    }

    /// Upstream strips every leading tilde but only distinguishes `~~` from `~`,
    /// so extra tildes are silently ignored. Reproduced, not tidied.
    #[test]
    fn extra_leading_tildes_are_stripped_and_stay_caller_relative() {
        assert_eq!(calculate_coordinate("~~~5", 100.0, 2.0), Some(7.0));
    }

    #[test]
    fn unparseable_coordinates_are_rejected() {
        assert_eq!(calculate_coordinate("abc", 1.0, 2.0), None);
        assert_eq!(calculate_coordinate("", 1.0, 2.0), None);
        assert_eq!(calculate_coordinate("~typo", 1.0, 2.0), None);
        assert_eq!(calculate_coordinate("~~oops", 1.0, 2.0), None);
        assert_eq!(calculate_coordinate("NaN", 1.0, 2.0), None);
        assert_eq!(calculate_coordinate("~1e308", 1e308, 2.0), None);
        assert_eq!(calculate_coordinate("~", f64::INFINITY, 2.0), None);
    }
}
