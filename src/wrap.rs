use std::ops::Range;

// Wrap one logical line without dropping whitespace. Units are characters or
// graphemes, described by terminal width and whether they are whitespace.
pub(crate) fn word_ranges(units: &[(usize, bool)], width: usize) -> Vec<Range<usize>> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < units.len() {
        let mut end = start;
        let mut used = 0;
        let mut word_seen = false;
        let mut boundary = None;
        while end < units.len() {
            let (cells, whitespace) = units[end];
            if !whitespace && end > start && units[end - 1].1 {
                let word_fits = || {
                    units[end..]
                        .iter()
                        .take_while(|(_, whitespace)| !whitespace)
                        .map(|(cells, _)| cells)
                        .sum::<usize>()
                        <= width
                };
                if word_seen || word_fits() {
                    boundary = Some(end);
                }
            }
            if used + cells > width && end > start {
                break;
            }
            used += cells;
            word_seen |= !whitespace;
            end += 1;
        }
        if end < units.len() {
            // Keep indentation with oversized tokens. With no word break,
            // fall back to a cell-width break so long tokens still make progress.
            end = boundary.unwrap_or(end);
        }
        rows.push(start..end);
        start = end;
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthChar;

    #[test]
    fn word_breaks_preserve_all_text_and_handle_long_tokens() {
        for (text, width, expected) in [
            ("one two three", 9, vec!["one two ", "three"]),
            ("one  two", 6, vec!["one  ", "two"]),
            ("  abcdefghi", 5, vec!["  abc", "defgh", "i"]),
            ("  hello world", 6, vec!["  ", "hello ", "world"]),
            ("ab cdefghijk", 5, vec!["ab ", "cdefg", "hijk"]),
            ("界界 界界", 6, vec!["界界 ", "界界"]),
            ("   ", 2, vec!["  ", " "]),
            ("", 2, vec![""]),
        ] {
            let chars: Vec<_> = text.chars().collect();
            let units: Vec<_> = chars
                .iter()
                .map(|c| (c.width().unwrap_or(0), c.is_whitespace()))
                .collect();
            let rows: Vec<String> = word_ranges(&units, width)
                .into_iter()
                .map(|range| chars[range].iter().collect())
                .collect();
            assert_eq!(rows, expected, "{text:?}");
            assert_eq!(rows.concat(), text);
        }
    }
}
