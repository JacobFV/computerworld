//! Recursive-descent parser for the supported SQLite dialect.
use crate::ast::*;
use crate::lexer::{tokenize, Tok, Token};
use crate::value::Value;
use crate::SqlError;

/// Words that end an expression or cannot start an implicit alias.
const RESERVED: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "AS",
    "ASC",
    "BETWEEN",
    "BY",
    "CASE",
    "CHECK",
    "COLLATE",
    "COMMIT",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DO",
    "DROP",
    "ELSE",
    "END",
    "ESCAPE",
    "EXCEPT",
    "EXISTS",
    "FILTER",
    "FOREIGN",
    "FROM",
    "FULL",
    "GLOB",
    "GROUP",
    "HAVING",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "ISNULL",
    "JOIN",
    "LEFT",
    "LIKE",
    "LIMIT",
    "MATCH",
    "NATURAL",
    "NOT",
    "NOTHING",
    "NOTNULL",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PRIMARY",
    "REFERENCES",
    "REGEXP",
    "RETURNING",
    "RIGHT",
    "SELECT",
    "SET",
    "TABLE",
    "THEN",
    "TO",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

/// Whether a bare word would be read as a keyword rather than a name.
pub fn is_keyword(word: &str) -> bool {
    RESERVED.contains(&word.to_ascii_uppercase().as_str())
}

pub struct Parser<'a> {
    sql: &'a str,
    toks: Vec<Token>,
    pos: usize,
    /// Parameter names by number (empty for positional ones).
    pub params: Vec<String>,
}

/// Parse every statement in `sql`.
pub fn parse(sql: &str) -> Result<Vec<Stmt>, SqlError> {
    let mut p = Parser::new(sql)?;
    let mut out = Vec::new();
    loop {
        while p.eat_semi() {}
        if p.at_eof() {
            break;
        }
        out.push(p.statement()?);
        if !p.at_eof() && !p.eat_semi() {
            return Err(p.error_here());
        }
    }
    Ok(out)
}
/// Parse a single expression (a DEFAULT or CHECK clause's text).
pub fn parse_expr(sql: &str) -> Result<Expr, SqlError> {
    let mut p = Parser::new(sql)?;
    let e = p.expr()?;
    if !p.at_eof() {
        return Err(p.error_here());
    }
    Ok(e)
}

impl<'a> Parser<'a> {
    pub fn new(sql: &'a str) -> Result<Self, SqlError> {
        Ok(Self {
            sql,
            toks: tokenize(sql)?,
            pos: 0,
            params: Vec::new(),
        })
    }
    pub fn at_end(&self) -> bool {
        self.at_eof()
    }
    pub fn skip_semi(&mut self) -> bool {
        self.eat_semi()
    }
    /// A syntax error at the current token.
    pub fn unexpected(&self) -> SqlError {
        self.error_here()
    }
    fn peek(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }
    fn peek_at(&self, n: usize) -> &Token {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)]
    }
    fn next(&mut self) -> Token {
        let t = self.peek().clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn at_eof(&self) -> bool {
        matches!(self.peek().tok, Tok::Eof)
    }
    fn eat_semi(&mut self) -> bool {
        if matches!(self.peek().tok, Tok::Semi) {
            self.next();
            true
        } else {
            false
        }
    }
    fn error_here(&self) -> SqlError {
        let t = self.peek();
        match t.tok {
            Tok::Eof => SqlError::syntax_at("incomplete input", t.start),
            _ => SqlError::syntax_at(
                format!("near \"{}\": syntax error", &self.sql[t.start..t.end]),
                t.start,
            ),
        }
    }
    fn is_kw(&self, kw: &str) -> bool {
        self.peek().keyword().as_deref() == Some(kw)
    }
    fn is_kw_at(&self, n: usize, kw: &str) -> bool {
        self.peek_at(n).keyword().as_deref() == Some(kw)
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect_kw(&mut self, kw: &str) -> Result<(), SqlError> {
        if self.eat_kw(kw) {
            Ok(())
        } else {
            Err(self.error_here())
        }
    }
    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek().tok, Tok::Op(o) if o == op)
    }
    fn eat_op(&mut self, op: &str) -> bool {
        if self.is_op(op) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect_op(&mut self, op: &str) -> Result<(), SqlError> {
        if self.eat_op(op) {
            Ok(())
        } else {
            Err(self.error_here())
        }
    }
    /// An identifier (quoted or not), or a string literal where SQLite accepts one as a name.
    fn name(&mut self) -> Result<String, SqlError> {
        match &self.peek().tok {
            Tok::Ident { name, quoted } => {
                if !quoted && RESERVED.contains(&name.to_ascii_uppercase().as_str()) {
                    return Err(self.error_here());
                }
                let n = name.clone();
                self.next();
                Ok(n)
            }
            Tok::Str(s) => {
                let n = s.clone();
                self.next();
                Ok(n)
            }
            _ => Err(self.error_here()),
        }
    }
    /// A name that may be a reserved word (after a `.`, or a type name).
    fn any_name(&mut self) -> Result<String, SqlError> {
        match &self.peek().tok {
            Tok::Ident { name, .. } => {
                let n = name.clone();
                self.next();
                Ok(n)
            }
            Tok::Str(s) => {
                let n = s.clone();
                self.next();
                Ok(n)
            }
            _ => Err(self.error_here()),
        }
    }
    /// `[schema.]name`; only the `main` schema exists.
    fn qualified_name(&mut self) -> Result<String, SqlError> {
        let first = self.name()?;
        if self.eat_op(".") {
            if !first.eq_ignore_ascii_case("main") && !first.eq_ignore_ascii_case("temp") {
                return Err(SqlError::new(format!("unknown database {first}")));
            }
            return self.any_name();
        }
        Ok(first)
    }
    fn text_from(&self, start: usize) -> String {
        let end = self.toks[self.pos.saturating_sub(1)].end;
        self.sql[start..end.max(start)].to_owned()
    }

    pub fn statement(&mut self) -> Result<Stmt, SqlError> {
        let kw = self.peek().keyword().unwrap_or_default();
        match kw.as_str() {
            "SELECT" | "VALUES" | "WITH" => {
                let with = if self.is_kw("WITH") {
                    Some(self.with_clause()?)
                } else {
                    None
                };
                match self.peek().keyword().as_deref() {
                    Some("INSERT") | Some("REPLACE") => {
                        let mut insert = self.insert()?;
                        insert.with = with;
                        Ok(Stmt::Insert(Box::new(insert)))
                    }
                    Some("UPDATE") => {
                        let mut update = self.update()?;
                        update.with = with;
                        Ok(Stmt::Update(Box::new(update)))
                    }
                    Some("DELETE") => {
                        let mut delete = self.delete()?;
                        delete.with = with;
                        Ok(Stmt::Delete(Box::new(delete)))
                    }
                    _ => {
                        let mut select = self.select_body()?;
                        select.with = with;
                        Ok(Stmt::Select(Box::new(select)))
                    }
                }
            }
            "INSERT" | "REPLACE" => Ok(Stmt::Insert(Box::new(self.insert()?))),
            "UPDATE" => Ok(Stmt::Update(Box::new(self.update()?))),
            "DELETE" => Ok(Stmt::Delete(Box::new(self.delete()?))),
            "CREATE" => self.create(),
            "DROP" => self.drop(),
            "ALTER" => self.alter(),
            "BEGIN" => {
                self.next();
                let _ =
                    self.eat_kw("DEFERRED") || self.eat_kw("IMMEDIATE") || self.eat_kw("EXCLUSIVE");
                self.eat_kw("TRANSACTION");
                Ok(Stmt::Begin)
            }
            "COMMIT" | "END" => {
                self.next();
                self.eat_kw("TRANSACTION");
                Ok(Stmt::Commit)
            }
            "ROLLBACK" => {
                self.next();
                self.eat_kw("TRANSACTION");
                let savepoint = if self.eat_kw("TO") {
                    self.eat_kw("SAVEPOINT");
                    Some(self.name()?)
                } else {
                    None
                };
                Ok(Stmt::Rollback { savepoint })
            }
            "SAVEPOINT" => {
                self.next();
                Ok(Stmt::Savepoint(self.name()?))
            }
            "RELEASE" => {
                self.next();
                self.eat_kw("SAVEPOINT");
                Ok(Stmt::Release(self.name()?))
            }
            "PRAGMA" => self.pragma(),
            "EXPLAIN" => {
                self.next();
                let query_plan = self.eat_kw("QUERY");
                if query_plan {
                    self.expect_kw("PLAN")?;
                }
                Ok(Stmt::Explain {
                    query_plan,
                    stmt: Box::new(self.statement()?),
                })
            }
            "VACUUM" => {
                self.next();
                Ok(Stmt::Vacuum)
            }
            "ANALYZE" => {
                self.next();
                if !self.at_eof() && !matches!(self.peek().tok, Tok::Semi) {
                    self.qualified_name()?;
                }
                Ok(Stmt::Analyze)
            }
            "REINDEX" => {
                self.next();
                if !self.at_eof() && !matches!(self.peek().tok, Tok::Semi) {
                    self.qualified_name()?;
                }
                Ok(Stmt::Reindex)
            }
            "ATTACH" | "DETACH" => Err(SqlError::new(
                "ATTACH and DETACH are not supported: this engine opens one database file",
            )),
            _ => Err(self.error_here()),
        }
    }

    fn pragma(&mut self) -> Result<Stmt, SqlError> {
        self.next();
        let name = self.qualified_name()?.to_ascii_lowercase();
        let arg = if self.eat_op("=") {
            Some(PragmaArg::Set(self.pragma_value()?))
        } else if self.eat_op("(") {
            let v = self.pragma_value()?;
            self.expect_op(")")?;
            Some(PragmaArg::Call(v.to_text()))
        } else {
            None
        };
        Ok(Stmt::Pragma { name, arg })
    }
    fn pragma_value(&mut self) -> Result<Value, SqlError> {
        let negative = self.eat_op("-");
        let t = self.next();
        let v = match t.tok {
            Tok::Int(s) => Value::Integer(parse_int(&s).unwrap_or(0)),
            Tok::Real(s) => Value::Real(s.parse().unwrap_or(0.0)),
            Tok::Str(s) => Value::Text(s),
            Tok::Ident { name, .. } => Value::Text(name),
            _ => {
                return Err(SqlError::syntax_at(
                    "near pragma value: syntax error",
                    t.start,
                ))
            }
        };
        Ok(match (negative, v) {
            (true, Value::Integer(i)) => Value::Integer(-i),
            (true, Value::Real(r)) => Value::Real(-r),
            (_, v) => v,
        })
    }

    fn with_clause(&mut self) -> Result<With, SqlError> {
        self.expect_kw("WITH")?;
        let recursive = self.eat_kw("RECURSIVE");
        let mut ctes = Vec::new();
        loop {
            let name = self.name()?;
            let mut columns = Vec::new();
            if self.eat_op("(") {
                loop {
                    columns.push(self.name()?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
            }
            self.expect_kw("AS")?;
            if self.eat_kw("NOT") {
                self.expect_kw("MATERIALIZED")?;
            } else {
                self.eat_kw("MATERIALIZED");
            }
            self.expect_op("(")?;
            let select = self.select()?;
            self.expect_op(")")?;
            ctes.push(Cte {
                name,
                columns,
                select: Box::new(select),
            });
            if !self.eat_op(",") {
                break;
            }
        }
        Ok(With { recursive, ctes })
    }
    pub fn select(&mut self) -> Result<Select, SqlError> {
        let with = if self.is_kw("WITH") {
            Some(self.with_clause()?)
        } else {
            None
        };
        let mut s = self.select_body()?;
        s.with = with;
        Ok(s)
    }
    fn select_body(&mut self) -> Result<Select, SqlError> {
        let first = self.select_core()?;
        let mut compounds = Vec::new();
        loop {
            let op = if self.eat_kw("UNION") {
                if self.eat_kw("ALL") {
                    CompoundOp::UnionAll
                } else {
                    CompoundOp::Union
                }
            } else if self.eat_kw("INTERSECT") {
                CompoundOp::Intersect
            } else if self.eat_kw("EXCEPT") {
                CompoundOp::Except
            } else {
                break;
            };
            compounds.push((op, self.select_core()?));
        }
        let mut order_by = Vec::new();
        if self.eat_kw("ORDER") {
            self.expect_kw("BY")?;
            order_by = self.order_terms()?;
        }
        let (mut limit, mut offset) = (None, None);
        if self.eat_kw("LIMIT") {
            let first = self.expr()?;
            if self.eat_kw("OFFSET") {
                limit = Some(first);
                offset = Some(self.expr()?);
            } else if self.eat_op(",") {
                offset = Some(first);
                limit = Some(self.expr()?);
            } else {
                limit = Some(first);
            }
        }
        Ok(Select {
            with: None,
            first,
            compounds,
            order_by,
            limit,
            offset,
        })
    }
    fn order_terms(&mut self) -> Result<Vec<OrderTerm>, SqlError> {
        let mut out = Vec::new();
        loop {
            let expr = self.expr()?;
            let desc = if self.eat_kw("DESC") {
                true
            } else {
                self.eat_kw("ASC");
                false
            };
            let nulls_first = if self.eat_kw("NULLS") {
                if self.eat_kw("FIRST") {
                    Some(true)
                } else {
                    self.expect_kw("LAST")?;
                    Some(false)
                }
            } else {
                None
            };
            out.push(OrderTerm {
                expr,
                desc,
                nulls_first,
            });
            if !self.eat_op(",") {
                return Ok(out);
            }
        }
    }
    fn select_core(&mut self) -> Result<SelectCore, SqlError> {
        if self.eat_kw("VALUES") {
            return Ok(SelectCore::Values(self.values_rows()?));
        }
        self.expect_kw("SELECT")?;
        let distinct = if self.eat_kw("DISTINCT") {
            true
        } else {
            self.eat_kw("ALL");
            false
        };
        let mut columns = Vec::new();
        loop {
            columns.push(self.result_col()?);
            if !self.eat_op(",") {
                break;
            }
        }
        let from = if self.eat_kw("FROM") {
            Some(self.parse_from()?)
        } else {
            None
        };
        let filter = if self.eat_kw("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        let mut group_by = Vec::new();
        let mut having = None;
        if self.eat_kw("GROUP") {
            self.expect_kw("BY")?;
            loop {
                group_by.push(self.expr()?);
                if !self.eat_op(",") {
                    break;
                }
            }
        }
        if self.eat_kw("HAVING") {
            having = Some(self.expr()?);
        }
        if self.is_kw("WINDOW") {
            return Err(SqlError::new("window functions are not supported"));
        }
        Ok(SelectCore::Select {
            distinct,
            columns,
            from,
            filter,
            group_by,
            having,
        })
    }
    fn values_rows(&mut self) -> Result<Vec<Vec<Expr>>, SqlError> {
        let mut rows = Vec::new();
        loop {
            self.expect_op("(")?;
            let mut row = Vec::new();
            loop {
                row.push(self.expr()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
            if let Some(first) = rows.first() {
                let first: &Vec<Expr> = first;
                if first.len() != row.len() {
                    return Err(SqlError::new(
                        "all VALUES must have the same number of terms",
                    ));
                }
            }
            rows.push(row);
            if !self.eat_op(",") {
                return Ok(rows);
            }
        }
    }
    fn result_col(&mut self) -> Result<ResultCol, SqlError> {
        if self.eat_op("*") {
            return Ok(ResultCol::Star);
        }
        if matches!(self.peek().tok, Tok::Ident { .. })
            && matches!(self.peek_at(1).tok, Tok::Op("."))
            && matches!(self.peek_at(2).tok, Tok::Op("*"))
        {
            let name = self.any_name()?;
            self.next();
            self.next();
            return Ok(ResultCol::TableStar(name));
        }
        let start = self.peek().start;
        let expr = self.expr()?;
        let text = self.text_from(start);
        let alias = self.alias()?;
        Ok(ResultCol::Expr { expr, alias, text })
    }
    fn alias(&mut self) -> Result<Option<String>, SqlError> {
        if self.eat_kw("AS") {
            return Ok(Some(self.any_name()?));
        }
        match &self.peek().tok {
            Tok::Ident { name, quoted } => {
                if *quoted || !RESERVED.contains(&name.to_ascii_uppercase().as_str()) {
                    let n = name.clone();
                    self.next();
                    return Ok(Some(n));
                }
                Ok(None)
            }
            Tok::Str(s) => {
                let n = s.clone();
                self.next();
                Ok(Some(n))
            }
            _ => Ok(None),
        }
    }
    fn parse_from(&mut self) -> Result<FromItem, SqlError> {
        let mut left = self.table_or_subquery()?;
        loop {
            let kind = if self.eat_op(",") {
                JoinKind::Cross
            } else if self.eat_kw("CROSS") {
                self.expect_kw("JOIN")?;
                JoinKind::Cross
            } else {
                let natural = self.eat_kw("NATURAL");
                let kind = if self.eat_kw("LEFT") {
                    self.eat_kw("OUTER");
                    JoinKind::Left
                } else if self.eat_kw("RIGHT") {
                    self.eat_kw("OUTER");
                    JoinKind::Right
                } else if self.eat_kw("FULL") {
                    self.eat_kw("OUTER");
                    JoinKind::Full
                } else if self.eat_kw("INNER") || self.is_kw("JOIN") {
                    JoinKind::Inner
                } else if natural {
                    return Err(self.error_here());
                } else {
                    break;
                };
                self.expect_kw("JOIN")?;
                let right = self.table_or_subquery()?;
                let constraint = if natural {
                    JoinConstraint::Natural
                } else {
                    self.join_constraint()?
                };
                left = FromItem::Join {
                    left: Box::new(left),
                    right: Box::new(right),
                    kind,
                    constraint,
                };
                continue;
            };
            let right = self.table_or_subquery()?;
            let constraint = self.join_constraint()?;
            left = FromItem::Join {
                left: Box::new(left),
                right: Box::new(right),
                kind,
                constraint,
            };
        }
        Ok(left)
    }
    fn join_constraint(&mut self) -> Result<JoinConstraint, SqlError> {
        if self.eat_kw("ON") {
            return Ok(JoinConstraint::On(self.expr()?));
        }
        if self.eat_kw("USING") {
            self.expect_op("(")?;
            let mut cols = Vec::new();
            loop {
                cols.push(self.name()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
            return Ok(JoinConstraint::Using(cols));
        }
        Ok(JoinConstraint::None)
    }
    fn table_or_subquery(&mut self) -> Result<FromItem, SqlError> {
        if self.eat_op("(") {
            if self.is_kw("SELECT") || self.is_kw("VALUES") || self.is_kw("WITH") {
                let select = self.select()?;
                self.expect_op(")")?;
                let alias = self.alias()?;
                return Ok(FromItem::Subquery {
                    select: Box::new(select),
                    alias,
                });
            }
            let inner = self.parse_from()?;
            self.expect_op(")")?;
            return Ok(inner);
        }
        let name = self.qualified_name()?;
        if self.is_op("(") {
            return Err(SqlError::new(format!(
                "no such table-valued function: {name}"
            )));
        }
        let alias = self.alias()?;
        if self.eat_kw("INDEXED") {
            self.expect_kw("BY")?;
            self.name()?;
        } else if self.is_kw("NOT") && self.is_kw_at(1, "INDEXED") {
            self.next();
            self.next();
        }
        Ok(FromItem::Table { name, alias })
    }

    fn conflict_clause(&mut self) -> Result<Conflict, SqlError> {
        Ok(if self.eat_kw("OR") {
            let t = self.next();
            match t.keyword().as_deref() {
                Some("REPLACE") => Conflict::Replace,
                Some("IGNORE") => Conflict::Ignore,
                Some("ABORT") => Conflict::Abort,
                Some("FAIL") => Conflict::Fail,
                Some("ROLLBACK") => Conflict::Rollback,
                _ => return Err(SqlError::syntax_at("near \"OR\": syntax error", t.start)),
            }
        } else {
            Conflict::Abort
        })
    }
    fn insert(&mut self) -> Result<Insert, SqlError> {
        let conflict = if self.eat_kw("REPLACE") {
            Conflict::Replace
        } else {
            self.expect_kw("INSERT")?;
            self.conflict_clause()?
        };
        self.expect_kw("INTO")?;
        let table = self.qualified_name()?;
        if self.eat_kw("AS") {
            self.name()?;
        }
        let mut columns = Vec::new();
        if self.eat_op("(") {
            loop {
                columns.push(self.name()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
        }
        let source = if self.eat_kw("DEFAULT") {
            self.expect_kw("VALUES")?;
            InsertSource::Default
        } else if self.eat_kw("VALUES") {
            InsertSource::Values(self.values_rows()?)
        } else {
            InsertSource::Select(Box::new(self.select()?))
        };
        let mut upsert = None;
        if self.eat_kw("ON") {
            self.expect_kw("CONFLICT")?;
            let mut target = Vec::new();
            if self.eat_op("(") {
                loop {
                    target.push(self.name()?);
                    if self.eat_kw("COLLATE") {
                        self.name()?;
                    }
                    self.eat_kw("ASC");
                    self.eat_kw("DESC");
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
                if self.eat_kw("WHERE") {
                    self.expr()?;
                }
            }
            self.expect_kw("DO")?;
            let update = if self.eat_kw("NOTHING") {
                None
            } else {
                self.expect_kw("UPDATE")?;
                self.expect_kw("SET")?;
                let sets = self.set_list()?;
                let filter = if self.eat_kw("WHERE") {
                    Some(self.expr()?)
                } else {
                    None
                };
                Some((sets, filter))
            };
            upsert = Some(Upsert { target, update });
        }
        let returning = self.returning()?;
        Ok(Insert {
            with: None,
            conflict,
            table,
            columns,
            source,
            upsert,
            returning,
        })
    }
    fn returning(&mut self) -> Result<Vec<ResultCol>, SqlError> {
        let mut out = Vec::new();
        if self.eat_kw("RETURNING") {
            loop {
                out.push(self.result_col()?);
                if !self.eat_op(",") {
                    break;
                }
            }
        }
        Ok(out)
    }
    fn set_list(&mut self) -> Result<Vec<(Vec<String>, Expr)>, SqlError> {
        let mut sets = Vec::new();
        loop {
            let cols = if self.eat_op("(") {
                let mut cols = Vec::new();
                loop {
                    cols.push(self.name()?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
                cols
            } else {
                vec![self.name()?]
            };
            self.expect_op("=")?;
            sets.push((cols, self.expr()?));
            if !self.eat_op(",") {
                return Ok(sets);
            }
        }
    }
    fn update(&mut self) -> Result<Update, SqlError> {
        self.expect_kw("UPDATE")?;
        let conflict = self.conflict_clause()?;
        let table = self.qualified_name()?;
        let alias = if self.eat_kw("AS") {
            Some(self.name()?)
        } else {
            None
        };
        self.expect_kw("SET")?;
        let sets = self.set_list()?;
        let from = if self.eat_kw("FROM") {
            Some(self.parse_from()?)
        } else {
            None
        };
        let filter = if self.eat_kw("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        let returning = self.returning()?;
        Ok(Update {
            with: None,
            conflict,
            table,
            alias,
            sets,
            from,
            filter,
            returning,
        })
    }
    fn delete(&mut self) -> Result<Delete, SqlError> {
        self.expect_kw("DELETE")?;
        self.expect_kw("FROM")?;
        let table = self.qualified_name()?;
        let alias = if self.eat_kw("AS") {
            Some(self.name()?)
        } else {
            None
        };
        let filter = if self.eat_kw("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        let returning = self.returning()?;
        Ok(Delete {
            with: None,
            table,
            alias,
            filter,
            returning,
        })
    }

    fn if_not_exists(&mut self) -> Result<bool, SqlError> {
        if self.eat_kw("IF") {
            self.expect_kw("NOT")?;
            self.expect_kw("EXISTS")?;
            return Ok(true);
        }
        Ok(false)
    }
    fn create(&mut self) -> Result<Stmt, SqlError> {
        self.expect_kw("CREATE")?;
        let temporary = self.eat_kw("TEMP") || self.eat_kw("TEMPORARY");
        let unique = self.eat_kw("UNIQUE");
        if self.eat_kw("TABLE") {
            let if_not_exists = self.if_not_exists()?;
            let name_start = self.peek().start;
            let name = self.qualified_name()?;
            if self.eat_kw("AS") {
                let select = self.select()?;
                return Ok(Stmt::CreateTable(Box::new(CreateTable {
                    name,
                    if_not_exists,
                    temporary,
                    columns: vec![],
                    constraints: vec![],
                    as_select: Some(Box::new(select)),
                    without_rowid: false,
                    strict: false,
                    sql: String::new(),
                })));
            }
            self.expect_op("(")?;
            let mut columns = Vec::new();
            let mut constraints = Vec::new();
            loop {
                if self.is_kw("CONSTRAINT")
                    || self.is_kw("PRIMARY")
                    || self.is_kw("UNIQUE")
                    || self.is_kw("CHECK")
                    || self.is_kw("FOREIGN")
                {
                    constraints.push(self.table_constraint()?);
                } else {
                    if !constraints.is_empty() {
                        return Err(self.error_here());
                    }
                    columns.push(self.column_def()?.0);
                }
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
            let (mut without_rowid, mut strict) = (false, false);
            loop {
                if self.eat_kw("WITHOUT") {
                    let t = self.next();
                    if t.keyword().as_deref() != Some("ROWID") {
                        return Err(SqlError::syntax_at("unknown table option", t.start));
                    }
                    without_rowid = true;
                } else if self.eat_kw("STRICT") {
                    strict = true;
                } else {
                    break;
                }
                if !self.eat_op(",") {
                    break;
                }
            }
            let sql = format!("CREATE TABLE {}", self.text_from(name_start));
            return Ok(Stmt::CreateTable(Box::new(CreateTable {
                name,
                if_not_exists,
                temporary,
                columns,
                constraints,
                as_select: None,
                without_rowid,
                strict,
                sql,
            })));
        }
        if self.eat_kw("INDEX") {
            let if_not_exists = self.if_not_exists()?;
            let name_start = self.peek().start;
            let name = self.qualified_name()?;
            self.expect_kw("ON")?;
            let table = self.name()?;
            self.expect_op("(")?;
            let columns = self.indexed_columns()?;
            let filter = if self.eat_kw("WHERE") {
                Some(self.expr()?)
            } else {
                None
            };
            let prefix = if unique {
                "CREATE UNIQUE INDEX"
            } else {
                "CREATE INDEX"
            };
            let sql = format!("{prefix} {}", self.text_from(name_start));
            return Ok(Stmt::CreateIndex(Box::new(CreateIndex {
                name,
                table,
                unique,
                if_not_exists,
                columns,
                filter,
                sql,
            })));
        }
        if unique {
            return Err(self.error_here());
        }
        if self.eat_kw("VIEW") {
            let if_not_exists = self.if_not_exists()?;
            let name_start = self.peek().start;
            let name = self.qualified_name()?;
            let mut columns = Vec::new();
            if self.eat_op("(") {
                loop {
                    columns.push(self.name()?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
            }
            self.expect_kw("AS")?;
            let select = self.select()?;
            let sql = format!("CREATE VIEW {}", self.text_from(name_start));
            return Ok(Stmt::CreateView(Box::new(CreateView {
                name,
                if_not_exists,
                columns,
                select: Box::new(select),
                sql,
            })));
        }
        if self.eat_kw("TRIGGER") {
            return self.create_trigger(temporary);
        }
        if self.is_kw("VIRTUAL") {
            return Err(SqlError::new(
                "virtual tables are not supported by this engine",
            ));
        }
        Err(self.error_here())
    }
    /// `CREATE [TEMP] TRIGGER [IF NOT EXISTS] name [BEFORE|AFTER|INSTEAD OF]
    /// DELETE|INSERT|UPDATE [OF col, …] ON table [FOR EACH ROW] [WHEN expr]
    /// BEGIN stmt; … END`, with `CREATE TRIGGER` already read.
    fn create_trigger(&mut self, temporary: bool) -> Result<Stmt, SqlError> {
        let if_not_exists = self.if_not_exists()?;
        let name_start = self.peek().start;
        let name = self.qualified_name()?;
        let timing = if self.eat_kw("BEFORE") {
            TriggerTiming::Before
        } else if self.eat_kw("AFTER") {
            TriggerTiming::After
        } else if self.eat_kw("INSTEAD") {
            self.expect_kw("OF")?;
            TriggerTiming::InsteadOf
        } else {
            TriggerTiming::Before
        };
        let event = if self.eat_kw("INSERT") {
            TriggerEvent::Insert
        } else if self.eat_kw("DELETE") {
            TriggerEvent::Delete
        } else if self.eat_kw("UPDATE") {
            let mut cols = Vec::new();
            if self.eat_kw("OF") {
                loop {
                    cols.push(self.name()?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
            }
            TriggerEvent::Update(cols)
        } else {
            return Err(self.error_here());
        };
        self.expect_kw("ON")?;
        let table = self.qualified_name()?;
        if self.eat_kw("FOR") {
            self.expect_kw("EACH")?;
            // SQLite has row triggers only; FOR EACH STATEMENT is a syntax error there too.
            if !self.eat_kw("ROW") {
                return Err(self.error_here());
            }
        }
        let when = if self.eat_kw("WHEN") {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect_kw("BEGIN")?;
        let mut body = Vec::new();
        loop {
            if self.eat_kw("END") {
                break;
            }
            let at = self.peek().start;
            let stmt = self.statement()?;
            let cte = match &stmt {
                Stmt::Insert(i) => i.with.is_some(),
                Stmt::Update(u) => u.with.is_some(),
                Stmt::Delete(d) => d.with.is_some(),
                Stmt::Select(s) => s.with.is_some(),
                _ => {
                    let t = &self.sql[at..];
                    let word = t.split_whitespace().next().unwrap_or("");
                    return Err(SqlError::syntax_at(
                        format!("near \"{word}\": syntax error"),
                        at,
                    ));
                }
            };
            if cte {
                return Err(SqlError::syntax_at(
                    "near \"WITH\": cannot use WITH clause in a trigger program",
                    at,
                ));
            }
            body.push(stmt);
            if !self.eat_semi() {
                return Err(self.error_here());
            }
        }
        if body.is_empty() {
            return Err(self.error_here());
        }
        let sql = format!("CREATE TRIGGER {}", self.text_from(name_start));
        Ok(Stmt::CreateTrigger(Box::new(CreateTrigger {
            name,
            if_not_exists,
            temporary,
            timing,
            event,
            table,
            when,
            body,
            sql,
            name_at: name_start,
        })))
    }
    /// `(col [COLLATE c] [ASC|DESC], ...)` including the closing parenthesis.
    fn indexed_columns(&mut self) -> Result<Vec<IndexedColumn>, SqlError> {
        let mut out = Vec::new();
        loop {
            let name = self.name()?;
            if self.is_op("(") || self.is_op("+") || self.is_op("||") {
                return Err(SqlError::new("indexes on expressions are not supported"));
            }
            let collation = if self.eat_kw("COLLATE") {
                Some(self.name()?)
            } else {
                None
            };
            let desc = if self.eat_kw("DESC") {
                true
            } else {
                self.eat_kw("ASC");
                false
            };
            out.push(IndexedColumn {
                name,
                collation,
                desc,
            });
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op(")")?;
        Ok(out)
    }
    fn type_name(&mut self) -> Result<String, SqlError> {
        let mut words = Vec::new();
        while let Tok::Ident {
            name,
            quoted: false,
        } = &self.peek().tok
        {
            let upper = name.to_ascii_uppercase();
            if matches!(
                upper.as_str(),
                "CONSTRAINT"
                    | "PRIMARY"
                    | "NOT"
                    | "NULL"
                    | "UNIQUE"
                    | "CHECK"
                    | "DEFAULT"
                    | "COLLATE"
                    | "REFERENCES"
                    | "GENERATED"
                    | "AS"
            ) {
                break;
            }
            words.push(name.clone());
            self.next();
        }
        let mut t = words.join(" ");
        if !t.is_empty() && self.eat_op("(") {
            let start = self.peek().start;
            let mut depth = 1;
            while depth > 0 {
                let tok = self.next();
                match tok.tok {
                    Tok::Op("(") => depth += 1,
                    Tok::Op(")") => depth -= 1,
                    Tok::Eof => return Err(self.error_here()),
                    _ => {}
                }
            }
            let end = self.toks[self.pos - 1].start;
            t = format!("{t}({})", self.sql[start..end].trim());
        }
        Ok(t)
    }
    /// A column definition, returning it with its source text.
    pub fn column_def(&mut self) -> Result<(ColumnDef, String), SqlError> {
        let start = self.peek().start;
        let name = self.name()?;
        let type_name = self.type_name()?;
        let mut def = ColumnDef {
            name,
            type_name,
            ..ColumnDef::default()
        };
        loop {
            if self.eat_kw("CONSTRAINT") {
                self.name()?;
            }
            if self.eat_kw("PRIMARY") {
                self.expect_kw("KEY")?;
                let desc = if self.eat_kw("DESC") {
                    true
                } else {
                    self.eat_kw("ASC");
                    false
                };
                self.on_conflict()?;
                let autoincrement = self.eat_kw("AUTOINCREMENT");
                def.primary_key = Some(PrimaryKeySpec {
                    desc,
                    autoincrement,
                });
            } else if self.eat_kw("NOT") {
                self.expect_kw("NULL")?;
                self.on_conflict()?;
                def.not_null = true;
            } else if self.eat_kw("NULL") {
            } else if self.eat_kw("UNIQUE") {
                self.on_conflict()?;
                def.unique = true;
            } else if self.eat_kw("CHECK") {
                self.expect_op("(")?;
                let s = self.peek().start;
                self.expr()?;
                def.checks.push(self.text_from(s));
                self.expect_op(")")?;
            } else if self.eat_kw("DEFAULT") {
                let s = self.peek().start;
                if self.eat_op("(") {
                    let s = self.peek().start;
                    self.expr()?;
                    def.default = Some(self.text_from(s));
                    self.expect_op(")")?;
                } else {
                    if self.is_op("-") || self.is_op("+") {
                        self.next();
                    }
                    let t = self.next();
                    match t.tok {
                        Tok::Int(_)
                        | Tok::Real(_)
                        | Tok::Str(_)
                        | Tok::Blob(_)
                        | Tok::Ident { .. } => {}
                        _ => {
                            return Err(SqlError::syntax_at("near DEFAULT: syntax error", t.start))
                        }
                    }
                    def.default = Some(self.text_from(s));
                }
            } else if self.eat_kw("COLLATE") {
                def.collation = Some(self.name()?);
            } else if self.eat_kw("REFERENCES") {
                def.references = Some(self.foreign_key_clause()?);
            } else if self.is_kw("GENERATED") || self.is_kw("AS") {
                if self.eat_kw("GENERATED") {
                    self.expect_kw("ALWAYS")?;
                }
                self.expect_kw("AS")?;
                self.expect_op("(")?;
                let s = self.peek().start;
                self.expr()?;
                def.generated = Some(self.text_from(s));
                self.expect_op(")")?;
                let _ = self.eat_kw("STORED") || self.eat_kw("VIRTUAL");
            } else {
                break;
            }
        }
        Ok((def, self.text_from(start)))
    }
    fn on_conflict(&mut self) -> Result<(), SqlError> {
        if self.eat_kw("ON") {
            self.expect_kw("CONFLICT")?;
            let t = self.next();
            if !matches!(
                t.keyword().as_deref(),
                Some("ROLLBACK" | "ABORT" | "FAIL" | "IGNORE" | "REPLACE")
            ) {
                return Err(SqlError::syntax_at(
                    "near ON CONFLICT: syntax error",
                    t.start,
                ));
            }
        }
        Ok(())
    }
    fn foreign_key_clause(&mut self) -> Result<ForeignKeySpec, SqlError> {
        let table = self.name()?;
        let mut columns = Vec::new();
        if self.eat_op("(") {
            loop {
                columns.push(self.name()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
        }
        let mut spec = ForeignKeySpec {
            table,
            columns,
            ..Default::default()
        };
        loop {
            if self.eat_kw("ON") {
                let delete = if self.eat_kw("DELETE") {
                    true
                } else {
                    self.expect_kw("UPDATE")?;
                    false
                };
                let action = if self.eat_kw("CASCADE") {
                    FkAction::Cascade
                } else if self.eat_kw("RESTRICT") {
                    FkAction::Restrict
                } else if self.eat_kw("SET") {
                    if self.eat_kw("NULL") {
                        FkAction::SetNull
                    } else {
                        self.expect_kw("DEFAULT")?;
                        FkAction::SetDefault
                    }
                } else {
                    self.expect_kw("NO")?;
                    self.expect_kw("ACTION")?;
                    FkAction::NoAction
                };
                if delete {
                    spec.on_delete = action;
                } else {
                    spec.on_update = action;
                }
            } else if self.eat_kw("MATCH") {
                self.name()?;
            } else if self.is_kw("DEFERRABLE")
                || (self.is_kw("NOT") && self.is_kw_at(1, "DEFERRABLE"))
            {
                self.eat_kw("NOT");
                self.next();
                if self.eat_kw("INITIALLY") {
                    let _ = self.eat_kw("DEFERRED") || self.eat_kw("IMMEDIATE");
                }
            } else {
                break;
            }
        }
        Ok(spec)
    }
    fn table_constraint(&mut self) -> Result<TableConstraint, SqlError> {
        if self.eat_kw("CONSTRAINT") {
            self.name()?;
        }
        if self.eat_kw("PRIMARY") {
            self.expect_kw("KEY")?;
            self.expect_op("(")?;
            let cols = self.indexed_columns()?;
            self.on_conflict()?;
            let autoincrement = self.eat_kw("AUTOINCREMENT");
            return Ok(TableConstraint::PrimaryKey(cols, autoincrement));
        }
        if self.eat_kw("UNIQUE") {
            self.expect_op("(")?;
            let cols = self.indexed_columns()?;
            self.on_conflict()?;
            return Ok(TableConstraint::Unique(cols));
        }
        if self.eat_kw("CHECK") {
            self.expect_op("(")?;
            let s = self.peek().start;
            self.expr()?;
            let text = self.text_from(s);
            self.expect_op(")")?;
            return Ok(TableConstraint::Check(text));
        }
        self.expect_kw("FOREIGN")?;
        self.expect_kw("KEY")?;
        self.expect_op("(")?;
        let mut cols = Vec::new();
        loop {
            cols.push(self.name()?);
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op(")")?;
        self.expect_kw("REFERENCES")?;
        let spec = self.foreign_key_clause()?;
        Ok(TableConstraint::ForeignKey(cols, spec))
    }
    fn drop(&mut self) -> Result<Stmt, SqlError> {
        self.expect_kw("DROP")?;
        let t = self.next();
        let kind = match t.keyword().as_deref() {
            Some("TABLE") => ObjectKind::Table,
            Some("INDEX") => ObjectKind::Index,
            Some("VIEW") => ObjectKind::View,
            Some("TRIGGER") => ObjectKind::Trigger,
            _ => return Err(SqlError::syntax_at("near DROP: syntax error", t.start)),
        };
        let if_exists = if self.eat_kw("IF") {
            self.expect_kw("EXISTS")?;
            true
        } else {
            false
        };
        let name = self.qualified_name()?;
        Ok(Stmt::Drop {
            kind,
            name,
            if_exists,
        })
    }
    fn alter(&mut self) -> Result<Stmt, SqlError> {
        self.expect_kw("ALTER")?;
        self.expect_kw("TABLE")?;
        let table = self.qualified_name()?;
        if self.eat_kw("RENAME") {
            if self.eat_kw("TO") {
                let to = self.name()?;
                return Ok(Stmt::AlterTable(Box::new(AlterTable::Rename { table, to })));
            }
            self.eat_kw("COLUMN");
            let from = self.name()?;
            self.expect_kw("TO")?;
            let to = self.name()?;
            return Ok(Stmt::AlterTable(Box::new(AlterTable::RenameColumn {
                table,
                from,
                to,
            })));
        }
        if self.eat_kw("ADD") {
            self.eat_kw("COLUMN");
            let (column, text) = self.column_def()?;
            return Ok(Stmt::AlterTable(Box::new(AlterTable::AddColumn {
                table,
                column,
                text,
            })));
        }
        self.expect_kw("DROP")?;
        self.eat_kw("COLUMN");
        let column = self.name()?;
        Ok(Stmt::AlterTable(Box::new(AlterTable::DropColumn {
            table,
            column,
        })))
    }

    // ----- expressions, lowest precedence first -----
    pub fn expr(&mut self) -> Result<Expr, SqlError> {
        let mut left = self.and_expr()?;
        while self.eat_kw("OR") {
            let right = self.and_expr()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn and_expr(&mut self) -> Result<Expr, SqlError> {
        let mut left = self.not_expr()?;
        while self.eat_kw("AND") {
            let right = self.not_expr()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn not_expr(&mut self) -> Result<Expr, SqlError> {
        if self.eat_kw("NOT") {
            let inner = self.not_expr()?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(inner)));
        }
        self.equality()
    }
    fn equality(&mut self) -> Result<Expr, SqlError> {
        let mut left = self.comparison()?;
        loop {
            if self.eat_op("=") || self.eat_op("==") {
                let r = self.comparison()?;
                left = Expr::Binary(BinOp::Eq, Box::new(left), Box::new(r));
            } else if self.eat_op("!=") || self.eat_op("<>") {
                let r = self.comparison()?;
                left = Expr::Binary(BinOp::Ne, Box::new(left), Box::new(r));
            } else if self.eat_kw("IS") {
                let mut not = self.eat_kw("NOT");
                if self.eat_kw("DISTINCT") {
                    self.expect_kw("FROM")?;
                    not = !not;
                }
                let r = self.comparison()?;
                left = Expr::Is {
                    left: Box::new(left),
                    right: Box::new(r),
                    not,
                };
            } else if self.eat_kw("ISNULL") {
                left = Expr::Is {
                    left: Box::new(left),
                    right: Box::new(Expr::lit(Value::Null)),
                    not: false,
                };
            } else if self.eat_kw("NOTNULL") {
                left = Expr::Is {
                    left: Box::new(left),
                    right: Box::new(Expr::lit(Value::Null)),
                    not: true,
                };
            } else if self.is_kw("NOT") && self.is_kw_at(1, "NULL") {
                self.next();
                self.next();
                left = Expr::Is {
                    left: Box::new(left),
                    right: Box::new(Expr::lit(Value::Null)),
                    not: true,
                };
            } else {
                let not = self.is_kw("NOT")
                    && ["IN", "LIKE", "GLOB", "BETWEEN", "MATCH", "REGEXP"]
                        .iter()
                        .any(|k| self.is_kw_at(1, k));
                if not {
                    self.next();
                }
                if self.eat_kw("IN") {
                    left = self.in_rest(left, not)?;
                } else if self.is_kw("LIKE") || self.is_kw("GLOB") {
                    let op = if self.eat_kw("LIKE") {
                        LikeOp::Like
                    } else {
                        self.next();
                        LikeOp::Glob
                    };
                    let pattern = self.comparison()?;
                    let escape = if self.eat_kw("ESCAPE") {
                        Some(Box::new(self.comparison()?))
                    } else {
                        None
                    };
                    left = Expr::Like {
                        op,
                        expr: Box::new(left),
                        pattern: Box::new(pattern),
                        escape,
                        not,
                    };
                } else if self.eat_kw("BETWEEN") {
                    let low = self.comparison()?;
                    self.expect_kw("AND")?;
                    let high = self.comparison()?;
                    left = Expr::Between {
                        expr: Box::new(left),
                        low: Box::new(low),
                        high: Box::new(high),
                        not,
                    };
                } else if self.is_kw("MATCH") || self.is_kw("REGEXP") {
                    let t = self.next();
                    return Err(SqlError::new(format!(
                        "no such function: {}",
                        t.keyword().unwrap_or_default()
                    )));
                } else {
                    return Ok(left);
                }
            }
        }
    }
    fn in_rest(&mut self, left: Expr, not: bool) -> Result<Expr, SqlError> {
        if self.eat_op("(") {
            if self.is_kw("SELECT") || self.is_kw("VALUES") || self.is_kw("WITH") {
                let select = self.select()?;
                self.expect_op(")")?;
                return Ok(Expr::InSelect {
                    expr: Box::new(left),
                    select: Box::new(select),
                    not,
                });
            }
            let mut list = Vec::new();
            if !self.is_op(")") {
                loop {
                    list.push(self.expr()?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
            }
            self.expect_op(")")?;
            return Ok(Expr::InList {
                expr: Box::new(left),
                list,
                not,
            });
        }
        let table = self.qualified_name()?;
        Ok(Expr::InTable {
            expr: Box::new(left),
            table,
            not,
        })
    }
    fn comparison(&mut self) -> Result<Expr, SqlError> {
        let mut left = self.bitwise()?;
        loop {
            let op = if self.eat_op("<") {
                BinOp::Lt
            } else if self.eat_op("<=") {
                BinOp::Le
            } else if self.eat_op(">") {
                BinOp::Gt
            } else if self.eat_op(">=") {
                BinOp::Ge
            } else {
                return Ok(left);
            };
            let right = self.bitwise()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }
    fn bitwise(&mut self) -> Result<Expr, SqlError> {
        let mut left = self.additive()?;
        loop {
            let op = if self.eat_op("&") {
                BinOp::BitAnd
            } else if self.eat_op("|") {
                BinOp::BitOr
            } else if self.eat_op("<<") {
                BinOp::Shl
            } else if self.eat_op(">>") {
                BinOp::Shr
            } else {
                return Ok(left);
            };
            let right = self.additive()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }
    fn additive(&mut self) -> Result<Expr, SqlError> {
        let mut left = self.multiplicative()?;
        loop {
            let op = if self.eat_op("+") {
                BinOp::Add
            } else if self.eat_op("-") {
                BinOp::Sub
            } else {
                return Ok(left);
            };
            let right = self.multiplicative()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }
    fn multiplicative(&mut self) -> Result<Expr, SqlError> {
        let mut left = self.concat()?;
        loop {
            let op = if self.eat_op("*") {
                BinOp::Mul
            } else if self.eat_op("/") {
                BinOp::Div
            } else if self.eat_op("%") {
                BinOp::Rem
            } else {
                return Ok(left);
            };
            let right = self.concat()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }
    fn concat(&mut self) -> Result<Expr, SqlError> {
        let mut left = self.unary()?;
        loop {
            if self.eat_op("||") {
                let right = self.unary()?;
                left = Expr::Binary(BinOp::Concat, Box::new(left), Box::new(right));
            } else if self.is_op("->") || self.is_op("->>") {
                return Err(SqlError::new("JSON operators are not supported"));
            } else {
                return Ok(left);
            }
        }
    }
    fn unary(&mut self) -> Result<Expr, SqlError> {
        if self.eat_op("-") {
            if matches!(&self.peek().tok, Tok::Int(s) if s == "9223372036854775808") {
                self.next();
                return Ok(Expr::Literal(Value::Integer(i64::MIN)));
            }
            let inner = self.unary()?;
            // Fold a negated literal so -9223372036854775808 stays an integer.
            return Ok(match inner {
                Expr::Literal(Value::Integer(i)) => Expr::Literal(Value::Integer(-i)),
                Expr::Literal(Value::Real(r)) => Expr::Literal(Value::Real(-r)),
                other => Expr::Unary(UnOp::Neg, Box::new(other)),
            });
        }
        if self.eat_op("+") {
            let inner = self.unary()?;
            return Ok(Expr::Unary(UnOp::Plus, Box::new(inner)));
        }
        if self.eat_op("~") {
            let inner = self.unary()?;
            return Ok(Expr::Unary(UnOp::BitNot, Box::new(inner)));
        }
        let mut e = self.primary()?;
        while self.eat_kw("COLLATE") {
            let collation = self.any_name()?;
            e = Expr::Collate {
                expr: Box::new(e),
                collation,
            };
        }
        Ok(e)
    }
    fn primary(&mut self) -> Result<Expr, SqlError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Int(s) => {
                self.next();
                // Too large for an integer: SQLite reads it as a real.
                Ok(Expr::lit(match parse_int(&s) {
                    Some(i) => Value::Integer(i),
                    None if s.starts_with("0x") || s.starts_with("0X") => {
                        return Err(SqlError::new(format!("hex literal too big: {s}")))
                    }
                    None if s == "9223372036854775808" => Value::Real(9.223_372_036_854_776e18),
                    None => Value::Real(s.parse().unwrap_or(0.0)),
                }))
            }
            Tok::Real(s) => {
                self.next();
                Ok(Expr::lit(Value::Real(s.parse().unwrap_or(0.0))))
            }
            Tok::Str(s) => {
                self.next();
                Ok(Expr::lit(Value::Text(s)))
            }
            Tok::Blob(b) => {
                self.next();
                Ok(Expr::lit(Value::Blob(b)))
            }
            Tok::Param(p) => {
                self.next();
                // Every parameter becomes `?N`: a bare `?` takes the next number, a
                // named one keeps the number it got on first sight.
                let n = if p == "?" {
                    self.params.len() + 1
                } else if let Some(n) = p.strip_prefix('?') {
                    let n: usize = n
                        .parse()
                        .ok()
                        .filter(|n| (1..=32766).contains(n))
                        .ok_or_else(|| {
                            SqlError::new("variable number must be between ?1 and ?32766")
                        })?;
                    n
                } else if let Some(i) = self.params.iter().position(|q| *q == p) {
                    i + 1
                } else {
                    self.params.len() + 1
                };
                while self.params.len() < n {
                    self.params.push(String::new());
                }
                if p.starts_with([':', '@', '$']) && self.params[n - 1].is_empty() {
                    self.params[n - 1] = p;
                }
                Ok(Expr::Param(format!("?{n}")))
            }
            Tok::Op("(") => {
                self.next();
                if self.is_kw("SELECT") || self.is_kw("VALUES") || self.is_kw("WITH") {
                    let select = self.select()?;
                    self.expect_op(")")?;
                    return Ok(Expr::Subquery(Box::new(select)));
                }
                let first = self.expr()?;
                if self.eat_op(",") {
                    let mut items = vec![first];
                    loop {
                        items.push(self.expr()?);
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    self.expect_op(")")?;
                    return Ok(Expr::Row(items));
                }
                self.expect_op(")")?;
                Ok(first)
            }
            Tok::Ident { ref name, quoted } => {
                let upper = name.to_ascii_uppercase();
                if !quoted {
                    match upper.as_str() {
                        "NULL" => {
                            self.next();
                            return Ok(Expr::lit(Value::Null));
                        }
                        "TRUE" if !self.is_op_at(1, "(") => {
                            self.next();
                            return Ok(Expr::lit(Value::Integer(1)));
                        }
                        "FALSE" if !self.is_op_at(1, "(") => {
                            self.next();
                            return Ok(Expr::lit(Value::Integer(0)));
                        }
                        "CURRENT_DATE" | "CURRENT_TIME" | "CURRENT_TIMESTAMP" => {
                            self.next();
                            return Ok(Expr::Function {
                                name: upper.to_ascii_lowercase(),
                                args: vec![],
                                distinct: false,
                                star: false,
                                filter: None,
                                window: false,
                            });
                        }
                        "CASE" => return self.case_expr(),
                        "CAST" => {
                            self.next();
                            self.expect_op("(")?;
                            let e = self.expr()?;
                            self.expect_kw("AS")?;
                            let type_name = self.type_name()?;
                            self.expect_op(")")?;
                            return Ok(Expr::Cast {
                                expr: Box::new(e),
                                type_name,
                            });
                        }
                        "EXISTS" => {
                            self.next();
                            self.expect_op("(")?;
                            let s = self.select()?;
                            self.expect_op(")")?;
                            return Ok(Expr::Exists(Box::new(s)));
                        }
                        "NOT" if self.is_kw_at(1, "EXISTS") => {
                            self.next();
                            let e = self.primary()?;
                            return Ok(Expr::Unary(UnOp::Not, Box::new(e)));
                        }
                        "RAISE" if self.is_op_at(1, "(") => {
                            self.next();
                            self.next();
                            let t = self.next();
                            let kind = match t.keyword().as_deref() {
                                Some("IGNORE") => RaiseKind::Ignore,
                                Some("ROLLBACK") => RaiseKind::Rollback,
                                Some("ABORT") => RaiseKind::Abort,
                                Some("FAIL") => RaiseKind::Fail,
                                _ => {
                                    return Err(SqlError::syntax_at(
                                        format!(
                                            "near \"{}\": syntax error",
                                            &self.sql[t.start..t.end]
                                        ),
                                        t.start,
                                    ))
                                }
                            };
                            let message = if kind == RaiseKind::Ignore {
                                None
                            } else {
                                self.expect_op(",")?;
                                Some(self.any_name()?)
                            };
                            self.expect_op(")")?;
                            return Ok(Expr::Raise(kind, message));
                        }
                        _ => {}
                    }
                    if RESERVED.contains(&upper.as_str()) && !self.is_op_at(1, "(") {
                        return Err(self.error_here());
                    }
                }
                let name = name.clone();
                self.next();
                if !quoted && self.is_op("(") {
                    return self.function_call(name);
                }
                if self.eat_op(".") {
                    let col = self.any_name()?;
                    if self.eat_op(".") {
                        // schema.table.column
                        let c = self.any_name()?;
                        return Ok(Expr::Column {
                            table: Some(col),
                            name: c,
                        });
                    }
                    return Ok(Expr::Column {
                        table: Some(name),
                        name: col,
                    });
                }
                // SQLite accepts a double-quoted name that matches no column as a
                // string; the resolver handles that fallback.
                Ok(Expr::Column { table: None, name })
            }
            _ => Err(self.error_here()),
        }
    }
    fn is_op_at(&self, n: usize, op: &str) -> bool {
        matches!(self.peek_at(n).tok, Tok::Op(o) if o == op)
    }
    fn function_call(&mut self, name: String) -> Result<Expr, SqlError> {
        self.expect_op("(")?;
        let mut args = Vec::new();
        let mut distinct = false;
        let mut star = false;
        if self.eat_op("*") {
            star = true;
        } else if !self.is_op(")") {
            distinct = self.eat_kw("DISTINCT");
            if !distinct {
                self.eat_kw("ALL");
            }
            loop {
                args.push(self.expr()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            if self.eat_kw("ORDER") {
                return Err(SqlError::new(
                    "ORDER BY inside an aggregate is not supported",
                ));
            }
        }
        self.expect_op(")")?;
        let filter = if self.eat_kw("FILTER") {
            self.expect_op("(")?;
            self.expect_kw("WHERE")?;
            let e = self.expr()?;
            self.expect_op(")")?;
            Some(Box::new(e))
        } else {
            None
        };
        let window = if self.eat_kw("OVER") {
            // Consume the window specification so the error names the real problem.
            if self.eat_op("(") {
                let mut depth = 1;
                while depth > 0 {
                    match self.next().tok {
                        Tok::Op("(") => depth += 1,
                        Tok::Op(")") => depth -= 1,
                        Tok::Eof => break,
                        _ => {}
                    }
                }
            } else {
                self.name()?;
            }
            true
        } else {
            false
        };
        Ok(Expr::Function {
            name: name.to_ascii_lowercase(),
            args,
            distinct,
            star,
            filter,
            window,
        })
    }
    fn case_expr(&mut self) -> Result<Expr, SqlError> {
        self.expect_kw("CASE")?;
        let operand = if self.is_kw("WHEN") {
            None
        } else {
            Some(Box::new(self.expr()?))
        };
        let mut whens = Vec::new();
        while self.eat_kw("WHEN") {
            let w = self.expr()?;
            self.expect_kw("THEN")?;
            let t = self.expr()?;
            whens.push((w, t));
        }
        if whens.is_empty() {
            return Err(self.error_here());
        }
        let otherwise = if self.eat_kw("ELSE") {
            Some(Box::new(self.expr()?))
        } else {
            None
        };
        self.expect_kw("END")?;
        Ok(Expr::Case {
            operand,
            whens,
            otherwise,
        })
    }
}
fn parse_int(s: &str) -> Option<i64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok().map(|v| v as i64);
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precedence_follows_sqlite() {
        let e = parse_expr("1 + 2 * 3 = 7 AND NOT 0 OR 0").unwrap();
        let Expr::Binary(BinOp::Or, left, _) = e else {
            panic!("OR binds loosest")
        };
        assert!(matches!(*left, Expr::Binary(BinOp::And, _, _)));
        assert!(matches!(
            parse_expr("'a' || 'b' * 2").unwrap(),
            Expr::Binary(BinOp::Mul, _, _)
        ));
        assert!(matches!(
            parse_expr("x NOT BETWEEN 1 AND 2").unwrap(),
            Expr::Between { not: true, .. }
        ));
        assert!(matches!(
            parse_expr("-9223372036854775808").unwrap(),
            Expr::Literal(Value::Integer(i64::MIN))
        ));
    }
    #[test]
    fn statements_keep_their_source_text() {
        let s =
            parse("create table t(a integer primary key, b text not null default 'x');").unwrap();
        let Stmt::CreateTable(t) = &s[0] else {
            panic!()
        };
        assert_eq!(
            t.sql,
            "CREATE TABLE t(a integer primary key, b text not null default 'x')"
        );
        assert_eq!(t.columns[1].default.as_deref(), Some("'x'"));
        assert_eq!(t.columns[0].type_name, "integer");
    }
    #[test]
    fn syntax_errors_name_the_token() {
        let e = parse("SELEC 1").unwrap_err();
        assert_eq!(e.message, "near \"SELEC\": syntax error");
        assert_eq!(e.offset, Some(0));
        assert_eq!(parse("SELECT 1 +").unwrap_err().message, "incomplete input");
    }
}
