//! Integer fixed-point calculator. No floats anywhere in state, so a snapshot round trip
//! is exact and two machines always agree.
use super::look::{action, look, INK, LINE, MUTED};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

/// Values are hundredths, so 12.34 is 1234. Division truncates toward zero.
const SCALE: i64 = 100;
const LIMIT: i64 = 999_999_999_999;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calculator {
    /// Digits the user is entering, as typed. Empty means `accumulator` is on display.
    pub entry: String,
    pub accumulator: i64,
    pub pending: Option<String>,
    /// Set after `=` or an operator, so the next digit starts a fresh entry.
    pub replace: bool,
    pub error: Option<String>,
    pub history: Vec<String>,
}
impl Calculator {
    pub const KIND: &'static str = "calculator";
    pub fn launch(_argument: &str, _window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        (Self::default(), vec![])
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, _theme: DesktopTheme) -> String {
        "Calculator".into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        self.display()
    }
    pub fn modified(&self) -> bool {
        false
    }
    pub fn offline(&mut self, _tag: &str, _reason: &str) {}
    pub fn http(
        &mut self,
        _window: u64,
        _tag: &str,
        _status: u16,
        _body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        Err("calculator makes no requests".into())
    }
    /// Value currently on the display, formatted with two decimals.
    pub fn display(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        if !self.entry.is_empty() {
            return self.entry.clone();
        }
        format_fixed(self.accumulator)
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        for ch in text.chars() {
            let key = match ch {
                '0'..='9' | '.' => ch.to_string(),
                '+' | '-' | '*' | '/' => ch.to_string(),
                '=' => "=".into(),
                'c' | 'C' => "clear".into(),
                _ => continue,
            };
            self.press(&key)?;
        }
        Ok(())
    }
    pub fn key(
        &mut self,
        _window: u64,
        key: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        match key {
            "Enter" => self.press("=")?,
            "Backspace" => {
                self.entry.pop();
            }
            "Escape" => self.press("clear")?,
            other if other.chars().count() == 1 => self.text(other)?,
            other => return Err(format!("unsupported calculator key {other}")),
        }
        Ok(vec![])
    }
    pub fn click(
        &mut self,
        _window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let key = target
            .strip_prefix("calc:")
            .ok_or("interaction does not belong to the calculator")?;
        self.press(key)?;
        Ok(vec![])
    }
    fn press(&mut self, key: &str) -> Result<(), String> {
        if self.error.is_some() && key != "clear" {
            return Ok(());
        }
        match key {
            "clear" => *self = Self::default(),
            "sign" => {
                if self.entry.is_empty() {
                    self.accumulator = -self.accumulator;
                } else if let Some(rest) = self.entry.strip_prefix('-') {
                    self.entry = rest.into();
                } else {
                    self.entry.insert(0, '-');
                }
            }
            "percent" => {
                let value = self.take_entry();
                self.accumulator = value / 100;
            }
            "." => {
                if self.replace {
                    self.entry.clear();
                    self.replace = false;
                }
                if !self.entry.contains('.') {
                    if self.entry.is_empty() {
                        self.entry.push('0');
                    }
                    self.entry.push('.');
                }
            }
            digit if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) => {
                if self.replace {
                    self.entry.clear();
                    self.replace = false;
                }
                // Two decimals is the whole precision of the model; refuse a third.
                let decimals = self.entry.split_once('.').map(|(_, d)| d.len());
                if decimals != Some(2) && self.entry.len() < 15 {
                    self.entry.push_str(digit);
                }
            }
            "+" | "-" | "*" | "/" => {
                self.apply()?;
                self.pending = Some(key.into());
                self.replace = true;
            }
            "=" => {
                self.apply()?;
                self.pending = None;
                self.replace = true;
            }
            other => return Err(format!("unknown calculator key {other}")),
        }
        Ok(())
    }
    fn take_entry(&mut self) -> i64 {
        if self.entry.is_empty() {
            return self.accumulator;
        }
        let value = parse_fixed(&std::mem::take(&mut self.entry));
        self.replace = false;
        value
    }
    fn apply(&mut self) -> Result<(), String> {
        let had_entry = !self.entry.is_empty();
        let value = self.take_entry();
        let Some(op) = self.pending.clone() else {
            self.accumulator = value;
            return Ok(());
        };
        if !had_entry && self.replace {
            // Two operators in a row only change which one is pending.
            return Ok(());
        }
        let left = self.accumulator;
        let result = match op.as_str() {
            "+" => left.checked_add(value),
            "-" => left.checked_sub(value),
            "*" => left.checked_mul(value).map(|v| v / SCALE),
            "/" => {
                if value == 0 {
                    self.error = Some("Cannot divide by zero".into());
                    return Ok(());
                }
                left.checked_mul(SCALE).map(|v| v / value)
            }
            _ => None,
        };
        match result.filter(|v| v.abs() <= LIMIT) {
            Some(result) => {
                self.history.push(format!(
                    "{} {op} {} = {}",
                    format_fixed(left),
                    format_fixed(value),
                    format_fixed(result)
                ));
                if self.history.len() > 32 {
                    self.history.remove(0);
                }
                self.accumulator = result;
            }
            None => self.error = Some("Result is out of range".into()),
        }
        Ok(())
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        page.elements.push(E::Heading {
            id: "calc-display".into(),
            text: self.display(),
            level: 2,
        });
        for (id, label) in KEYS.iter().flatten() {
            page.elements.push(E::Button {
                id: format!("calc:{id}"),
                text: (*label).into(),
                action: cw_protocol::PageAction {
                    method: "APP".into(),
                    url: format!("calc:{id}"),
                    fields: Default::default(),
                },
            });
        }
        for (index, entry) in self.history.iter().enumerate() {
            page.elements.push(E::Text {
                id: format!("calc-history-{index}"),
                text: entry.clone(),
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = if theme.mobile() {
            Color::rgb(28, 28, 30)
        } else {
            l.surface
        };
        let dark = theme.mobile();
        let ink = if dark { Color::WHITE } else { INK };
        let readout = (height / 5).clamp(56, 140);
        p.label(
            0,
            readout as i32 - 56,
            width.saturating_sub(18),
            &self.display(),
            if self.error.is_some() { 20 } else { 38 },
            if self.error.is_some() {
                Color::rgb(215, 68, 52)
            } else {
                ink
            },
            false,
            Align::Right,
        );
        if !dark {
            p.hline(0, readout as i32, width, LINE);
        }
        let top = readout as i32 + 8;
        let rows = KEYS.len() as u32;
        let gap = 6;
        let cell_h = (height.saturating_sub(top as u32 + gap)) / rows.max(1);
        let cell_w = (width.saturating_sub(gap)) / 4;
        for (row, keys) in KEYS.iter().enumerate() {
            for (column, (id, label)) in keys.iter().enumerate() {
                let r = Rect::new(
                    gap as i32 / 2 + column as i32 * cell_w as i32,
                    top + row as i32 * cell_h as i32,
                    cell_w.saturating_sub(gap),
                    cell_h.saturating_sub(gap),
                );
                let operator = matches!(*id, "+" | "-" | "*" | "/" | "=");
                let fill = if operator {
                    l.accent
                } else if dark {
                    Color::rgb(58, 58, 60)
                } else {
                    Color::rgb(240, 240, 242)
                };
                p.button(
                    r,
                    fill,
                    if dark { r.height / 2 } else { l.radius },
                    &format!("calc:{id}"),
                    label,
                );
                p.label(
                    r.x,
                    r.y + (r.height as i32 - 22) / 2,
                    r.width,
                    label,
                    18,
                    if operator || dark { Color::WHITE } else { INK },
                    false,
                    Align::Center,
                );
            }
        }
        if !theme.mobile() && width > 420 {
            // Desktop calculators keep a visible tape; ours shows only real results.
            let x = width as i32 - 150;
            p.vline(x - 10, top, height.saturating_sub(top as u32), LINE);
            p.left(x, top, 140, "History", 11, MUTED);
            for (index, entry) in self.history.iter().rev().take(8).enumerate() {
                p.left(x, top + 20 + index as i32 * 18, 140, entry, 11, MUTED);
            }
        }
        let _ = action;
    }
}
/// Keypad layout: (interaction id, label).
const KEYS: [[(&str, &str); 4]; 5] = [
    [
        ("clear", "AC"),
        ("sign", "+/−"),
        ("percent", "%"),
        ("/", "÷"),
    ],
    [("7", "7"), ("8", "8"), ("9", "9"), ("*", "×")],
    [("4", "4"), ("5", "5"), ("6", "6"), ("-", "−")],
    [("1", "1"), ("2", "2"), ("3", "3"), ("+", "+")],
    [("0", "0"), (".", "."), ("=", "="), ("=", "=")],
];

fn parse_fixed(text: &str) -> i64 {
    let negative = text.starts_with('-');
    let digits = text.trim_start_matches('-');
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let whole: i64 = whole.parse().unwrap_or(0);
    let mut fraction: i64 = fraction
        .chars()
        .take(2)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    if digits.split_once('.').map(|(_, f)| f.len()) == Some(1) {
        fraction *= 10;
    }
    let value = whole.saturating_mul(SCALE).saturating_add(fraction);
    if negative {
        -value
    } else {
        value
    }
}
fn format_fixed(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.unsigned_abs();
    let whole = magnitude / SCALE as u64;
    let fraction = magnitude % SCALE as u64;
    if fraction == 0 {
        format!("{sign}{whole}")
    } else if fraction.is_multiple_of(10) {
        format!("{sign}{whole}.{}", fraction / 10)
    } else {
        format!("{sign}{whole}.{fraction:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(keys: &[&str]) -> Calculator {
        let mut c = Calculator::default();
        for key in keys {
            c.click(1, &format!("calc:{key}"), 0).unwrap();
        }
        c
    }
    #[test]
    fn arithmetic_is_exact_fixed_point_and_records_real_history() {
        assert_eq!(run(&["1", "2", ".", "5", "+", "2", "="]).display(), "14.5");
        assert_eq!(run(&["3", "*", "4", "="]).display(), "12");
        assert_eq!(run(&["1", "0", "/", "4", "="]).display(), "2.5");
        assert_eq!(run(&["5", "-", "8", "="]).display(), "-3");
        let c = run(&["2", "+", "2", "="]);
        assert_eq!(c.history, vec!["2 + 2 = 4"]);
    }
    #[test]
    fn division_by_zero_and_overflow_are_states_not_panics() {
        let c = run(&["5", "/", "0", "="]);
        assert_eq!(c.display(), "Cannot divide by zero");
        let mut c = Calculator {
            accumulator: super::LIMIT,
            pending: Some("*".into()),
            ..Default::default()
        };
        c.click(1, "calc:9", 0).unwrap();
        c.click(1, "calc:=", 0).unwrap();
        assert_eq!(c.display(), "Result is out of range");
        // Only Clear leaves an error state.
        c.click(1, "calc:1", 0).unwrap();
        assert_eq!(c.display(), "Result is out of range");
        c.click(1, "calc:clear", 0).unwrap();
        assert_eq!(c.display(), "0");
    }
    #[test]
    fn precision_and_repeated_operators_are_bounded() {
        assert_eq!(run(&["1", ".", "2", "3", "4", "5"]).display(), "1.23");
        assert_eq!(run(&[".", ".", "5"]).display(), "0.5");
        assert_eq!(run(&["7", "+", "-", "3", "="]).display(), "4");
    }
    #[test]
    fn every_painted_key_is_one_the_model_accepts() {
        for (id, _) in KEYS.iter().flatten() {
            let mut c = Calculator::default();
            assert!(c.click(1, &format!("calc:{id}"), 0).is_ok(), "{id}");
        }
        let mut c = Calculator::default();
        assert!(c.click(1, "calc:bogus", 0).is_err());
        assert!(c.click(1, "not-mine", 0).is_err());
    }
}
