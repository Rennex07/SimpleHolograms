//! Completion for the greedy edit argument, without replacing its earlier words.

use crate::data::LineEdit;

pub const EDITS: [&str; 5] = [
    "addline",
    "setline",
    "removeline",
    "insertbefore",
    "insertafter",
];

pub fn suggestions(
    input: &str,
    editing: bool,
    holograms: &[(String, usize)],
    allowed_edits: &[&str],
) -> (usize, Vec<String>) {
    let start = input
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let previous: Vec<_> = input[..start].split_whitespace().collect();
    let prefix = input[start..].to_lowercase();
    let candidates: Vec<String> = match previous.as_slice() {
        [] => holograms.iter().map(|(name, _)| name.clone()).collect(),
        [name]
            if editing
                && holograms
                    .iter()
                    .any(|(known, _)| known.eq_ignore_ascii_case(name)) =>
        {
            allowed_edits.iter().map(|edit| edit.to_string()).collect()
        }
        [name, edit]
            if editing
                && allowed_edits
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(edit)) =>
        {
            let count = holograms
                .iter()
                .find(|(known, _)| known.eq_ignore_ascii_case(name))
                .map(|(_, count)| *count);
            match (count, LineEdit::parse(edit)) {
                (Some(count), Some(edit)) if edit != LineEdit::Add => {
                    let first = usize::from(edit != LineEdit::InsertAfter);
                    let last =
                        count + usize::from(matches!(edit, LineEdit::Set | LineEdit::InsertBefore));
                    (first..=last).map(|line| line.to_string()).collect()
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    };
    (
        start,
        candidates
            .into_iter()
            .filter(|value| value.to_lowercase().starts_with(&prefix))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(input: &str) -> (usize, Vec<String>) {
        suggestions(
            input,
            true,
            &[("spawn".into(), 2), ("欢迎".into(), 1)],
            &EDITS,
        )
    }

    #[test]
    fn completes_names_and_every_edit_operation() {
        assert_eq!(complete("sp"), (0, vec!["spawn".into()]));
        assert_eq!(
            complete("spawn "),
            (6, EDITS.iter().map(|s| s.to_string()).collect())
        );
        assert_eq!(complete("spawn ins").1, ["insertbefore", "insertafter"]);
        assert_eq!(complete("SPAWN SET").1, ["setline"]);
    }

    #[test]
    fn line_numbers_follow_the_operation_bounds() {
        assert_eq!(complete("spawn setline ").1, ["1", "2", "3"]);
        assert_eq!(complete("spawn removeline ").1, ["1", "2"]);
        assert_eq!(complete("spawn insertbefore ").1, ["1", "2", "3"]);
        assert_eq!(complete("spawn insertafter ").1, ["0", "1", "2"]);
        assert!(complete("spawn addline ").1.is_empty());
        assert!(complete("spawn setline 1 new text").1.is_empty());
    }

    #[test]
    fn replaces_only_the_current_word_even_with_unicode_and_extra_spaces() {
        let input = "欢迎  set";
        let (start, values) = complete(input);
        assert_eq!(&input[start..], "set");
        assert_eq!(values, ["setline"]);
        assert_eq!(complete("spawn removeline 2").1, ["2"]);
    }

    #[test]
    fn unknown_names_and_disallowed_edits_offer_no_followups() {
        assert!(complete("missing ").1.is_empty());
        assert!(complete("spawn missing ").1.is_empty());
        let names = [("spawn".into(), 2)];
        assert_eq!(
            suggestions("spawn ", true, &names, &["addline"]).1,
            ["addline"]
        );
        assert!(
            suggestions("spawn removeline ", true, &names, &["addline"])
                .1
                .is_empty()
        );
        assert!(suggestions("spawn ", false, &names, &EDITS).1.is_empty());
    }
}
