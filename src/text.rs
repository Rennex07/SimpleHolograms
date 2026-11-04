//! Legacy text formatting and hologram background colours.

/// The 16 Minecraft named text colours and their RGB values.
///
/// Adventure's `NamedTextColor.NAMES`, which the original looks colours up in
/// (`TextHologramData.java:158`). Both the `_gray` and `_grey` spellings are
/// present upstream, so both are accepted here.
const NAMED_COLOURS: &[(&str, u32)] = &[
    ("black", 0x00_00_00),
    ("dark_blue", 0x00_00_AA),
    ("dark_green", 0x00_AA_00),
    ("dark_aqua", 0x00_AA_AA),
    ("dark_red", 0xAA_00_00),
    ("dark_purple", 0xAA_00_AA),
    ("gold", 0xFF_AA_00),
    ("gray", 0xAA_AA_AA),
    ("grey", 0xAA_AA_AA),
    ("dark_gray", 0x55_55_55),
    ("dark_grey", 0x55_55_55),
    ("blue", 0x55_55_FF),
    ("green", 0x55_FF_55),
    ("aqua", 0x55_FF_FF),
    ("red", 0xFF_55_55),
    ("light_purple", 0xFF_55_FF),
    ("yellow", 0xFF_FF_55),
    ("white", 0xFF_FF_FF),
];

/// Colour and formatting codes the legacy parser understands.
const LEGACY_CODES: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'k', 'l', 'm',
    'n', 'o', 'r',
];

/// Rewrites `&`-prefixed colour codes to the section sign the host parser wants.
///
/// Only recognised codes are rewritten: the host parser otherwise consumes
/// the character following a code symbol even when it is not a valid code.
///
/// A code and the text it colours stay in one string: the host parser emits
/// nothing at all for a bare code, so splitting them would drop the styling.
pub fn legacy_ampersand_to_section(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&'
            && i + 1 < chars.len()
            && LEGACY_CODES.contains(&chars[i + 1].to_ascii_lowercase())
        {
            out.push('\u{a7}');
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The default text-display background, `TextDisplay.INITIAL_BACKGROUND`.
///
/// 0x40000000 = 1073741824 (`HologramImpl.java:78`, `Hologram1_20_4.java:116`).
pub const INITIAL_BACKGROUND: u32 = 1_073_741_824;

/// Fully transparent, `Color.fromARGB(0)` (`Hologram.java:34`).
pub const TRANSPARENT: u32 = 0;

/// Reads a stored background colour into a packed ARGB value.
///
/// `None` means "no override", which is distinct from [`TRANSPARENT`].
///
/// Reproduces the upstream alpha quirk: a colour named in the config is ORed
/// with `0xC8000000` (`TextHologramData.java:158`), so it comes back with
/// alpha 200 rather than 255. This is why the original cannot round-trip a
/// named background — see [`format_background`].
pub fn parse_background(raw: Option<&str>) -> Option<u32> {
    let raw = raw?;
    if raw.eq_ignore_ascii_case("transparent") {
        return Some(TRANSPARENT);
    }

    if let Some(hex) = raw.strip_prefix('#') {
        // `Long.parseLong(hex, 16)` upstream: up to 16 hex digits, no `0x`.
        let value = u32::from_str_radix(hex, 16).ok()?;
        let argb = if hex.len() > 6 {
            value
        } else {
            // Backwards compatibility, quoted from the original: a bare #RRGGBB
            // has alpha 0 by default, so make it solid
            // (`TextHologramData.java:156`).
            value | 0xFF00_0000
        };
        return Some(argb);
    }

    // `.toLowerCase().trim().replace(' ', '_')` upstream.
    let key = raw.to_lowercase().trim().replace(' ', "_");
    let rgb = NAMED_COLOURS.iter().find(|(name, _)| *name == key)?.1;
    // The alpha 0xC8 is upstream's, not a choice made here.
    Some(rgb | 0xC800_0000)
}

/// Renders a background colour back to its stored form.
///
/// Mirrors `TextHologramData.write` (`:174-184`). A colour with alpha 255 that
/// happens to be a named colour is written as that name; everything else is
/// written as `#aarrggbb`.
///
/// Named colours acquire alpha 0xC8, so they are written as `#c8...` and an
/// unquoted YAML value then reads back as null, matching upstream.
pub fn format_background(argb: Option<u32>) -> Option<String> {
    let argb = argb?;
    if argb == TRANSPARENT {
        return Some("transparent".to_string());
    }
    if (argb >> 24) == 0xFF {
        let rgb = argb & 0x00FF_FFFF;
        if let Some((name, _)) = NAMED_COLOURS.iter().find(|(_, v)| *v == rgb) {
            // Normalise the `_grey` aliases back to the canonical spelling.
            let canonical = match *name {
                "grey" => "gray",
                "dark_grey" => "dark_gray",
                other => other,
            };
            return Some(canonical.to_string());
        }
    }
    Some(format!("#{argb:08x}"))
}

/// Looks up a named colour's RGB value, for tests and command parsing.
pub fn named_rgb(name: &str) -> Option<u32> {
    let key = name.to_lowercase().trim().replace(' ', "_");
    NAMED_COLOURS
        .iter()
        .find(|(n, _)| *n == key)
        .map(|(_, v)| *v)
}

/// Joins a hologram's lines the way `Hologram.getShownText` does
/// (`Hologram.java:355`).
pub fn join_lines(lines: &[String]) -> String {
    lines.join("\n")
}

/// Parses a boolean the way the original's command handlers do, from the exact
/// set of literals the commands advertise (`true` / `false`).
pub fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Parses a floating-point number.
///
/// The original uses Guava's `Doubles.tryParse`, which also accepts Java-only
/// forms (`1d`, `1f`, `0x1p3`). Rust's parser is stricter; a value that Java
/// would accept and this rejects is reported as unparseable rather than
/// guessed at.
pub fn parse_double(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_known_ampersand_codes() {
        assert_eq!(
            legacy_ampersand_to_section("&aRed &lBold"),
            "\u{a7}aRed \u{a7}lBold"
        );
    }

    #[test]
    fn leaves_ampersand_before_a_non_code_alone() {
        // The host parser would eat both the '&' and the space here.
        assert_eq!(legacy_ampersand_to_section("Fish & Chips"), "Fish & Chips");
        assert_eq!(legacy_ampersand_to_section("&HI"), "&HI");
    }

    #[test]
    fn accepts_uppercase_codes_like_paper_does() {
        assert_eq!(legacy_ampersand_to_section("&CRed"), "\u{a7}CRed");
    }

    #[test]
    fn leaves_a_trailing_ampersand_alone() {
        assert_eq!(legacy_ampersand_to_section("Fish &"), "Fish &");
    }

    #[test]
    fn code_stays_with_the_text_it_colours() {
        // A bare code produces nothing on the client, so the run must not be
        // split — this is the whole reason the conversion is string-wide.
        let out = legacy_ampersand_to_section("&fClick to create");
        assert_eq!(out, "\u{a7}fClick to create");
    }

    #[test]
    fn transparent_is_zero() {
        assert_eq!(parse_background(Some("transparent")), Some(TRANSPARENT));
        assert_eq!(parse_background(Some("TRANSPARENT")), Some(TRANSPARENT));
    }

    /// Upstream quirk: named colours are forced to alpha 0xC8.
    #[test]
    fn named_colours_take_alpha_c8() {
        assert_eq!(parse_background(Some("red")), Some(0xC8FF_5555));
        assert_eq!(parse_background(Some("  Dark Gray ")), Some(0xC855_5555));
    }

    #[test]
    fn hash_without_alpha_is_made_solid() {
        // `TextHologramData.java:156` — a bare #RRGGBB would otherwise have
        // alpha 0 and be invisible.
        assert_eq!(
            parse_background(Some("#ff5555")),
            Some(0xFF00_0000 | 0x00FF_5555)
        );
    }

    #[test]
    fn hash_with_alpha_is_taken_as_written() {
        assert_eq!(parse_background(Some("#c8ff5555")), Some(0xC8FF_5555));
        assert_eq!(parse_background(Some("#FF00FF00")), Some(0xFF00_FF00));
    }

    #[test]
    fn absent_and_unknown_are_distinguished_from_transparent() {
        assert_eq!(parse_background(None), None);
        assert_eq!(parse_background(Some("chartreuse")), None);
    }

    #[test]
    fn formats_solid_named_colour_as_a_name() {
        assert_eq!(
            format_background(Some(0xFF00_0000 | 0x00FF_5555)),
            Some("red".into())
        );
    }

    /// The lossy round trip, preserved deliberately.
    ///
    /// Read `red` -> 0xC8FF5555 (alpha forced to 0xC8) -> not alpha 255 ->
    /// written `#c8ff5555` -> re-read as null by YAML. A named background set
    /// at runtime does not survive a save/reload upstream, and does not here.
    #[test]
    fn named_colour_round_trip_is_lossy_upstream_and_here() {
        let parsed = parse_background(Some("red")).unwrap();
        let formatted = format_background(Some(parsed)).unwrap();
        assert_eq!(formatted, "#c8ff5555");
        // And YAML reads that unquoted value back as null.
        let reparsed = crate::yaml::parse(&format!("background: {formatted}\n")).unwrap();
        assert!(reparsed.get("background").unwrap().is_null());
    }

    #[test]
    fn formats_transparent_by_name() {
        assert_eq!(
            format_background(Some(TRANSPARENT)),
            Some("transparent".into())
        );
        assert_eq!(format_background(None), None);
    }

    #[test]
    fn joins_lines_with_newlines() {
        assert_eq!(join_lines(&["a".into(), "b".into(), "c".into()]), "a\nb\nc");
        assert_eq!(join_lines(&[]), "");
    }

    #[test]
    fn parses_bools_strictly() {
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("FALSE"), Some(false));
        assert_eq!(parse_bool("yes"), None);
    }

    #[test]
    fn parses_doubles() {
        assert_eq!(parse_double("1.5"), Some(1.5));
        assert_eq!(parse_double(" -3 "), Some(-3.0));
        assert_eq!(parse_double("abc"), None);
    }
}
