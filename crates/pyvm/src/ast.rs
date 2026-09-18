//! Abstract syntax tree. Every node carries the line (and column) it starts on so
//! tracebacks and SyntaxErrors can point at source.
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: u32,
    pub col: u32,
    pub end_col: u32,
}

#[derive(Clone, Debug)]
pub enum Const {
    None,
    True,
    False,
    Ellipsis,
    Int(String, u32),
    Float(f64),
    Imag(f64),
    Str(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    MatMul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    LShift,
    RShift,
    BitOr,
    BitXor,
    BitAnd,
}
impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::MatMul => "@",
            BinOp::Div => "/",
            BinOp::FloorDiv => "//",
            BinOp::Mod => "%",
            BinOp::Pow => "**",
            BinOp::LShift => "<<",
            BinOp::RShift => ">>",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::BitAnd => "&",
        }
    }
    pub fn dunder(self) -> (&'static str, &'static str, &'static str) {
        match self {
            BinOp::Add => ("__add__", "__radd__", "__iadd__"),
            BinOp::Sub => ("__sub__", "__rsub__", "__isub__"),
            BinOp::Mul => ("__mul__", "__rmul__", "__imul__"),
            BinOp::MatMul => ("__matmul__", "__rmatmul__", "__imatmul__"),
            BinOp::Div => ("__truediv__", "__rtruediv__", "__itruediv__"),
            BinOp::FloorDiv => ("__floordiv__", "__rfloordiv__", "__ifloordiv__"),
            BinOp::Mod => ("__mod__", "__rmod__", "__imod__"),
            BinOp::Pow => ("__pow__", "__rpow__", "__ipow__"),
            BinOp::LShift => ("__lshift__", "__rlshift__", "__ilshift__"),
            BinOp::RShift => ("__rshift__", "__rrshift__", "__irshift__"),
            BinOp::BitOr => ("__or__", "__ror__", "__ior__"),
            BinOp::BitXor => ("__xor__", "__rxor__", "__ixor__"),
            BinOp::BitAnd => ("__and__", "__rand__", "__iand__"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Pos,
    Invert,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    Is,
    IsNot,
    In,
    NotIn,
}
impl CmpOp {
    pub fn symbol(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::NotEq => "!=",
            CmpOp::Lt => "<",
            CmpOp::LtE => "<=",
            CmpOp::Gt => ">",
            CmpOp::GtE => ">=",
            CmpOp::Is => "is",
            CmpOp::IsNot => "is not",
            CmpOp::In => "in",
            CmpOp::NotIn => "not in",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Comprehension {
    pub target: Expr,
    pub iter: Expr,
    pub ifs: Vec<Expr>,
    pub is_async: bool,
}

#[derive(Clone, Debug)]
pub enum FStrPart {
    Lit(String),
    Expr {
        value: Box<Expr>,
        conversion: Option<char>,
        spec: Option<Vec<FStrPart>>,
    },
}

#[derive(Clone, Debug)]
pub struct Keyword {
    /// `None` is `**mapping`.
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Name(String),
    Const(Const),
    JoinedStr(Vec<FStrPart>),
    List(Vec<Expr>),
    Tuple(Vec<Expr>),
    Set(Vec<Expr>),
    /// `None` key means `**expr`.
    Dict(Vec<(Option<Expr>, Expr)>),
    Starred(Box<Expr>),
    Attribute(Box<Expr>, String),
    Subscript(Box<Expr>, Box<Expr>),
    Slice(Option<Box<Expr>>, Option<Box<Expr>>, Option<Box<Expr>>),
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        keywords: Vec<Keyword>,
    },
    BinOp(BinOp, Box<Expr>, Box<Expr>),
    UnaryOp(UnaryOp, Box<Expr>),
    /// `and` is true, `or` is false.
    BoolOp(bool, Vec<Expr>),
    Compare(Box<Expr>, Vec<(CmpOp, Expr)>),
    IfExp(Box<Expr>, Box<Expr>, Box<Expr>),
    Lambda(Rc<FuncDef>),
    ListComp(Box<Expr>, Vec<Comprehension>),
    SetComp(Box<Expr>, Vec<Comprehension>),
    GenExp(Box<Expr>, Vec<Comprehension>),
    DictComp(Box<Expr>, Box<Expr>, Vec<Comprehension>),
    Yield(Option<Box<Expr>>),
    YieldFrom(Box<Expr>),
    Await(Box<Expr>),
    NamedExpr(Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub name: String,
    pub annotation: Option<Expr>,
    pub line: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Params {
    pub posonly: Vec<Arg>,
    pub args: Vec<Arg>,
    pub vararg: Option<Arg>,
    pub kwonly: Vec<Arg>,
    pub kwarg: Option<Arg>,
    /// Defaults for the last N of posonly+args.
    pub defaults: Vec<Expr>,
    /// One per kwonly arg.
    pub kw_defaults: Vec<Option<Expr>>,
}

#[derive(Clone, Debug)]
pub struct FuncDef {
    pub name: String,
    pub params: Params,
    pub body: Vec<Stmt>,
    pub decorators: Vec<Expr>,
    pub returns: Option<Expr>,
    pub is_async: bool,
    pub is_lambda: bool,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct ClassDef {
    pub name: String,
    pub bases: Vec<Expr>,
    pub keywords: Vec<Keyword>,
    pub body: Vec<Stmt>,
    pub decorators: Vec<Expr>,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Handler {
    pub typ: Option<Expr>,
    pub name: Option<String>,
    pub body: Vec<Stmt>,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Alias {
    pub name: String,
    pub asname: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Pattern {
    /// `_` or a capture name.
    Capture(Option<String>),
    Value(Expr),
    Singleton(Const),
    Sequence(Vec<Pattern>),
    /// `*name` inside a sequence (None for `*_`).
    Star(Option<String>),
    Mapping {
        keys: Vec<Expr>,
        patterns: Vec<Pattern>,
        rest: Option<String>,
    },
    Class {
        cls: Expr,
        args: Vec<Pattern>,
        kwargs: Vec<(String, Pattern)>,
    },
    Or(Vec<Pattern>),
    As(Box<Pattern>, String),
}

#[derive(Clone, Debug)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Expr(Expr),
    Assign(Vec<Expr>, Expr),
    AugAssign(Expr, BinOp, Expr),
    AnnAssign(Expr, Expr, Option<Expr>, bool),
    Return(Option<Expr>),
    Pass,
    Break,
    Continue,
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>, Vec<Stmt>),
    For(Expr, Expr, Vec<Stmt>, Vec<Stmt>, bool),
    Try(Vec<Stmt>, Vec<Handler>, Vec<Stmt>, Vec<Stmt>),
    Raise(Option<Expr>, Option<Expr>),
    With(Vec<(Expr, Option<Expr>)>, Vec<Stmt>, bool),
    FunctionDef(Rc<FuncDef>),
    ClassDef(Rc<ClassDef>),
    Import(Vec<Alias>),
    ImportFrom(Option<String>, Vec<Alias>, u32),
    Global(Vec<String>),
    Nonlocal(Vec<String>),
    Assert(Expr, Option<Expr>),
    Delete(Vec<Expr>),
    Match(Expr, Vec<MatchCase>),
}
