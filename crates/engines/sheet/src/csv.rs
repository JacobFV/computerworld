//! CSV import and export (RFC 4180). Import reads each field as if it were typed into
//! the cell, so numbers, dates, percentages and formulas arrive as themselves.
use crate::address::Cell;
use crate::workbook::{Input, Style, Workbook};

/// Split CSV text into records of fields.
pub fn records(text: &str, sep: char) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    let mut chars = text.chars().peekable();
    let mut pending = false;
    while let Some(c) = chars.next() {
        pending = true;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            c if c == sep => {
                row.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                was_quoted = false;
                pending = false;
            }
            c => field.push(c),
        }
    }
    if pending {
        row.push(field);
        rows.push(row);
    }
    rows
}
/// Guess the separator Excel's text import would: comma unless the first line has
/// more semicolons or tabs.
pub fn sniff(text: &str) -> char {
    let first = text.lines().next().unwrap_or("");
    let count = |c: char| first.matches(c).count();
    [(',', count(',')), (';', count(';')), ('\t', count('\t'))]
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n > 0)
        .map_or(',', |(c, _)| c)
}
/// A one-sheet workbook from CSV text; the sheet is named after the file, as Excel does.
pub fn read(text: &str, sheet_name: &str) -> Workbook {
    let mut wb = Workbook::with_sheet(sheet_name);
    for (r, record) in records(text, sniff(text)).into_iter().enumerate() {
        for (c, field) in record.into_iter().enumerate() {
            if field.is_empty() {
                continue;
            }
            let (input, format) = match Workbook::parse_entry(&field) {
                Ok(x) => x,
                // A formula that does not parse arrives as the text it was.
                Err(_) => (Input::Value(crate::value::Value::Text(field.clone())), None),
            };
            let style = Style {
                format: format.unwrap_or("General").into(),
                ..Style::default()
            };
            let value = match &input {
                Input::Value(v) => v.clone(),
                Input::Formula(_) => crate::value::Value::Empty,
            };
            wb.load_cell(0, Cell::new(r as u32, c as u32), input, value, style);
        }
    }
    wb.loaded()
}
/// One sheet as CSV: each cell as displayed through its format (as Excel saves CSV),
/// quoted when it holds the separator, a quote or a line break. Lines end in CRLF.
pub fn write(wb: &Workbook, sheet: usize) -> String {
    let Some(used) = wb.used_range(sheet) else {
        return String::new();
    };
    let mut out = String::new();
    for row in 0..=used.end.row {
        let mut fields = Vec::new();
        for col in 0..=used.end.col {
            let shown = wb.display(sheet, Cell::new(row, col));
            if shown.contains([',', '"', '\n', '\r']) {
                fields.push(format!("\"{}\"", shown.replace('"', "\"\"")));
            } else {
                fields.push(shown);
            }
        }
        while fields.last().is_some_and(String::is_empty) {
            fields.pop();
        }
        out.push_str(&fields.join(","));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    #[test]
    fn csv_round_trips_values_and_formulas() {
        let wb = read(
            "Item,Qty,Price,Total\r\nApples,3,$1.50,=B2*C2\r\n\"Pears, green\",4,$2.00,=B3*C3\r\n",
            "prices",
        );
        assert_eq!(wb.sheets[0].name, "prices");
        assert_eq!(wb.value(0, Cell::new(1, 3)), Value::Number(4.5));
        assert_eq!(wb.display(0, Cell::new(1, 2)), "$1.50");
        assert_eq!(
            wb.value(0, Cell::new(2, 0)),
            Value::Text("Pears, green".into())
        );
        let text = write(&wb, 0);
        assert_eq!(
            text,
            "Item,Qty,Price,Total\r\nApples,3,$1.50,4.5\r\n\"Pears, green\",4,$2.00,8\r\n"
        );
        assert_eq!(
            records("a;b\n1;2", sniff("a;b\n1;2")),
            vec![vec!["a", "b"], vec!["1", "2"]]
        );
    }
}
