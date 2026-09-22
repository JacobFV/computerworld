//! FreeCAD's expression engine at this kernel's scale: a property may be bound to an
//! expression over numbers, `+ - * /`, parentheses and references to other objects'
//! properties (`Pad.Length`, `Sketch.Constraints.width`). Bindings are evaluated on
//! recompute, before the features they drive.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Num(f64),
    /// `Object.Property` or `Object.Constraints.Name`: the object and the property path.
    Ref {
        object: String,
        property: String,
    },
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Every `(object, property)` the expression reads.
    pub fn references(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }
    fn collect(&self, out: &mut Vec<(String, String)>) {
        match self {
            Expr::Num(_) => {}
            Expr::Ref { object, property } => out.push((object.clone(), property.clone())),
            Expr::Neg(a) => a.collect(out),
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                a.collect(out);
                b.collect(out);
            }
        }
    }
    /// Evaluate with `lookup(object, property)` supplying referenced values.
    pub fn eval(&self, lookup: &dyn Fn(&str, &str) -> Option<f64>) -> Result<f64, String> {
        let v = match self {
            Expr::Num(v) => *v,
            Expr::Ref { object, property } => lookup(object, property)
                .ok_or_else(|| format!("{object}.{property} is not a number property"))?,
            Expr::Neg(a) => -a.eval(lookup)?,
            Expr::Add(a, b) => a.eval(lookup)? + b.eval(lookup)?,
            Expr::Sub(a, b) => a.eval(lookup)? - b.eval(lookup)?,
            Expr::Mul(a, b) => a.eval(lookup)? * b.eval(lookup)?,
            Expr::Div(a, b) => {
                let d = b.eval(lookup)?;
                if d == 0.0 {
                    return Err("Division by zero".into());
                }
                a.eval(lookup)? / d
            }
        };
        if !v.is_finite() {
            return Err("The expression does not evaluate to a number".into());
        }
        Ok(v)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
}

fn lex(text: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            // An exponent, when a sign or digit follows the e.
            if i < chars.len()
                && (chars[i] == 'e' || chars[i] == 'E')
                && i + 1 < chars.len()
                && (chars[i + 1].is_ascii_digit() || chars[i + 1] == '-' || chars[i + 1] == '+')
            {
                i += 2;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = chars[start..i].iter().collect();
            let v: f64 = s.parse().map_err(|_| format!("\"{s}\" is not a number"))?;
            out.push(Tok::Num(v));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if "+-*/()".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else {
            return Err(format!("Unexpected character '{c}' in expression"));
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}
impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }
    fn expr(&mut self) -> Result<Expr, String> {
        let mut lhs = self.term()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            let rhs = self.term()?;
            lhs = if c == '+' {
                Expr::Add(Box::new(lhs), Box::new(rhs))
            } else {
                Expr::Sub(Box::new(lhs), Box::new(rhs))
            };
        }
        Ok(lhs)
    }
    fn term(&mut self) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek().cloned() {
            self.at += 1;
            let rhs = self.unary()?;
            lhs = if c == '*' {
                Expr::Mul(Box::new(lhs), Box::new(rhs))
            } else {
                Expr::Div(Box::new(lhs), Box::new(rhs))
            };
        }
        Ok(lhs)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.at += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op('+')) => {
                self.at += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Num(v)) => {
                // A unit after a number: mm, deg and the like are the document's own units.
                if let Some(Tok::Ident(u)) = self.peek() {
                    let scale = match u.as_str() {
                        "mm" | "deg" | "°" => Some(1.0),
                        "cm" => Some(10.0),
                        "m" => Some(1000.0),
                        "in" => Some(25.4),
                        _ => None,
                    };
                    if let Some(s) = scale {
                        self.at += 1;
                        return Ok(Expr::Num(v * s));
                    }
                }
                Ok(Expr::Num(v))
            }
            Some(Tok::Ident(name)) => {
                let (object, property) = name
                    .split_once('.')
                    .ok_or_else(|| format!("Unknown identifier '{name}': use Object.Property"))?;
                if object.is_empty() || property.is_empty() || property.ends_with('.') {
                    return Err(format!("Malformed reference '{name}'"));
                }
                Ok(Expr::Ref {
                    object: object.to_owned(),
                    property: property.to_owned(),
                })
            }
            Some(Tok::Op('(')) => {
                let e = self.expr()?;
                match self.next() {
                    Some(Tok::Op(')')) => Ok(e),
                    _ => Err("Expected ')'".into()),
                }
            }
            Some(Tok::Op(c)) => Err(format!("Unexpected '{c}'")),
            None => Err("Unexpected end of expression".into()),
        }
    }
}

/// Parse an expression such as `Sketch.Constraints.width * 2 + 5`.
pub fn parse(text: &str) -> Result<Expr, String> {
    let text = text.trim().trim_start_matches('=').trim();
    if text.is_empty() {
        return Err("The expression is empty".into());
    }
    let toks = lex(text)?;
    let mut p = Parser { toks, at: 0 };
    let e = p.expr()?;
    if p.at < p.toks.len() {
        return Err("Unexpected input after the expression".into());
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_references() {
        let e = parse("Sketch.Constraints.width * 2 + (10 - 4) / 3").unwrap();
        let lookup = |o: &str, p: &str| match (o, p) {
            ("Sketch", "Constraints.width") => Some(7.0),
            _ => None,
        };
        assert!((e.eval(&lookup).unwrap() - 16.0).abs() < 1e-12);
        assert_eq!(
            e.references(),
            vec![("Sketch".to_owned(), "Constraints.width".to_owned())]
        );
        assert!((parse("-2 * -3").unwrap().eval(&lookup).unwrap() - 6.0).abs() < 1e-12);
        assert!((parse("1 in").unwrap().eval(&lookup).unwrap() - 25.4).abs() < 1e-12);
        assert!(parse("Pad.Length").unwrap().eval(&lookup).is_err());
        assert!(parse("1 / 0").unwrap().eval(&lookup).is_err());
        assert!(parse("width").is_err());
        assert!(parse("2 +").is_err());
        assert!(parse("(2").is_err());
        assert!(parse("2 $ 3").is_err());
    }
}
