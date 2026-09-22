use super::*;

/// A shell variable or function name: letters, digits and `_`, never leading a digit.
pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
}
/// The unquoted text of a one-part word; keywords are only keywords unquoted, so
/// `"if"` and `\if` stay ordinary arguments.
pub(super) fn bare(t: Option<&Token>) -> Option<&str> {
    match t {
        Some(Token::Word(parts)) if parts.len() == 1 && parts[0].1 & 2 != 0 => Some(&parts[0].0),
        _ => None,
    }
}
const KEYWORDS: &[&str] = &[
    "if", "then", "elif", "else", "fi", "for", "in", "while", "until", "do", "done", "case",
    "esac", "function", "{", "}",
];
pub(super) struct Grammar<'a> {
    t: &'a [Token],
    i: usize,
}
impl<'a> Grammar<'a> {
    pub(super) fn program(tokens: &'a [Token]) -> Result<Vec<Node>, String> {
        let mut g = Grammar { t: tokens, i: 0 };
        let nodes = g.list(&[])?;
        if g.i < g.t.len() {
            return Err(match &g.t[g.i] {
                Token::Op(op) => format!("unexpected `{op}`"),
                Token::Word(_) => format!("unexpected `{}`", bare(g.t.get(g.i)).unwrap_or("word")),
            });
        }
        Ok(nodes)
    }
    fn op(&self, offset: usize) -> Option<&str> {
        match self.t.get(self.i + offset) {
            Some(Token::Op(s)) => Some(s.as_str()),
            _ => None,
        }
    }
    fn word(&self, offset: usize) -> Option<&str> {
        bare(self.t.get(self.i + offset))
    }
    /// `;;` ends a `case` arm; the lexer emits it as two separators.
    fn double_semicolon(&self) -> bool {
        self.op(0) == Some(";") && self.op(1) == Some(";")
    }
    fn stops(&self, terminators: &[&str]) -> bool {
        if self.i >= self.t.len() {
            return true;
        }
        if terminators.contains(&";;") && self.double_semicolon() {
            return true;
        }
        self.op(0).is_some_and(|o| terminators.contains(&o))
            || self.word(0).is_some_and(|w| terminators.contains(&w))
    }
    fn list(&mut self, terminators: &[&str]) -> Result<Vec<Node>, String> {
        let mut nodes = Vec::new();
        loop {
            while matches!(self.op(0), Some(";" | "\n" | "&")) {
                if terminators.contains(&";;") && self.double_semicolon() {
                    break;
                }
                self.i += 1;
            }
            if self.stops(terminators) {
                return Ok(nodes);
            }
            nodes.push(self.statement()?);
            // Anything left that is not a separator is a stray operator, not silence.
            if let Some(op) = self.op(0).filter(|o| !matches!(*o, ";" | "\n" | "&" | ")")) {
                return Err(format!("unexpected `{op}`"));
            }
        }
    }
    fn expect_word(&mut self, name: &str) -> Result<(), String> {
        if self.word(0) == Some(name) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected `{name}`"))
        }
    }
    fn expect_op(&mut self, name: &str) -> Result<(), String> {
        if self.op(0) == Some(name) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected `{name}`"))
        }
    }
    /// A word that can only begin a compound command, so a gate in front of it means
    /// the pipeline continues into that compound rather than into another argument.
    fn compound_ahead(&self, offset: usize) -> bool {
        self.op(offset) == Some("(")
            || bare(self.t.get(self.i + offset)).is_some_and(|w| {
                matches!(
                    w,
                    "if" | "for" | "while" | "until" | "case" | "function" | "{" | "[["
                )
            })
    }
    fn statement(&mut self) -> Result<Node, String> {
        let mut node = self.unit()?;
        // A gate is consumed here only when `unit` stopped at one, which happens when
        // a compound follows it or the unit itself was compound.
        while let Some(gate) = self.op(0).filter(|o| matches!(*o, "|" | "&&" | "||")) {
            let gate = gate.to_string();
            self.i += 1;
            if self.i >= self.t.len() {
                return Err(format!("missing command after `{gate}`"));
            }
            node = Node::Chain {
                head: Box::new(node),
                gate,
                tail: Box::new(self.unit()?),
            };
        }
        Ok(node)
    }
    /// One pipeline element: a compound (with any redirections of its own) or a run
    /// of tokens the older and-or evaluator understands.
    fn unit(&mut self) -> Result<Node, String> {
        if self.compound_ahead(0) {
            let node = self.compound()?;
            return self.redirections(node);
        }
        self.simple()
    }
    /// Trailing `< f`, `> f`, `2>&1` and friends on a compound command.
    fn redirections(&mut self, node: Node) -> Result<Node, String> {
        let mut ops = Vec::new();
        while let Some(op) = self
            .op(0)
            .filter(|o| *o == "<" || *o == "<<" || o.contains('>'))
        {
            let op = op.to_string();
            self.i += 1;
            if op.ends_with("&1") || op.ends_with("&2") {
                ops.push((op, None));
                continue;
            }
            let target = self
                .t
                .get(self.i)
                .filter(|t| matches!(t, Token::Word(_)))
                .ok_or_else(|| format!("missing target for `{op}`"))?
                .clone();
            self.i += 1;
            ops.push((op, Some(target)));
        }
        Ok(if ops.is_empty() {
            node
        } else {
            Node::Redirect {
                body: Box::new(node),
                ops,
            }
        })
    }
    fn compound(&mut self) -> Result<Node, String> {
        if self.op(0) == Some("(") {
            self.i += 1;
            let body = self.list(&[")"])?;
            self.expect_op(")")?;
            return Ok(Node::Subshell(body));
        }
        match self.word(0) {
            Some("{") => {
                self.i += 1;
                let body = self.list(&["}"])?;
                self.expect_word("}")?;
                return Ok(Node::Group(body));
            }
            Some("if") => return self.parse_if(),
            Some("for") => return self.parse_for(),
            Some("while") => return self.parse_loop(false),
            Some("until") => return self.parse_loop(true),
            Some("case") => return self.parse_case(),
            Some("[[") => {
                self.i += 1;
                let start = self.i;
                while self.i < self.t.len() && self.word(0) != Some("]]") {
                    self.i += 1;
                }
                if self.i >= self.t.len() {
                    return Err("expected `]]`".into());
                }
                let body = self.t[start..self.i].to_vec();
                self.i += 1;
                return Ok(Node::Conditional(body));
            }
            Some("function") => {
                self.i += 1;
                let name = self.function_name()?;
                if self.op(0) == Some("(") && self.op(1) == Some(")") {
                    self.i += 2;
                }
                return self.parse_body(name);
            }
            _ => {}
        }
        Err("expected a compound command".into())
    }
    fn simple(&mut self) -> Result<Node, String> {
        if self.op(1) == Some("(") && self.op(2) == Some(")") && self.word(0).is_some() {
            let name = self.function_name()?;
            self.i += 2;
            return self.parse_body(name);
        }
        let start = self.i;
        while self.i < self.t.len() && !matches!(self.op(0), Some(";" | "\n" | "&" | ")")) {
            // A gate in front of a compound ends this run; `statement` chains them.
            if matches!(self.op(0), Some("|" | "&&" | "||")) && self.compound_ahead(1) {
                break;
            }
            self.i += 1;
        }
        if self.i == start {
            return Err(format!("unexpected `{}`", self.op(0).unwrap_or("token")));
        }
        Ok(Node::Simple(self.t[start..self.i].to_vec()))
    }
    fn function_name(&mut self) -> Result<String, String> {
        let name = self
            .word(0)
            .filter(|n| !KEYWORDS.contains(n) && valid_name(n))
            .ok_or("invalid function name")?
            .to_string();
        self.i += 1;
        Ok(name)
    }
    fn parse_body(&mut self, name: String) -> Result<Node, String> {
        while matches!(self.op(0), Some(";" | "\n")) {
            self.i += 1;
        }
        self.expect_word("{")?;
        let body = self.list(&["}"])?;
        self.expect_word("}")?;
        Ok(Node::Function {
            name,
            body: std::sync::Arc::new(body),
        })
    }
    fn parse_if(&mut self) -> Result<Node, String> {
        let mut branches = Vec::new();
        let mut otherwise = Vec::new();
        self.expect_word("if")?;
        loop {
            let condition = self.list(&["then"])?;
            self.expect_word("then")?;
            branches.push((condition, self.list(&["elif", "else", "fi"])?));
            match self.word(0) {
                Some("elif") => self.i += 1,
                Some("else") => {
                    self.i += 1;
                    otherwise = self.list(&["fi"])?;
                    break;
                }
                _ => break,
            }
        }
        self.expect_word("fi")?;
        Ok(Node::If {
            branches,
            otherwise,
        })
    }
    fn parse_for(&mut self) -> Result<Node, String> {
        self.expect_word("for")?;
        let name = self.function_name()?;
        let mut words = None;
        if self.word(0) == Some("in") {
            self.i += 1;
            let mut collected = Vec::new();
            while self.i < self.t.len() && self.op(0).is_none() && self.word(0) != Some("do") {
                collected.push(self.t[self.i].clone());
                self.i += 1;
            }
            words = Some(collected);
        }
        while matches!(self.op(0), Some(";" | "\n")) {
            self.i += 1;
        }
        self.expect_word("do")?;
        let body = self.list(&["done"])?;
        self.expect_word("done")?;
        Ok(Node::For { name, words, body })
    }
    fn parse_loop(&mut self, until: bool) -> Result<Node, String> {
        self.expect_word(if until { "until" } else { "while" })?;
        let condition = self.list(&["do"])?;
        self.expect_word("do")?;
        let body = self.list(&["done"])?;
        self.expect_word("done")?;
        Ok(Node::Loop {
            until,
            condition,
            body,
        })
    }
    fn parse_case(&mut self) -> Result<Node, String> {
        self.expect_word("case")?;
        let subject = self
            .t
            .get(self.i)
            .filter(|t| matches!(t, Token::Word(_)))
            .ok_or("case needs a word to match")?
            .clone();
        self.i += 1;
        while matches!(self.op(0), Some("\n")) {
            self.i += 1;
        }
        self.expect_word("in")?;
        let mut arms = Vec::new();
        loop {
            while matches!(self.op(0), Some(";" | "\n")) {
                self.i += 1;
            }
            if self.word(0) == Some("esac") {
                self.i += 1;
                break;
            }
            if self.i >= self.t.len() {
                return Err("expected `esac`".into());
            }
            // A leading `(` is optional, as in `(a|b)`.
            if self.op(0) == Some("(") {
                self.i += 1;
            }
            let mut patterns = Vec::new();
            loop {
                let pattern = self
                    .t
                    .get(self.i)
                    .filter(|t| matches!(t, Token::Word(_)))
                    .ok_or("case pattern expected")?
                    .clone();
                patterns.push(pattern);
                self.i += 1;
                if self.op(0) == Some("|") {
                    self.i += 1;
                } else {
                    break;
                }
            }
            self.expect_op(")")?;
            let body = self.list(&[";;", "esac"])?;
            if self.double_semicolon() {
                self.i += 2;
            }
            arms.push((patterns, body));
        }
        Ok(Node::Case { subject, arms })
    }
}
