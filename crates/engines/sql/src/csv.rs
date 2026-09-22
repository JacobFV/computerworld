//! RFC 4180 CSV, as `sqlite3`'s `.import` reads it and `.mode csv` writes it.

/// Split text into records of fields. Quoted fields may hold separators, doubled
/// quotes and line breaks; CRLF and LF both end a record.
pub fn parse(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !quoted => {
                in_quotes = true;
                quoted = true;
            }
            c if c == sep => {
                row.push(std::mem::take(&mut field));
                quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                quoted = false;
                any = false;
            }
            c => field.push(c),
        }
    }
    if any || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}
/// One field, quoted when it would not survive unquoted.
pub fn quote(field: &str, sep: &str) -> String {
    let needs = field.contains(sep)
        || field
            .chars()
            .any(|c| c == '"' || c == '\n' || c == '\r' || (c as u32) < 0x20);
    if needs {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_fields_hold_separators_quotes_and_newlines() {
        let rows = parse("a,\"b,c\",\"say \"\"hi\"\"\"\r\n1,\"two\nlines\",3\n", ',');
        assert_eq!(
            rows,
            vec![vec!["a", "b,c", "say \"hi\""], vec!["1", "two\nlines", "3"]]
        );
        assert_eq!(parse("x|y", '|'), vec![vec!["x", "y"]]);
        assert_eq!(quote("a,b", ","), "\"a,b\"");
        assert_eq!(quote("plain", ","), "plain");
    }
}
