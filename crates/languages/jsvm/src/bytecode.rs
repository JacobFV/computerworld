//! Bytecode instruction set and compiled function objects.

use crate::ast::{FuncKind, Pos};
use crate::value::{JsStr, Obj, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// `repr(u8)`: the first byte is the variant, which the profiler's histogram reads.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Op {
    // constants
    Undef,
    Null,
    True,
    False,
    Num(f64),
    Const(u32),
    // stack
    Pop,
    Dup,
    Dup2,
    Swap,
    /// a b c -> c a b
    Rot3,
    /// a b c d -> d a b c
    Rot4,
    // bindings
    Load(u32),
    Store(u32),
    Init(u32),
    LoadFree(u32),
    StoreFree(u32),
    InitFree(u32),
    /// Resets a lexical binding to uninitialised (fresh cell if captured).
    DeclLet(u32),
    /// Per-iteration copy of a captured loop binding.
    CopyCell(u32),
    /// TypeError: Assignment to constant variable.
    ConstAssign,
    LoadGlobal(u32),
    StoreGlobal(u32),
    TypeofGlobal(u32),
    // properties
    GetProp(u32),
    GetPropKeep(u32),
    SetProp(u32),
    GetElem,
    GetElemKeep,
    SetElem,
    DeleteProp(u32),
    DeleteElem,
    /// [this home] -> value
    SuperGet(u32),
    /// [this home key] -> value
    SuperGetElem,
    /// [this home] -> [this fn]
    SuperGetKeep(u32),
    SuperGetElemKeep,
    /// [this home value] -> value
    SuperSet(u32),
    /// [this home key value] -> value
    SuperSetElem,
    /// [obj key] -> value
    GetPrivate,
    /// [obj key] -> [obj value]
    GetPrivateKeep,
    /// [obj key value] -> value
    SetPrivate,
    /// [obj key value] -> obj (field definition)
    DefinePrivate,
    /// [key obj] -> bool
    HasPrivate,
    // calls: `name` is a constant with the callee text for error messages
    Call(u32, u32),
    CallMethod(u32, u32),
    CallSpread(u32),
    CallMethodSpread(u32),
    New(u32, u32),
    NewSpread(u32),
    /// [fn newtarget args...] -> this
    SuperCall(u32),
    SuperCallSpread,
    Return,
    /// Return through enclosing finally blocks / iterator closers.
    ReturnUnwind,
    // operators
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
    /// `with` lookup: obj -> bool, whether the binding object has the name
    /// (constant) and its `Symbol.unscopables` does not block it.
    WithHas(u32),
    /// value -> object (`ToObject`, the `with` statement's binding object).
    ToObject,
    InstanceOf,
    Neg,
    Plus,
    Not,
    BitNot,
    Typeof,
    ToNumeric,
    Inc,
    Dec,
    ToStr,
    ToPropertyKey,
    Concat(u32),
    // jumps
    Jump(u32),
    JumpIfFalse(u32),
    JumpIfTrue(u32),
    JumpIfFalseKeep(u32),
    JumpIfTrueKeep(u32),
    JumpIfNotNullishKeep(u32),
    JumpIfNotUndefKeep(u32),
    /// Optional chain: if TOS is nullish, pop `n` values and jump.
    OptCheck(u32, u32),
    // exceptions
    EnterTry(u32, bool),
    ExitTry,
    Throw,
    Rethrow,
    /// Finally epilogue: (kind slot, value slot).
    EndFinally(u32, u32),
    PushPc(u32),
    /// Throws a TypeError / ReferenceError / SyntaxError built from a
    /// constant message: (kind 0=TypeError 1=ReferenceError 2=SyntaxError, msg).
    ThrowError(u32, u32),
    // iteration
    /// [obj] -> [iter next]; operand: callee-text constant (u32::MAX: native wording).
    GetIter(u32),
    GetAsyncIter(u32),
    /// [iter next] -> [iter next value] or jump when done.
    IterNext(u32),
    /// Destructuring step: [iter next] -> [iter next value]; marks done.
    IterStep,
    /// [iter next] -> [iter next array]
    IterRest,
    /// [iter next] -> []  (calls `return` unless exhausted)
    IterClose,
    /// Handler epilogue of for-of: [iter next value kind] -> re-raise.
    IterCloseCompletion,
    /// [obj] -> [forin]
    ForInPrep,
    /// [forin] -> [forin key] or jump when done.
    ForInNext(u32),
    /// Async for-of: [iter next] -> [iter next result] awaiting.
    AsyncIterNext,
    /// [result] -> [value] or jump when done.
    IterResult(u32),
    // objects
    NewObject,
    NewArray(u32),
    ArrayPush,
    ArrayHole,
    /// [arr iterable] -> arr (operand: expression text for errors, or u32::MAX)
    ArraySpread(u32),
    /// [obj value] -> obj ; enumerable data property
    DefineField(u32),
    /// [obj key value] -> obj
    DefineElem,
    /// [obj fn] -> obj : (name, kind 0=method 1=get 2=set, enumerable)
    DefineMethod(u32, u8, bool),
    /// [obj key fn] -> obj
    DefineMethodElem(u8, bool),
    /// [obj src] -> obj
    CopyData,
    /// [obj src k1..kn] -> obj
    CopyDataExcluding(u32),
    /// [obj proto] -> obj  (`__proto__: v` in literals)
    SetProtoLit,
    /// [value] -> [value] requiring object-coercible (destructuring):
    /// (name const of first property or u32::MAX)
    RequireCoercible(u32),
    // functions & classes
    Closure(u32),
    /// Fresh private name symbol (description constant).
    NewPrivateName(u32),
    /// New RegExp object (pattern constant, flags constant).
    RegExp(u32, u32),
    /// [ctor proto fn] -> [ctor proto] : (name, kind, is_static)
    ClassMethod(u32, u8, bool),
    /// [ctor proto key fn] -> [ctor proto]
    ClassMethodElem(u8, bool),
    /// [ns fn] -> [ns] : live export getter
    ExportGetter(u32),
    /// [fn key] -> [fn key] naming an anonymous function after a computed key.
    SetFnNameElem(u8),
    /// [super?] -> [ctor proto] : (code index, has_super)
    Class(u32, bool),
    /// [ctor proto fn] -> [ctor proto]
    SetFieldInit,
    /// [fn this] -> [] run instance field initialisers
    RunFields,
    /// [this] -> [this]; binds `this` after super() (slot or free index)
    BindThis(u32, bool),
    RestParam(u32),
    Arg(u32),
    // templates
    TemplateObj(u32),
    // generators & async
    Yield,
    /// [iter next received] -> value (jump target when done)
    YieldStar(u32),
    Await,
    // misc
    /// Pops the value of a top-level expression statement (eval / -p).
    SetCompletion,
    /// Pushes (and clears) the completion value.
    TakeCompletion,
    Debugger,
    Nop,
    ImportMeta,
    DynImport,
}

#[derive(Clone, Copy, Debug)]
pub enum Capture {
    Local(u32),
    Free(u32),
}

pub struct Code {
    pub name: JsStr,
    pub ops: Vec<Op>,
    pub pos: Vec<Pos>,
    pub consts: Vec<Value>,
    pub codes: Vec<Rc<Code>>,
    pub nlocals: u32,
    pub local_names: Vec<JsStr>,
    pub is_cell: Vec<bool>,
    pub captures: Vec<Capture>,
    pub free_names: Vec<JsStr>,
    /// Number of leading simple identifier parameters copied straight into
    /// slots 0..n.
    pub simple_params: Option<u32>,
    /// `length` of the function.
    pub length: u32,
    pub this_slot: Option<u32>,
    pub newtarget_slot: Option<u32>,
    pub home_slot: Option<u32>,
    pub fn_slot: Option<u32>,
    pub args_slot: Option<u32>,
    pub kind: FuncKind,
    pub is_async: bool,
    pub is_generator: bool,
    pub strict: bool,
    pub file: Rc<str>,
    /// Source text of the function (for `toString`).
    pub source: Rc<str>,
    pub template_cache: RefCell<Vec<Option<Obj>>>,
    /// Tagged template sites: (cooked, raw).
    pub templates: Vec<(Vec<Option<JsStr>>, Vec<JsStr>)>,
    /// Module top level (frame named `Object.<anonymous>`).
    pub is_top: bool,
    /// Uses `arguments`, so the frame keeps its argument list.
    pub needs_args: bool,
    /// Whether this body has already been charged its compile cost: V8 compiles
    /// a function the first time it runs, and the simulation charges for it
    /// then (see `Vm::charge_compile`).
    pub compiled: std::cell::Cell<bool>,
    /// Bytes of source that belong to this body alone (nested functions carry
    /// their own), which is what that charge is proportional to.
    pub own_bytes: u32,
}
