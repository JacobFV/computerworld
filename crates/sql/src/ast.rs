//! Syntax tree for the supported SQLite dialect.
use crate::value::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Select(Box<Select>),
    Insert(Box<Insert>),
    Update(Box<Update>),
    Delete(Box<Delete>),
    CreateTable(Box<CreateTable>),
    CreateIndex(Box<CreateIndex>),
    CreateView(Box<CreateView>),
    Drop {
        kind: ObjectKind,
        name: String,
        if_exists: bool,
    },
    AlterTable(Box<AlterTable>),
    Begin,
    Commit,
    Rollback {
        savepoint: Option<String>,
    },
    Savepoint(String),
    Release(String),
    Pragma {
        name: String,
        arg: Option<PragmaArg>,
    },
    Explain {
        query_plan: bool,
        stmt: Box<Stmt>,
    },
    Vacuum,
    Analyze,
    Reindex,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Table,
    Index,
    View,
    Trigger,
}
#[derive(Clone, Debug, PartialEq)]
pub enum PragmaArg {
    /// `PRAGMA name = value`
    Set(Value),
    /// `PRAGMA name(arg)`
    Call(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub with: Option<With>,
    pub first: SelectCore,
    pub compounds: Vec<(CompoundOp, SelectCore)>,
    pub order_by: Vec<OrderTerm>,
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct With {
    pub recursive: bool,
    pub ctes: Vec<Cte>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Cte {
    pub name: String,
    pub columns: Vec<String>,
    pub select: Box<Select>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompoundOp {
    Union,
    UnionAll,
    Intersect,
    Except,
}
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum SelectCore {
    Select {
        distinct: bool,
        columns: Vec<ResultCol>,
        from: Option<FromItem>,
        filter: Option<Expr>,
        group_by: Vec<Expr>,
        having: Option<Expr>,
    },
    Values(Vec<Vec<Expr>>),
}
#[derive(Clone, Debug, PartialEq)]
pub enum ResultCol {
    Star,
    TableStar(String),
    Expr {
        expr: Expr,
        alias: Option<String>,
        /// Source text, which names an unaliased column the way SQLite does.
        text: String,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub enum FromItem {
    Table {
        name: String,
        alias: Option<String>,
    },
    Subquery {
        select: Box<Select>,
        alias: Option<String>,
    },
    Join {
        left: Box<FromItem>,
        right: Box<FromItem>,
        kind: JoinKind,
        constraint: JoinConstraint,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}
#[derive(Clone, Debug, PartialEq)]
pub enum JoinConstraint {
    None,
    On(Expr),
    Using(Vec<String>),
    Natural,
}
#[derive(Clone, Debug, PartialEq)]
pub struct OrderTerm {
    pub expr: Expr,
    pub desc: bool,
    /// `Some(true)` for NULLS FIRST, `Some(false)` for NULLS LAST.
    pub nulls_first: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    Abort,
    Fail,
    Ignore,
    Replace,
    Rollback,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    pub with: Option<With>,
    pub conflict: Conflict,
    pub table: String,
    pub columns: Vec<String>,
    pub source: InsertSource,
    pub upsert: Option<Upsert>,
    pub returning: Vec<ResultCol>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum InsertSource {
    Values(Vec<Vec<Expr>>),
    Select(Box<Select>),
    Default,
}
/// `SET (a, b) = expr`: the target columns and the value.
pub type Assignment = (Vec<String>, Expr);
#[derive(Clone, Debug, PartialEq)]
pub struct Upsert {
    pub target: Vec<String>,
    /// `None` is DO NOTHING.
    pub update: Option<(Vec<Assignment>, Option<Expr>)>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub with: Option<With>,
    pub conflict: Conflict,
    pub table: String,
    pub alias: Option<String>,
    pub sets: Vec<(Vec<String>, Expr)>,
    pub from: Option<FromItem>,
    pub filter: Option<Expr>,
    pub returning: Vec<ResultCol>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Delete {
    pub with: Option<With>,
    pub table: String,
    pub alias: Option<String>,
    pub filter: Option<Expr>,
    pub returning: Vec<ResultCol>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateTable {
    pub name: String,
    pub if_not_exists: bool,
    pub temporary: bool,
    pub columns: Vec<ColumnDef>,
    pub constraints: Vec<TableConstraint>,
    pub as_select: Option<Box<Select>>,
    pub without_rowid: bool,
    pub strict: bool,
    /// The statement's own text, stored in the schema as SQLite does.
    pub sql: String,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ColumnDef {
    pub name: String,
    pub type_name: String,
    pub primary_key: Option<PrimaryKeySpec>,
    pub not_null: bool,
    pub unique: bool,
    /// Source text of the DEFAULT expression.
    pub default: Option<String>,
    pub checks: Vec<String>,
    pub collation: Option<String>,
    pub references: Option<ForeignKeySpec>,
    /// `GENERATED ALWAYS AS (expr)` source text.
    pub generated: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PrimaryKeySpec {
    pub desc: bool,
    pub autoincrement: bool,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ForeignKeySpec {
    pub table: String,
    pub columns: Vec<String>,
    pub on_delete: FkAction,
    pub on_update: FkAction,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FkAction {
    #[default]
    NoAction,
    Restrict,
    SetNull,
    SetDefault,
    Cascade,
}
#[derive(Clone, Debug, PartialEq)]
pub enum TableConstraint {
    PrimaryKey(Vec<IndexedColumn>, bool),
    Unique(Vec<IndexedColumn>),
    Check(String),
    ForeignKey(Vec<String>, ForeignKeySpec),
}
#[derive(Clone, Debug, PartialEq)]
pub struct IndexedColumn {
    pub name: String,
    pub collation: Option<String>,
    pub desc: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CreateIndex {
    pub name: String,
    pub table: String,
    pub unique: bool,
    pub if_not_exists: bool,
    pub columns: Vec<IndexedColumn>,
    pub filter: Option<Expr>,
    pub sql: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CreateView {
    pub name: String,
    pub if_not_exists: bool,
    pub columns: Vec<String>,
    pub select: Box<Select>,
    pub sql: String,
}
#[derive(Clone, Debug, PartialEq)]
pub enum AlterTable {
    Rename {
        table: String,
        to: String,
    },
    RenameColumn {
        table: String,
        from: String,
        to: String,
    },
    AddColumn {
        table: String,
        column: ColumnDef,
        text: String,
    },
    DropColumn {
        table: String,
        column: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Plus,
    Not,
    BitNot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Concat,
    BitAnd,
    BitOr,
    Shl,
    Shr,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LikeOp {
    Like,
    Glob,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Literal(Value),
    Column {
        table: Option<String>,
        name: String,
    },
    Param(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `IS`, `IS NOT`, `IS [NOT] DISTINCT FROM`: `not` is true for the negated forms.
    Is {
        left: Box<Expr>,
        right: Box<Expr>,
        not: bool,
    },
    Like {
        op: LikeOp,
        expr: Box<Expr>,
        pattern: Box<Expr>,
        escape: Option<Box<Expr>>,
        not: bool,
    },
    Between {
        expr: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
        not: bool,
    },
    InList {
        expr: Box<Expr>,
        list: Vec<Expr>,
        not: bool,
    },
    InSelect {
        expr: Box<Expr>,
        select: Box<Select>,
        not: bool,
    },
    InTable {
        expr: Box<Expr>,
        table: String,
        not: bool,
    },
    Exists(Box<Select>),
    Subquery(Box<Select>),
    Case {
        operand: Option<Box<Expr>>,
        whens: Vec<(Expr, Expr)>,
        otherwise: Option<Box<Expr>>,
    },
    Cast {
        expr: Box<Expr>,
        type_name: String,
    },
    Collate {
        expr: Box<Expr>,
        collation: String,
    },
    Function {
        name: String,
        args: Vec<Expr>,
        distinct: bool,
        star: bool,
        filter: Option<Box<Expr>>,
        /// `OVER (...)` was written: window functions are refused at execution.
        window: bool,
    },
    /// `(a, b)` row value, only valid in comparisons.
    Row(Vec<Expr>),
}
impl Expr {
    pub fn lit(v: Value) -> Self {
        Self::Literal(v)
    }
    /// Visit this node and every node below it, not descending into subqueries.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Expr)) {
        f(self);
        match self {
            Self::Unary(_, a) | Self::Cast { expr: a, .. } | Self::Collate { expr: a, .. } => {
                a.walk(f)
            }
            Self::Binary(_, a, b)
            | Self::Is {
                left: a, right: b, ..
            } => {
                a.walk(f);
                b.walk(f);
            }
            Self::Like {
                expr,
                pattern,
                escape,
                ..
            } => {
                expr.walk(f);
                pattern.walk(f);
                if let Some(e) = escape {
                    e.walk(f);
                }
            }
            Self::Between {
                expr, low, high, ..
            } => {
                expr.walk(f);
                low.walk(f);
                high.walk(f);
            }
            Self::InList { expr, list, .. } => {
                expr.walk(f);
                for e in list {
                    e.walk(f);
                }
            }
            Self::InSelect { expr, .. } | Self::InTable { expr, .. } => expr.walk(f),
            Self::Case {
                operand,
                whens,
                otherwise,
            } => {
                if let Some(o) = operand {
                    o.walk(f);
                }
                for (w, t) in whens {
                    w.walk(f);
                    t.walk(f);
                }
                if let Some(o) = otherwise {
                    o.walk(f);
                }
            }
            Self::Function { args, filter, .. } => {
                for a in args {
                    a.walk(f);
                }
                if let Some(e) = filter {
                    e.walk(f);
                }
            }
            Self::Row(items) => {
                for e in items {
                    e.walk(f);
                }
            }
            Self::Literal(_)
            | Self::Column { .. }
            | Self::Param(_)
            | Self::Exists(_)
            | Self::Subquery(_) => {}
        }
    }
}
