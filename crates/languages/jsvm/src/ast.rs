//! Syntax tree produced by the parser.

use std::rc::Rc;

pub type Name = Rc<str>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Plus,
    Not,
    BitNot,
    Typeof,
    Void,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Exp,
    Shl,
    Shr,
    UShr,
    BitAnd,
    BitOr,
    BitXor,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Gt,
    Le,
    Ge,
    In,
    InstanceOf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogOp {
    And,
    Or,
    Nullish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Op(BinOp),
    Logical(LogOp),
}

#[derive(Debug)]
pub enum ExprKind {
    Num(f64),
    BigInt(Rc<str>),
    Str(Rc<str>),
    Bool(bool),
    Null,
    Template {
        cooked: Vec<Rc<str>>,
        exprs: Vec<Expr>,
    },
    Tagged {
        tag: Box<Expr>,
        cooked: Vec<Option<Rc<str>>>,
        raw: Vec<Rc<str>>,
        exprs: Vec<Expr>,
    },
    Regex {
        pattern: Rc<str>,
        flags: Rc<str>,
    },
    Ident(Name),
    This,
    Array(Vec<ArrElem>),
    Object(Vec<Prop>),
    Function(Rc<Func>),
    Class(Rc<Class>),
    Unary(UnOp, Box<Expr>),
    Update {
        inc: bool,
        prefix: bool,
        target: Box<Expr>,
    },
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Logical(LogOp, Box<Expr>, Box<Expr>),
    Assign {
        op: AssignOp,
        target: Box<Pattern>,
        value: Box<Expr>,
    },
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<ArrElem>,
        optional: bool,
    },
    New {
        callee: Box<Expr>,
        args: Vec<ArrElem>,
    },
    Member {
        obj: Box<Expr>,
        prop: MemberProp,
        optional: bool,
    },
    /// Delimits an optional chain: a short-circuit inside jumps to its end.
    OptChain(Box<Expr>),
    Seq(Vec<Expr>),
    Yield {
        arg: Option<Box<Expr>>,
        delegate: bool,
    },
    Await(Box<Expr>),
    SuperMember(MemberProp),
    SuperCall(Vec<ArrElem>),
    NewTarget,
    ImportMeta,
    /// Dynamic `import(x)`.
    Import(Box<Expr>),
    PrivateIn(Name, Box<Expr>),
    /// `{a = 1}` shorthand only valid as a destructuring pattern.
    CoverInit(Name, Box<Expr>),
}

#[derive(Debug)]
pub enum MemberProp {
    Name(Name, Pos),
    Computed(Box<Expr>),
    Private(Name, Pos),
}

#[derive(Debug)]
pub enum ArrElem {
    Expr(Expr),
    Spread(Expr),
    Hole,
}

#[derive(Debug)]
pub enum PropKey {
    Lit(Name),
    Computed(Box<Expr>),
    Private(Name),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    Method,
    Get,
    Set,
}

#[derive(Debug)]
pub enum Prop {
    KeyValue(PropKey, Expr),
    Shorthand(Name, Pos),
    Method {
        key: PropKey,
        func: Rc<Func>,
        kind: MethodKind,
    },
    Spread(Expr),
}

#[derive(Debug)]
pub enum Pattern {
    Ident(Name, Pos),
    /// A member expression (assignment targets only).
    Expr(Expr),
    Object {
        props: Vec<PatProp>,
        rest: Option<Box<Pattern>>,
    },
    Array {
        elems: Vec<Option<PatElem>>,
        rest: Option<Box<Pattern>>,
    },
}

#[derive(Debug)]
pub struct PatProp {
    pub key: PropKey,
    pub value: PatElem,
}

#[derive(Debug)]
pub struct PatElem {
    pub target: Pattern,
    pub default: Option<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuncKind {
    Normal,
    Arrow,
    Method,
    Getter,
    Setter,
    BaseConstructor,
    DerivedConstructor,
    /// Class field initialisers and static blocks.
    ClassInit,
}

#[derive(Debug)]
pub enum FuncBody {
    Block(Vec<Stmt>),
    Expr(Box<Expr>),
}

#[derive(Debug)]
pub struct Func {
    pub name: Option<Name>,
    pub params: Vec<PatElem>,
    pub rest: Option<Pattern>,
    pub body: FuncBody,
    pub kind: FuncKind,
    pub is_async: bool,
    pub is_generator: bool,
    pub strict: bool,
    pub pos: Pos,
    /// Char offsets of the function's source text (for `toString`).
    pub src_start: usize,
    pub src_end: usize,
    /// Class fields to initialise (constructors only).
    pub fields: Option<Rc<Func>>,
}

#[derive(Debug)]
pub enum ClassMember {
    Method {
        key: PropKey,
        func: Rc<Func>,
        kind: MethodKind,
        is_static: bool,
    },
    Field {
        key: PropKey,
        value: Option<Expr>,
        is_static: bool,
        pos: Pos,
    },
    StaticBlock(Vec<Stmt>),
}

#[derive(Debug)]
pub struct Class {
    pub name: Option<Name>,
    pub extends: Option<Box<Expr>>,
    pub ctor: Option<Rc<Func>>,
    pub members: Vec<ClassMember>,
    pub pos: Pos,
    pub src_start: usize,
    pub src_end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Var,
    Let,
    Const,
}

#[derive(Debug)]
pub struct Declarator {
    pub target: Pattern,
    pub init: Option<Expr>,
    /// Start of the binding pattern.
    pub pos: Pos,
}

#[derive(Debug)]
pub enum ForInit {
    Var(VarKind, Vec<Declarator>),
    Expr(Expr),
}

#[derive(Debug)]
pub enum ForLeft {
    Var(VarKind, Pattern),
    Pattern(Pattern),
}

#[derive(Debug)]
pub struct Case {
    pub test: Option<Expr>,
    pub body: Vec<Stmt>,
    pub pos: Pos,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub pos: Pos,
}

#[derive(Debug)]
pub enum ImportName {
    Default(Name),
    Namespace(Name),
    Named(Name, Name),
}

#[derive(Debug)]
pub enum ExportKind {
    /// `export default expr`
    Default(Expr),
    /// `export const/let/var/function/class`
    Decl(Box<Stmt>),
    /// `export default function f() {}` / `export default class C {}`
    DefaultDecl(Box<Stmt>),
    /// `export {a as b}` (optionally `from "m"`)
    Names(Vec<(Name, Name)>, Option<Rc<str>>),
    /// `export * from "m"` / `export * as ns from "m"`
    All(Option<Name>, Rc<str>),
}

#[derive(Debug)]
pub enum StmtKind {
    Expr(Expr),
    Var(VarKind, Vec<Declarator>),
    Func(Rc<Func>),
    Class(Rc<Class>),
    Return(Option<Expr>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    For {
        init: Option<ForInit>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    ForIn {
        left: ForLeft,
        right: Expr,
        body: Box<Stmt>,
    },
    ForOf {
        left: ForLeft,
        right: Expr,
        body: Box<Stmt>,
        is_await: bool,
    },
    While(Expr, Box<Stmt>),
    /// `with (object) body` (sloppy mode only).
    With(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    Break(Option<Name>),
    Continue(Option<Name>),
    Throw(Expr),
    Try {
        block: Vec<Stmt>,
        param: Option<Pattern>,
        handler: Option<Vec<Stmt>>,
        finalizer: Option<Vec<Stmt>>,
    },
    Switch(Expr, Vec<Case>),
    Block(Vec<Stmt>),
    Labeled(Name, Box<Stmt>),
    Import(Vec<ImportName>, Rc<str>),
    Export(ExportKind),
    Empty,
    Debugger,
}

#[derive(Debug)]
pub struct Program {
    pub body: Vec<Stmt>,
    pub is_module: bool,
    pub strict: bool,
}
