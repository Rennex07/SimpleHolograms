//! YAML subset for plugin files: nested maps, scalar sequences, comments and null.
//! Scalars remain text until interpreted by the consumer.

use std::fmt::Write as _;

/// A parsed YAML node.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A scalar, kept as the raw source text. `""` is the empty scalar.
    Scalar(String),
    /// A block sequence (`- item`).
    List(Vec<Value>),
    /// A mapping. Insertion order is preserved so emitted files are stable.
    Map(Vec<(String, Value)>),
}

impl Value {
    /// Looks a key up in a map. Returns `None` for a non-map.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The scalar text, or `None` for a collection.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Scalar(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The sequence items, or `None` for anything else.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    /// True when this node is a scalar holding YAML's `null`, or an empty
    /// scalar. Matches how the original treats `section.getString("x", null)`:
    /// an absent key and an explicit `null` are the same thing.
    pub fn is_null(&self) -> bool {
        match self {
            Value::Scalar(s) => s.is_empty() || s == "null" || s == "~",
            _ => false,
        }
    }
}

/// Parses a YAML document into a [`Value`].
///
/// Returns `Err` on input this subset cannot represent, so a malformed file is
/// reported rather than silently half-read.
pub fn parse(input: &str) -> Result<Value, String> {
    let mut lines: Vec<(usize, usize, String)> = Vec::new();

    for (n, raw) in input.lines().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with('\t') {
            return Err(format!(
                "line {}: tabs are not valid YAML indentation",
                n + 1
            ));
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        lines.push((n + 1, indent, trimmed.to_string()));
    }

    if lines.is_empty() {
        return Ok(Value::Map(Vec::new()));
    }

    let (value, consumed) = parse_block(&lines, 0, lines[0].1)?;
    if consumed != lines.len() {
        return Err(format!(
            "line {}: unexpected indentation",
            lines[consumed].0
        ));
    }
    Ok(value)
}

/// Parses the block starting at `start`, which is known to be at `indent`.
fn parse_block(
    lines: &[(usize, usize, String)],
    start: usize,
    indent: usize,
) -> Result<(Value, usize), String> {
    if start >= lines.len() {
        return Ok((Value::Map(Vec::new()), start));
    }

    if lines[start].2.starts_with("- ") || lines[start].2 == "-" {
        let mut items = Vec::new();
        let mut i = start;
        while i < lines.len()
            && lines[i].1 == indent
            && (lines[i].2.starts_with("- ") || lines[i].2 == "-")
        {
            let rest = lines[i].2[1..].trim().to_string();
            if rest.is_empty() {
                // `-` on its own: the item is whatever block follows, deeper.
                let child_indent = lines.get(i + 1).map_or(indent + 2, |l| l.1);
                let (child, next) = parse_block(lines, i + 1, child_indent)?;
                items.push(child);
                i = next;
            } else if split_key(&rest).is_some() {
                // `- key: value` — an inline first key of a nested map, which
                // this subset does not represent. `split_key` ignores colons
                // inside single quotes, so a quoted scalar containing `: `
                // is still treated as a scalar.
                return Err(format!(
                    "line {}: mappings inside sequences are not supported",
                    lines[i].0
                ));
            } else {
                items.push(Value::Scalar(unquote(&rest)?));
                i += 1;
            }
        }
        return Ok((Value::List(items), i));
    }

    let mut entries: Vec<(String, Value)> = Vec::new();
    let mut i = start;
    while i < lines.len() && lines[i].1 == indent {
        let (line_no, _, text) = &lines[i];
        let (key, value_text) =
            split_key(text).ok_or_else(|| format!("line {line_no}: expected `key: value`"))?;
        let key = unquote(&key)?;

        let value_text = strip_inline_comment(&value_text);
        if !value_text.trim().is_empty() {
            let value = match value_text.trim() {
                "{}" => Value::Map(Vec::new()),
                "[]" => Value::List(Vec::new()),
                value => Value::Scalar(unquote(value)?),
            };
            entries.push((key, value));
            i += 1;
        } else if lines
            .get(i + 1)
            .is_some_and(|l| l.1 == indent && (l.2.starts_with("- ") || l.2 == "-"))
        {
            // A block sequence may sit at the same indent as its key — that is
            // the form Bukkit's YamlConfiguration writes, and the one the files
            // this plugin reads use.
            let (child, next) = parse_block(lines, i + 1, indent)?;
            entries.push((key, child));
            i = next;
        } else {
            // Empty value with no nested block beneath it: an explicit null.
            // Note `None`, not `usize::MAX` — a sentinel indent would make the
            // last key of a document parse as an empty mapping instead.
            match lines.get(i + 1) {
                Some(next) if next.1 > indent => {
                    let (child, next) = parse_block(lines, i + 1, next.1)?;
                    entries.push((key, child));
                    i = next;
                }
                _ => {
                    entries.push((key, Value::Scalar(String::new())));
                    i += 1;
                }
            }
        }
    }
    Ok((Value::Map(entries), i))
}

/// Splits `key: value`, returning `None` when there is no `: ` separator.
///
/// Returns owned halves: the value half may need to have its leading spaces
/// trimmed, and borrowing a freshly built `String` would not outlive the call.
fn split_key(text: &str) -> Option<(String, String)> {
    let mut in_single = false;
    let chars: Vec<char> = text.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        match ch {
            '\'' => in_single = !in_single,
            ':' if !in_single => {
                let rest: String = chars[i + 1..].iter().collect();
                // A `:` only separates a key when followed by a space or EOL.
                if rest.is_empty() || rest.starts_with(' ') {
                    let key: String = chars[..i].iter().collect();
                    return Some((key, rest.trim_start().to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

/// Removes a trailing ` # ...` comment.
///
/// A `#` only starts a comment at the beginning of a line or when preceded by
/// whitespace, per the YAML spec. That rule is what turns an unquoted
/// `#c8ff5555` background into a null on the way back in — see the note on
/// [`Value::is_null`].
fn strip_inline_comment(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut in_single = false;
    for (i, ch) in chars.iter().enumerate() {
        match ch {
            '\'' => in_single = !in_single,
            '#' if !in_single && (i == 0 || chars[i - 1].is_whitespace()) => {
                return chars[..i].iter().collect();
            }
            _ => {}
        }
    }
    value.to_string()
}

/// Removes single-quote wrapping, resolving `''` as a literal quote.
fn unquote(raw: &str) -> Result<String, String> {
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        Ok(raw[1..raw.len() - 1].replace("''", "'"))
    } else {
        Ok(raw.to_string())
    }
}

/// Formats a float the way the original's YAML files do: always with a decimal
/// point, so a stored `1.0` does not come back as the integer `1`.
pub fn fmt_f64(v: f64) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// Formats a float for embedding in a message. The original uses Java's
/// `DecimalFormat("#########.##")`: between one and nine integer digits, and
/// at most two decimals with trailing zeros dropped. Because the integer part
/// is all `#` — optional digits — a value below 1 renders with no leading
/// zero, so `-0.5` prints as `-.5`.
pub fn fmt_coord(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let mut s = format!("{:.2}", (v * 100.0).trunc() / 100.0);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    match s.strip_prefix("-0.") {
        Some(rest) => format!("-.{rest}"),
        None => s,
    }
}

/// True when a scalar must be quoted to survive a round trip.
///
/// Only structural characters matter here. Numbers, booleans and `null` are
/// deliberately *not* quoted: scalars are carried as raw text and interpreted
/// by the key's own reader, so quoting them would change nothing except how the
/// file looks. The empty string must be quoted, though — an unquoted empty
/// scalar would be read back as the start of a nested block.
fn needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if s != s.trim() {
        return true;
    }
    if s.starts_with("- ") || s == "-" {
        return true;
    }
    if s.starts_with('#') || s.contains(" #") {
        return true;
    }
    if s.contains(": ") || s.ends_with(':') {
        return true;
    }
    s.chars().any(|c| {
        matches!(
            c,
            ':' | '#'
                | '\''
                | '"'
                | '\n'
                | '\t'
                | '{'
                | '}'
                | '['
                | ']'
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '%'
                | '@'
                | '`'
        )
    })
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Renders a [`Value`] as a YAML document, with no trailing newline.
pub fn emit(value: &Value) -> String {
    let mut out = String::new();
    emit_into(value, 0, &mut out);
    out
}

fn emit_into(value: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        Value::Scalar(s) => {
            if needs_quoting(s) {
                let _ = write!(out, "{pad}{}", quote(s));
            } else {
                let _ = write!(out, "{pad}{s}");
            }
        }
        Value::List(items) => {
            for item in items {
                let _ = write!(out, "{pad}- ");
                match item {
                    Value::Scalar(s) => {
                        if needs_quoting(s) {
                            let _ = write!(out, "{}", quote(s));
                        } else {
                            let _ = write!(out, "{s}");
                        }
                    }
                    other => {
                        out.push('\n');
                        emit_into(other, indent + 2, out);
                    }
                }
                out.push('\n');
            }
            out.pop();
        }
        Value::Map(entries) => {
            for (key, child) in entries {
                let key_text = if needs_quoting(key) {
                    quote(key)
                } else {
                    key.clone()
                };
                match child {
                    Value::Scalar(s) => {
                        let rendered = if needs_quoting(s) {
                            quote(s)
                        } else {
                            s.clone()
                        };
                        let _ = write!(out, "{pad}{key_text}: {rendered}");
                    }
                    Value::List(items) if items.is_empty() => {
                        let _ = write!(out, "{pad}{key_text}:");
                    }
                    Value::List(_) => {
                        let _ = write!(out, "{pad}{key_text}:");
                        out.push('\n');
                        let mut nested = String::new();
                        emit_into(child, indent, &mut nested);
                        out.push_str(&nested);
                    }
                    Value::Map(inner) if inner.is_empty() => {
                        let _ = write!(out, "{pad}{key_text}: {{}}");
                    }
                    Value::Map(_) => {
                        let _ = write!(out, "{pad}{key_text}:");
                        out.push('\n');
                        let mut nested = String::new();
                        emit_into(child, indent + 2, &mut nested);
                        out.push_str(&nested);
                    }
                }
                out.push('\n');
            }
            out.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(v: &'a Value, path: &str) -> &'a Value {
        let mut cur = v;
        for part in path.split('.') {
            cur = cur
                .get(part)
                .unwrap_or_else(|| panic!("missing {part} in {cur:?}"));
        }
        cur
    }

    #[test]
    fn parses_nested_maps_and_sequences() {
        let doc = "\
holograms:
  welcome:
    type: TEXT
    text:
    - 'line one'
    - line two
    location:
      world: world
      x: 1.5
";
        let v = parse(doc).unwrap();
        assert_eq!(get(&v, "holograms.welcome.type").as_str(), Some("TEXT"));
        assert_eq!(
            get(&v, "holograms.welcome.location.world").as_str(),
            Some("world")
        );
        assert_eq!(
            get(&v, "holograms.welcome.location.x").as_str(),
            Some("1.5")
        );
        let text = get(&v, "holograms.welcome.text").as_list().unwrap();
        assert_eq!(text.len(), 2);
        assert_eq!(text[0].as_str(), Some("line one"));
        assert_eq!(text[1].as_str(), Some("line two"));
    }

    #[test]
    fn empty_value_becomes_null() {
        let v = parse("a:\nb: 1\n").unwrap();
        assert!(get(&v, "a").is_null());
        assert_eq!(get(&v, "b").as_str(), Some("1"));
    }

    #[test]
    fn inline_comment_is_stripped() {
        let v = parse("version: 2 # DO NOT CHANGE\n").unwrap();
        assert_eq!(get(&v, "version").as_str(), Some("2"));
    }

    /// Reproduced bug, not a parser error.
    ///
    /// The original writes a non-named background as `'#' + Integer.toHexString(...)`
    /// (`TextHologramData.java:181`). SnakeYAML treats a `#` preceded by a
    /// space as a comment, so that value reads back as null and the background
    /// is lost on reload. The parser follows YAML here rather than "fixing" it,
    /// so the round trip behaves exactly as it does upstream.
    #[test]
    fn unquoted_hash_reads_back_as_null() {
        let v = parse("background: #c8ff5555\n").unwrap();
        assert!(get(&v, "background").is_null());
    }

    #[test]
    fn quoted_hash_survives() {
        let v = parse("background: '#c8ff5555'\n").unwrap();
        assert_eq!(get(&v, "background").as_str(), Some("#c8ff5555"));
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let v = parse("# leading\n\nkey: value\n\n# trailing\n").unwrap();
        assert_eq!(get(&v, "key").as_str(), Some("value"));
    }

    #[test]
    fn round_trips_through_emit() {
        let src = "\
holograms:
  welcome:
    type: TEXT
    text:
    - 'Edit this line with /hologram edit welcome'
    persistent: true
    billboard:
    background: transparent
    x: 1.0
";
        let parsed = parse(src).unwrap();
        let emitted = emit(&parsed);
        let reparsed = parse(&emitted).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn emit_quotes_only_structurally_ambiguous_scalars() {
        let v = parse("a: 'true'\nb: plain\nc: 'has: colon'\nd: ''\ne: ' leading'\n").unwrap();
        let out = emit(&v);
        // Scalars are carried as raw text, so `true` needs no quoting...
        assert!(out.contains("a: true"), "{out}");
        assert!(out.contains("b: plain"), "{out}");
        // ...but anything that would change the structure is quoted.
        assert!(out.contains("c: 'has: colon'"), "{out}");
        assert!(out.contains("d: ''"), "{out}");
        assert!(out.contains("e: ' leading'"), "{out}");
    }

    #[test]
    fn emit_escapes_single_quotes() {
        let v = parse("a: 'it''s here'\n").unwrap();
        let out = emit(&v);
        assert_eq!(
            parse(&out).unwrap().get("a").unwrap().as_str(),
            Some("it's here")
        );
    }

    #[test]
    fn fmt_coord_matches_decimal_format_behaviour() {
        assert_eq!(fmt_coord(1.0), "1");
        assert_eq!(fmt_coord(1.5), "1.5");
        assert_eq!(fmt_coord(-0.5), "-.5");
        assert_eq!(fmt_coord(1.234), "1.23");
        assert_eq!(fmt_coord(1.235), "1.23");
        assert_eq!(fmt_coord(1234.5678), "1234.56");
    }

    #[test]
    fn fmt_f64_keeps_a_decimal_point() {
        assert_eq!(fmt_f64(1.0), "1.0");
        assert_eq!(fmt_f64(-0.0), "-0.0");
        assert_eq!(fmt_f64(0.5), "0.5");
    }

    #[test]
    fn rejects_tab_indentation() {
        assert!(parse("a:\n\tb: 1\n").is_err());
    }
}
