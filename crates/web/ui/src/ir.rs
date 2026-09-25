//! The UI IR: what `cw-tsx` compiles a React-syntax module to and what the runtime
//! mounts. It is plain data (serde), versioned by [`IR_VERSION`].
//!
//! Shape. A [`Module`] is a table of globals (module-level `const`s, functions,
//! components, contexts, in source order), a table of [`Function`]s (every function
//! body in the module, closures included), a table of host-element [`Template`]s and
//! the [`Root`] the module mounts. Functions address variables by slot, never by name:
//! [`Expr::Local`] is a slot of the running frame, [`Expr::Capture`] a value the
//! closure copied when it was created (captured variables are never reassigned, which
//! the compiler checks, so copying is exact), [`Expr::Global`] a module slot.
//!
//! JSX. A run of host elements (`<div><span>{x}</span></div>`) is one [`Template`]: the
//! static skeleton with numbered holes for dynamic attributes and dynamic children. An
//! [`Expr::Element`] evaluates the hole expressions and yields an element value; the
//! runtime instantiates the template once and afterwards patches only the holes whose
//! values changed. A component, a fragment or a context provider in JSX is its own
//! element expression, nested in a template as a child hole.
//!
//! Types. Every function carries the static types of its parameters and locals and
//! every template hole its type, which the runtime ignores but a later Rust code
//! generator needs: with them each slot can become a typed Rust variable.
//!
//! Dependencies. A template hole lists the frame slots it reads
//! ([`Hole::deps`]). When a component re-renders and none of those slots changed
//! (by identity), the runtime skips evaluating the hole at all.

use serde::{Deserialize, Serialize};

/// Bumped whenever the IR changes incompatibly; the runtime refuses other versions
/// (the page then falls back to the React build).
pub const IR_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub version: u32,
    /// The source file name, for diagnostics.
    pub source: String,
    pub globals: Vec<Global>,
    pub functions: Vec<Function>,
    pub templates: Vec<Template>,
    pub root: Option<Root>,
    /// Whether any code mutates a value that outlives the render that created it
    /// (`state.push(x)`, `props.items.sort()`, `obj.field = v` on a non-fresh
    /// object). When set, the runtime never skips a hole by identity of its inputs.
    pub mutates_shared: bool,
}

/// A module-level binding, initialised in declaration order when the module loads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Global {
    pub name: String,
    pub init: GlobalInit,
    pub ty: Ty,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GlobalInit {
    /// A function declaration (hoisted: initialised before any expression).
    Function(u32),
    /// `const X = expr` / `let X = expr` (a module statement's destructuring is split
    /// into one global per name, each with its own path into the value).
    Expr(Expr),
    /// `createContext(default)`.
    Context(Expr),
    /// Declared but not yet assigned (`let x;`).
    Undefined,
}

/// Where the module renders: `createRoot(document.getElementById(id)).render(<App/>)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Root {
    /// The `id` of the container element in the HTML shell.
    pub container_id: String,
    /// The element rendered into it (a function of no arguments returning it).
    pub element: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub kind: FunctionKind,
    /// One pattern per parameter, binding frame slots.
    pub params: Vec<Pattern>,
    /// Frame size.
    pub n_locals: u32,
    /// What a closure copies when created, in capture-slot order.
    pub captures: Vec<Capture>,
    pub body: Vec<Stmt>,
    /// Static types of the frame slots (`n_locals` entries).
    pub local_types: Vec<Ty>,
    pub ret: Ty,
    /// Source line of the declaration.
    pub line: u32,
    /// For a component: it calls `useEffect`/`useLayoutEffect` without a
    /// dependency list, so skipping one of its renders would be observable.
    pub has_depless_effect: bool,
    /// Frame slots that closures capture and someone reassigns: they live in a
    /// shared cell (created by each `Let` that binds them), so the frame and every
    /// closure see one variable, as in JavaScript.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boxed: Vec<u32>,
    /// An `async` function: calling it returns a promise, and `await` suspends it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_async: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionKind {
    /// Used as `<Name />`: called with one props object, may call hooks.
    Component,
    /// A custom hook (`useX`): may call hooks.
    Hook,
    /// Anything else: a helper, a handler, a callback.
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capture {
    /// A slot of the enclosing frame.
    Local(u32),
    /// A capture of the enclosing closure.
    Capture(u32),
}

/// A destructuring target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    Local(u32),
    /// `[a, , b, ...rest]`.
    Array {
        items: Vec<Option<Pattern>>,
        rest: Option<Box<Pattern>>,
    },
    /// `{a, b: c, ...rest}`.
    Object {
        props: Vec<(String, Pattern)>,
        rest: Option<Box<Pattern>>,
    },
    /// `p = default`: the default when the value is `undefined`.
    Default(Box<Pattern>, Expr),
    /// A parameter nobody reads.
    Ignore,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// `const`/`let` declaration (also used for function declarations inside a body).
    Let(Pattern, Option<Expr>),
    Expr(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Return(Option<Expr>),
    /// `for (const x of xs)`.
    ForOf(Pattern, Expr, Vec<Stmt>),
    /// `for (init; test; update)`; `while` has no init/update.
    For {
        init: Vec<Stmt>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Vec<Stmt>,
    },
    /// `switch`: cases in order (`None` is `default`), with fallthrough.
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Break,
    Continue,
    Block(Vec<Stmt>),
    /// `throw expr`.
    Throw(Expr),
    /// `try { block } catch (param) { handler } finally { finalizer }`.
    Try {
        block: Vec<Stmt>,
        param: Option<Pattern>,
        handler: Option<Vec<Stmt>>,
        finalizer: Option<Vec<Stmt>>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Local(u32),
    Capture(u32),
    Global(u32),
    /// A template literal: `quasis.len() == exprs.len() + 1`.
    Template(Vec<String>, Vec<Expr>),
    Array(Vec<ArrayItem>),
    Object(Vec<Prop>),
    /// `obj.name` / `obj?.name`.
    Member(Box<Expr>, String, bool),
    /// `obj[index]` / `obj?.[index]`.
    Index(Box<Expr>, Box<Expr>, bool),
    /// `f(args)` / `f?.(args)`.
    Call(Box<Expr>, Vec<ArrayItem>, bool),
    /// A built-in method on a receiver: `xs.map(f)`, `s.trim()`; `optional` is `?.`.
    Method {
        recv: Box<Expr>,
        method: Method,
        args: Vec<ArrayItem>,
        optional: bool,
    },
    /// A built-in function or constant: `Math.max(...)`, `Number(x)`, `JSON.stringify`.
    Builtin(Builtin, Vec<ArrayItem>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `&&`, `||`, `??`.
    Logical(LogicalOp, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `target op= value`.
    Assign(Box<LValue>, Option<BinaryOp>, Box<Expr>),
    /// `++x`, `x--`: `prefix`, delta.
    Update(Box<LValue>, bool, f64),
    /// Creates a closure of function `n`.
    Closure(u32),
    Hook(Hook, Vec<Expr>),
    Element(Box<ElementExpr>),
    Seq(Vec<Expr>),
    /// `typeof x`.
    TypeOf(Box<Expr>),
    /// The boundary of an optional chain: an optional link (`?.`) that meets
    /// `null`/`undefined` makes the whole chain `undefined`.
    Chain(Box<Expr>),
    /// A regular expression literal: pattern and flags (JavaScript syntax). Each
    /// evaluation is a new `RegExp` object, as in JavaScript.
    Regex(String, String),
    /// `await expr`, in an async function. The compiler puts it only where the
    /// runtime can suspend: a whole `let` initialiser, expression statement,
    /// assignment's right side or `return` value.
    Await(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ArrayItem {
    Item(Expr),
    Spread(Expr),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Prop {
    KeyValue(String, Expr),
    Computed(Expr, Expr),
    Spread(Expr),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LValue {
    Local(u32),
    /// A captured variable of the enclosing function (always a boxed slot there).
    Capture(u32),
    Global(u32),
    Member(Expr, String),
    Index(Expr, Expr),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
    Plus,
    BitNot,
    Void,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Exp,
    Eq,
    NotEq,
    StrictEq,
    StrictNotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    In,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicalOp {
    And,
    Or,
    Nullish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hook {
    /// `useState(init)`; a function `init` is called once.
    State,
    /// `useReducer(reducer, init, initFn?)`.
    Reducer,
    /// `useMemo(fn, deps)`.
    Memo,
    /// `useCallback(fn, deps)`.
    Callback,
    /// `useRef(init)`.
    Ref,
    /// `useEffect(fn, deps?)`.
    Effect,
    /// `useLayoutEffect(fn, deps?)`.
    LayoutEffect,
    /// `useContext(Ctx)`.
    Context,
    /// `useId()`.
    Id,
    /// `useSyncExternalStore(subscribe, getSnapshot)`.
    SyncExternalStore,
}

/// Methods the runtime implements natively, resolved by the compiler from the
/// receiver's static type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    // Arrays.
    ArrayMap,
    ArrayFilter,
    ArrayFind,
    ArrayFindIndex,
    ArrayFindLast,
    ArraySome,
    ArrayEvery,
    ArrayReduce,
    ArrayForEach,
    ArraySlice,
    ArrayConcat,
    ArrayIncludes,
    ArrayIndexOf,
    ArrayJoin,
    ArraySort,
    ArrayToSorted,
    ArrayReverse,
    ArrayToReversed,
    ArrayPush,
    ArrayPop,
    ArrayShift,
    ArrayUnshift,
    ArraySplice,
    ArrayFlat,
    ArrayFlatMap,
    ArrayFill,
    ArrayAt,
    ArrayKeys,
    ArrayEntries,
    ArrayWith,
    // Strings.
    StrTrim,
    StrTrimStart,
    StrTrimEnd,
    StrToUpperCase,
    StrToLowerCase,
    StrIncludes,
    StrStartsWith,
    StrEndsWith,
    StrIndexOf,
    StrLastIndexOf,
    StrSlice,
    StrSubstring,
    StrSplit,
    StrReplace,
    StrReplaceAll,
    StrRepeat,
    StrPadStart,
    StrPadEnd,
    StrCharAt,
    StrCharCodeAt,
    StrAt,
    StrLocaleCompare,
    StrConcat,
    StrCodePointAt,
    // Numbers.
    NumToFixed,
    NumToString,
    // Any value.
    ToString,
    // `Promise` values.
    PromiseThen,
    PromiseCatch,
    PromiseFinally,
    // A `fetch` response.
    ResponseJson,
    ResponseText,
    /// `response.headers.get(name)` / `.has(name)`.
    HeadersGet,
    HeadersHas,
    // A DOM node reached through a ref or `event.target`.
    NodeFocus,
    NodeBlur,
    NodeSelect,
    /// `el.setSelectionRange(start, end)`.
    NodeSetSelectionRange,
    // An event.
    EventPreventDefault,
    EventStopPropagation,
    // A `RegExp`, and strings searched with one.
    RegexTest,
    RegexExec,
    StrMatch,
    StrSearch,
    // `Set` and `Map`.
    SetHas,
    SetAdd,
    SetDelete,
    SetClear,
    MapGet,
    MapSet,
    CollectionForEach,
    CollectionKeys,
    CollectionValues,
    CollectionEntries,
    // Maps of `Object.*`: none; they are builtins.
}

/// Built-in functions (called with the argument list) and constants (no arguments).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Builtin {
    MathMax,
    MathMin,
    MathRound,
    MathFloor,
    MathCeil,
    MathAbs,
    MathTrunc,
    MathSign,
    MathSqrt,
    MathPow,
    MathRandom,
    MathPi,
    Number,
    NumberIsNaN,
    NumberIsInteger,
    NumberIsFinite,
    ParseInt,
    ParseFloat,
    IsNaN,
    String,
    Boolean,
    ArrayIsArray,
    ArrayFrom,
    ArrayOf,
    ObjectKeys,
    ObjectValues,
    ObjectEntries,
    ObjectAssign,
    ObjectFromEntries,
    JsonStringify,
    JsonParse,
    /// `Date.now()` on the world clock.
    DateNow,
    ConsoleLog,
    ConsoleWarn,
    ConsoleError,
    SetTimeout,
    ClearTimeout,
    SetInterval,
    ClearInterval,
    Fetch,
    PromiseResolve,
    Infinity,
    NaN,
    /// `document.title = x` is an assignment; reading it is this.
    DocumentTitle,
    /// `new Error(msg)` (or `TypeError`, … by name in the first argument's place:
    /// `Error` is `new Error(message)`).
    Error,
    /// `new TypeError(msg)`.
    TypeError,
    /// `new Promise(executor)`.
    NewPromise,
    /// `Promise.all(promises)`.
    PromiseAll,
    /// `Promise.reject(reason)`.
    PromiseReject,
    /// `Object.is(a, b)`.
    ObjectIs,
    /// `x instanceof Error` (the constructor's name is the second argument).
    IsError,
    /// `window.addEventListener(type, listener, options?)`.
    WindowAddListener,
    WindowRemoveListener,
    /// `document.addEventListener(type, listener, options?)`.
    DocumentAddListener,
    DocumentRemoveListener,
    /// `document.getElementById(id)`.
    GetElementById,
    /// `document.querySelector(selector)`.
    QuerySelector,
    /// `document.activeElement`.
    ActiveElement,
    /// `document.body`.
    DocumentBody,
    /// `window.innerWidth` / `window.innerHeight`.
    InnerWidth,
    InnerHeight,
    /// The `cw` global of a computerworld desktop web app (see `crate::cw`).
    /// `cw.kind`, `cw.argument`, `cw.env`.
    CwKind,
    CwArgument,
    CwEnv,
    /// `cw.onEnv(listener)`: returns the function that removes it.
    CwOnEnv,
    /// `cw.now()`.
    CwNow,
    /// `cw.state.get()`, `cw.state.set(value)`.
    CwStateGet,
    CwStateSet,
    /// `cw.fs.readFile(path)`, `writeFile(path, content)`, `list(path)`, `mkdir(path)`.
    CwReadFile,
    CwWriteFile,
    CwList,
    CwMkdir,
    /// `cw.fetch(url, init?)`.
    CwFetch,
    /// `cw.launch(kind, argument?)`, `cw.emit(name, data?)`.
    CwLaunch,
    CwEmit,
    /// `cw.refuse(message)`.
    CwRefuse,
    /// `cw.window.set(facts)`.
    CwWindowSet,
    /// `new Set(iterable?)`.
    NewSet,
    /// `new Map(entries?)`.
    NewMap,
}

/// A JSX element expression.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ElementExpr {
    /// A host element tree: template `template` with hole values in order.
    Template {
        template: u32,
        holes: Vec<Expr>,
        key: Option<Expr>,
    },
    /// `<Comp {...props}>children</Comp>`: `callee` evaluates to the component.
    Component {
        callee: Expr,
        props: Vec<Prop>,
        /// `props.children`: absent, one child, or an array of several.
        children: Option<Expr>,
        key: Option<Expr>,
    },
    /// `<>…</>` / `<Fragment key=…>`.
    Fragment {
        children: Vec<Expr>,
        key: Option<Expr>,
    },
    /// `<Ctx.Provider value={v}>`.
    Provider {
        context: Expr,
        value: Expr,
        children: Vec<Expr>,
        key: Option<Expr>,
    },
}

/// A static host-element tree with holes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub root: TNode,
    /// One entry per hole, in hole order.
    pub holes: Vec<Hole>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    /// Frame slots (`Local`) and captures (`Capture`) the hole expression reads,
    /// directly or through closures it creates; empty with `always` set when it
    /// reads something identity cannot track (a ref's `.current`, a global that is
    /// reassigned).
    pub deps: Vec<Capture>,
    pub always: bool,
    pub ty: Ty,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TNode {
    Element {
        tag: String,
        /// In JSX order, which is the order React applies them.
        attrs: Vec<TAttr>,
        children: Vec<TNode>,
    },
    Text(String),
    /// A dynamic child: text, number, element, array, or nothing.
    Hole(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TAttr {
    /// A constant attribute, already mapped to its DOM name (`className` → `class`).
    Static(String, String),
    /// A dynamic React prop (`className`, `style`, `value`, `onClick`, ...).
    Dynamic(String, u32),
    /// `{...props}`: an object of React props.
    Spread(u32),
    /// `ref={r}`.
    Ref(u32),
}

/// Static types, for diagnostics and future code generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Ty {
    Number,
    String,
    Boolean,
    Null,
    Undefined,
    Void,
    Array(Box<Ty>),
    Tuple(Vec<Ty>),
    Object(Vec<(String, Ty, bool)>),
    /// `Record<string, T>` / `{ [k: string]: T }`.
    Dict(Box<Ty>),
    Union(Vec<Ty>),
    Function(Vec<Ty>, Box<Ty>),
    /// Anything React can render (`ReactNode`, `JSX.Element`).
    Node,
    Ref(Box<Ty>),
    Setter(Box<Ty>),
    Dispatch(Box<Ty>),
    Context(Box<Ty>),
    Event,
    DomNode,
    Promise(Box<Ty>),
    Response,
    /// A `cw.fetch` response: a `Response` whose `body` is its text.
    CwResponse,
    /// `response.headers` (evaluates to the response itself; `get`/`has` read it).
    Headers,
    Regex,
    Error,
    Set(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    /// A string literal type (unions of them are how TS spells enums of names).
    Lit(String),
    NumLit(f64),
    Unknown,
    /// A generic function's type parameter, in the signature its callers
    /// instantiate (never in a lowered value's type).
    Param(String),
}
