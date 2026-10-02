//! Books and notes as the player reads them: the server's text with its
//! line breaks made plain, and a book's text laid out page by page.

/// The server's text with its line-break markup (`<BR>` in any case, and
/// the `^` notes use) as newlines, without trailing ones.
#[must_use]
pub fn plain(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        plain.push_str(&rest[..start]);
        let tag = &rest[start..];
        if tag
            .get(..4)
            .is_some_and(|head| head.eq_ignore_ascii_case("<br>"))
        {
            plain.push('\n');
            rest = &tag[4..];
        } else {
            plain.push('<');
            rest = &tag[1..];
        }
    }
    plain.push_str(rest);
    plain
        .replace("\r\n", "\n")
        .replace('^', "\n")
        .trim_end()
        .to_owned()
}

/// A book's text in pages of so many lines of so many characters, wrapped
/// at spaces; a word longer than a line is cut. A book always has a page,
/// even an empty one.
#[must_use]
pub fn pages(text: &str, columns: usize, lines: usize) -> Vec<String> {
    let (columns, lines) = (columns.max(1), lines.max(1));
    let mut wrapped: Vec<String> = Vec::new();
    for paragraph in plain(text).split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let mut word = word.to_owned();
            loop {
                let room = if line.is_empty() {
                    columns
                } else {
                    columns.saturating_sub(line.chars().count() + 1)
                };
                if word.chars().count() <= room {
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(&word);
                    break;
                }
                if line.is_empty() {
                    // A word longer than a line: cut it.
                    let cut: String = word.chars().take(columns).collect();
                    word = word.chars().skip(columns).collect();
                    wrapped.push(cut);
                } else {
                    wrapped.push(std::mem::take(&mut line));
                }
            }
        }
        wrapped.push(line);
    }
    let mut pages: Vec<String> = wrapped
        .chunks(lines)
        .map(|page| page.join("\n").trim_end().to_owned())
        .collect();
    if pages.is_empty() {
        pages.push(String::new());
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_breaks_become_newlines_and_other_markup_stays() {
        assert_eq!(
            plain("To the guard,<BR>greetings.<br>2 < 3"),
            "To the guard,\ngreetings.\n2 < 3"
        );
        assert_eq!(plain("a\r\nb"), "a\nb");
        // A tag cut short by a character longer than a byte stays as it is.
        assert_eq!(plain("<é>"), "<é>");
        // Notes end lines with carets.
        assert_eq!(plain("Find Hager.^Go.^"), "Find Hager.\nGo.");
    }

    #[test]
    fn a_book_wraps_at_spaces_and_fills_pages_in_order() {
        let pages = pages("one two three four five<BR>six", 9, 2);
        assert_eq!(pages, ["one two\nthree", "four five\nsix"]);
        assert_eq!(super::pages("", 10, 3), [""]);
        // A word longer than a line is cut across lines.
        assert_eq!(super::pages("abcdefghij", 4, 5), ["abcd\nefgh\nij"]);
    }
}
