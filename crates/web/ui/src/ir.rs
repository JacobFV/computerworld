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
    /// The app's code outside the compiled subset and the packages it imports,
    /// run on the JS VM beside the compiled code (see `cw_ui::island`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub island: Option<Island>,
    /// Whether any code mutates a value that outlives the render that created it
    /// (`state.push(x)`, `props.items.sort()`, `obj.field = v` on a non-fresh
    /// object). When set, the runtime never skips a hole by identity of its inputs.
    pub mutates_shared: bool,
}

/// The island of an app: a script for the JS VM and what compiled code takes from
/// it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Island {
    /// A classic script. It defines `__cw_exports`, an array of the values
    /// `GlobalInit::Island` globals are initialised with, in `imports` order.
    pub script: String,
    /// What each export is: a module (its file when it resolved, else the
    /// specifier) and a name (`"default"`, `"*"` for the namespace, or `"!run"`
    /// for running a module of the app that is on the island).
    pub imports: Vec<(String, String)>,
    /// What the island's modules of the app import from compiled ones: module
    /// file, exported name, and the global slot it reads (`__cw.g(slot)`).
    #[serde(default)]
    pub provides: Vec<(String, String, u32)>,
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
    /// Not a binding: module-level code that runs at this point of the module's
    /// initialisation (function `n`, called with no arguments), such as a
    /// statement with effects or an initialiser with variables of its own.
    Run(u32),
    /// Export `n` of the app's island (a package's value, or app code outside the
    /// compiled subset).
    Island(u32),
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
    /// `...rest`: bound to an array of the arguments after `params`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<Pattern>,
    /// A `forwardRef` render function: a component called with its element's
    /// `ref` as the second argument.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forward_ref: bool,
}

impl Function {
    /// How many arguments a caller that builds them lazily (an array method's
    /// callback) passes: every one for a function with a rest parameter.
    pub fn arity(&self) -> usize {
        if self.rest.is_some() {
            self.params.len() + 3
        } else {
            self.params.len()
        }
    }
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
    /// A built-in function as a value (`Boolean` in `xs.filter(Boolean)`).
    BuiltinFn(Builtin),
    /// `recv.name(args)` where the receiver's type does not say which method it is:
    /// resolved when it runs, by the value's kind, as JavaScript looks the method up
    /// on the receiver (a built-in's method, or an object's function property).
    Invoke {
        recv: Box<Expr>,
        name: String,
        args: Vec<ArrayItem>,
        optional: bool,
    },
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
    /// `useImperativeHandle(ref, create, deps?)`: a layout effect setting the ref
    /// to `create()`, and to null when it is cleaned up.
    ImperativeHandle,
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
    /// `el.getBoundingClientRect()`: a `DOMRect` (as a plain object).
    NodeGetBoundingClientRect,
    NodeGetClientRects,
    /// `el.scrollIntoView(arg)`.
    NodeScrollIntoView,
    /// `el.scrollTo(x, y)` / `scrollTo({ left, top })`, and `scroll`.
    NodeScrollTo,
    NodeScrollBy,
    NodeContains,
    NodeClosest,
    NodeMatches,
    NodeGetAttribute,
    NodeHasAttribute,
    NodeQuerySelector,
    NodeQuerySelectorAll,
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
    // `Date` (the local time zone is UTC, as on the JS VM).
    DateGetFullYear,
    DateGetMonth,
    DateGetDate,
    DateGetDay,
    DateGetHours,
    DateGetMinutes,
    DateGetSeconds,
    DateGetMilliseconds,
    /// `getTime` and `valueOf`.
    DateGetTime,
    DateGetTimezoneOffset,
    DateGetYear,
    DateSetFullYear,
    DateSetMonth,
    DateSetDate,
    DateSetHours,
    DateSetMinutes,
    DateSetSeconds,
    DateSetMilliseconds,
    DateSetTime,
    DateToISOString,
    DateToJSON,
    DateToString,
    DateToDateString,
    DateToTimeString,
    DateToUTCString,
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
    /// `document.documentElement`.
    DocumentElement,
    /// `document.querySelectorAll(selector)`: an array of elements.
    QuerySelectorAll,
    /// `window.scrollX` / `scrollY` (and `pageXOffset` / `pageYOffset`).
    ScrollX,
    ScrollY,
    /// `window.scrollTo(x, y)` / `scrollTo({ left, top })` / `scroll`.
    WindowScrollTo,
    WindowScrollBy,
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
    /// `Array(n)` / `new Array(n)` / `new Array(a, b)`.
    NewArray,
    /// `new Date(...)`: now on the world clock, a time value, a string to parse,
    /// or local fields (the local time zone is UTC, as on the JS VM).
    NewDate,
    /// `Date.UTC(y, m, …)` and `Date.parse(s)`.
    DateUTC,
    DateParse,
    /// `new C(args)` for a constructor that is not built in (an island's class).
    Construct,
    /// `x instanceof C` for a built-in constructor named by the second argument
    /// (`Date`, `Array`, `Map`, `Set`, `RegExp`, `Promise`, `Object`, `Function`).
    IsInstance,
    /// `startTransition(fn)`: calls `fn` (there is no concurrent rendering).
    StartTransition,
    /// `delete obj[key]`: `true`.
    Delete,
    /// `localStorage`/`sessionStorage` (area 0/1, the first argument): `getItem`,
    /// `setItem`, `removeItem`, `clear`, `key`, `length`.
    StorageGet,
    StorageSet,
    StorageRemove,
    StorageClear,
    StorageKey,
    StorageLength,
    /// `location.href`, `.pathname`, … (the part's name is the argument).
    LocationPart,
    /// `alert`/`confirm`/`prompt` (kind, message): the host records `kind: text`.
    Alert,
    /// The keys a `for...in` visits: an object's enumerable string keys in order
    /// (an array's or string's indices), none for `null`/`undefined`.
    ForInKeys,
    /// `history.pushState(state, title, url?)` / `replaceState`.
    HistoryPush,
    HistoryReplace,
    /// `history.go(delta)` (`back` is -1, `forward` 1): traversed on a timer.
    HistoryGo,
    /// The traversal a `history.go` scheduled (delta, the URL it was called at).
    HistoryTraverse,
    /// `history.length`, `history.state`.
    HistoryLength,
    HistoryState,
    /// `location[part] = value` (part name, value).
    LocationSet,
    /// `location.assign(url)`, `location.replace(url)`, `location.reload()`.
    LocationAssign,
    LocationReplace,
    LocationReload,
    /// `requestAnimationFrame(cb)` / `cancelAnimationFrame(id)`: callbacks run
    /// once per 16 ms of the world clock, as the Realm runs them.
    RequestAnimationFrame,
    CancelAnimationFrame,
    /// `performance.now()`: the Realm's (virtual milliseconds, plus its boot offset).
    PerformanceNow,
    /// `matchMedia(query)`: a MediaQueryList, re-evaluated on resize.
    MatchMedia,
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
    /// A `Date`.
    Date,
    /// A string literal type (unions of them are how TS spells enums of names).
    Lit(String),
    NumLit(f64),
    Unknown,
    /// A generic function's type parameter, in the signature its callers
    /// instantiate (never in a lowered value's type).
    Param(String),
}

/// The kinds of receiver a built-in method is looked up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    Array,
    String,
    Number,
    Boolean,
    Promise,
    /// A `fetch` response (also standing for its `headers`).
    Response,
    Regex,
    Set,
    Map,
    /// A DOM element.
    Node,
    Event,
    Date,
}

/// The built-in method `name` on a receiver of kind `kind`, as the runtime
/// implements it; `None` when there is none (the method is not a built-in of that
/// kind, or the runtime lacks it: see [`unimplemented_builtin`]).
pub fn method_by_name(kind: MethodKind, name: &str) -> Option<Method> {
    use Method as M;
    use MethodKind as K;
    Some(match (kind, name) {
        (K::Array, "map") => M::ArrayMap,
        (K::Array, "filter") => M::ArrayFilter,
        (K::Array, "find") => M::ArrayFind,
        (K::Array, "findIndex") => M::ArrayFindIndex,
        (K::Array, "findLast") => M::ArrayFindLast,
        (K::Array, "some") => M::ArraySome,
        (K::Array, "every") => M::ArrayEvery,
        (K::Array, "reduce") => M::ArrayReduce,
        (K::Array, "forEach") => M::ArrayForEach,
        (K::Array, "slice") => M::ArraySlice,
        (K::Array, "concat") => M::ArrayConcat,
        (K::Array, "includes") => M::ArrayIncludes,
        (K::Array, "indexOf") => M::ArrayIndexOf,
        (K::Array, "join") => M::ArrayJoin,
        (K::Array, "sort") => M::ArraySort,
        (K::Array, "toSorted") => M::ArrayToSorted,
        (K::Array, "reverse") => M::ArrayReverse,
        (K::Array, "toReversed") => M::ArrayToReversed,
        (K::Array, "push") => M::ArrayPush,
        (K::Array, "pop") => M::ArrayPop,
        (K::Array, "shift") => M::ArrayShift,
        (K::Array, "unshift") => M::ArrayUnshift,
        (K::Array, "splice") => M::ArraySplice,
        (K::Array, "flat") => M::ArrayFlat,
        (K::Array, "flatMap") => M::ArrayFlatMap,
        (K::Array, "fill") => M::ArrayFill,
        (K::Array, "at") => M::ArrayAt,
        (K::Array, "keys") => M::ArrayKeys,
        (K::Array, "entries") => M::ArrayEntries,
        (K::Array, "with") => M::ArrayWith,
        (K::String, "trim") => M::StrTrim,
        (K::String, "trimStart") => M::StrTrimStart,
        (K::String, "trimEnd") => M::StrTrimEnd,
        (K::String, "toUpperCase" | "toLocaleUpperCase") => M::StrToUpperCase,
        (K::String, "toLowerCase" | "toLocaleLowerCase") => M::StrToLowerCase,
        (K::String, "includes") => M::StrIncludes,
        (K::String, "startsWith") => M::StrStartsWith,
        (K::String, "endsWith") => M::StrEndsWith,
        (K::String, "indexOf") => M::StrIndexOf,
        (K::String, "lastIndexOf") => M::StrLastIndexOf,
        (K::String, "slice") => M::StrSlice,
        (K::String, "substring") => M::StrSubstring,
        (K::String, "split") => M::StrSplit,
        (K::String, "replace") => M::StrReplace,
        (K::String, "replaceAll") => M::StrReplaceAll,
        (K::String, "repeat") => M::StrRepeat,
        (K::String, "padStart") => M::StrPadStart,
        (K::String, "padEnd") => M::StrPadEnd,
        (K::String, "charAt") => M::StrCharAt,
        (K::String, "charCodeAt") => M::StrCharCodeAt,
        (K::String, "codePointAt") => M::StrCodePointAt,
        (K::String, "at") => M::StrAt,
        (K::String, "localeCompare") => M::StrLocaleCompare,
        (K::String, "concat") => M::StrConcat,
        (K::String, "match") => M::StrMatch,
        (K::String, "search") => M::StrSearch,
        (K::Number, "toFixed") => M::NumToFixed,
        (K::Number, "toString") => M::NumToString,
        (K::Promise, "then") => M::PromiseThen,
        (K::Promise, "catch") => M::PromiseCatch,
        (K::Promise, "finally") => M::PromiseFinally,
        (K::Response, "json") => M::ResponseJson,
        (K::Response, "text") => M::ResponseText,
        (K::Response, "get") => M::HeadersGet,
        (K::Response, "has") => M::HeadersHas,
        (K::Regex, "test") => M::RegexTest,
        (K::Regex, "exec") => M::RegexExec,
        (K::Set, "has") => M::SetHas,
        (K::Set, "add") => M::SetAdd,
        (K::Set | K::Map, "delete") => M::SetDelete,
        (K::Set | K::Map, "clear") => M::SetClear,
        (K::Map, "has") => M::SetHas,
        (K::Map, "get") => M::MapGet,
        (K::Map, "set") => M::MapSet,
        (K::Set | K::Map, "forEach") => M::CollectionForEach,
        (K::Set | K::Map, "keys") => M::CollectionKeys,
        (K::Set | K::Map, "values") => M::CollectionValues,
        (K::Set | K::Map, "entries") => M::CollectionEntries,
        (K::Node, "focus") => M::NodeFocus,
        (K::Node, "blur") => M::NodeBlur,
        (K::Node, "select") => M::NodeSelect,
        (K::Node, "setSelectionRange") => M::NodeSetSelectionRange,
        (K::Node, "getBoundingClientRect") => M::NodeGetBoundingClientRect,
        (K::Node, "getClientRects") => M::NodeGetClientRects,
        (K::Node, "scrollIntoView") => M::NodeScrollIntoView,
        (K::Node, "scrollTo" | "scroll") => M::NodeScrollTo,
        (K::Node, "scrollBy") => M::NodeScrollBy,
        (K::Node, "contains") => M::NodeContains,
        (K::Node, "closest") => M::NodeClosest,
        (K::Node, "matches") => M::NodeMatches,
        (K::Node, "getAttribute") => M::NodeGetAttribute,
        (K::Node, "hasAttribute") => M::NodeHasAttribute,
        (K::Node, "querySelector") => M::NodeQuerySelector,
        (K::Node, "querySelectorAll") => M::NodeQuerySelectorAll,
        (K::Event, "preventDefault") => M::EventPreventDefault,
        (K::Date, "getFullYear" | "getUTCFullYear") => M::DateGetFullYear,
        (K::Date, "getMonth" | "getUTCMonth") => M::DateGetMonth,
        (K::Date, "getDate" | "getUTCDate") => M::DateGetDate,
        (K::Date, "getDay" | "getUTCDay") => M::DateGetDay,
        (K::Date, "getHours" | "getUTCHours") => M::DateGetHours,
        (K::Date, "getMinutes" | "getUTCMinutes") => M::DateGetMinutes,
        (K::Date, "getSeconds" | "getUTCSeconds") => M::DateGetSeconds,
        (K::Date, "getMilliseconds" | "getUTCMilliseconds") => M::DateGetMilliseconds,
        (K::Date, "getTime" | "valueOf") => M::DateGetTime,
        (K::Date, "getTimezoneOffset") => M::DateGetTimezoneOffset,
        (K::Date, "getYear") => M::DateGetYear,
        (K::Date, "setFullYear" | "setUTCFullYear") => M::DateSetFullYear,
        (K::Date, "setMonth" | "setUTCMonth") => M::DateSetMonth,
        (K::Date, "setDate" | "setUTCDate") => M::DateSetDate,
        (K::Date, "setHours" | "setUTCHours") => M::DateSetHours,
        (K::Date, "setMinutes" | "setUTCMinutes") => M::DateSetMinutes,
        (K::Date, "setSeconds" | "setUTCSeconds") => M::DateSetSeconds,
        (K::Date, "setMilliseconds" | "setUTCMilliseconds") => M::DateSetMilliseconds,
        (K::Date, "setTime") => M::DateSetTime,
        (K::Date, "toISOString") => M::DateToISOString,
        (K::Date, "toJSON") => M::DateToJSON,
        (K::Date, "toString") => M::DateToString,
        (K::Date, "toDateString") => M::DateToDateString,
        (K::Date, "toTimeString") => M::DateToTimeString,
        (K::Date, "toUTCString" | "toGMTString") => M::DateToUTCString,
        (K::Event, "stopPropagation") => M::EventStopPropagation,
        (_, "toString") => M::ToString,
        _ => return None,
    })
}

/// The JavaScript name of a built-in method (for calling it on a value of the
/// island, which has its own).
pub fn method_js_name(m: Method) -> &'static str {
    use Method as M;
    match m {
        M::ArrayMap => "map",
        M::ArrayFilter => "filter",
        M::ArrayFind => "find",
        M::ArrayFindIndex => "findIndex",
        M::ArrayFindLast => "findLast",
        M::ArraySome => "some",
        M::ArrayEvery => "every",
        M::ArrayReduce => "reduce",
        M::ArrayForEach => "forEach",
        M::ArraySlice | M::StrSlice => "slice",
        M::ArrayConcat | M::StrConcat => "concat",
        M::ArrayIncludes | M::StrIncludes => "includes",
        M::ArrayIndexOf | M::StrIndexOf => "indexOf",
        M::ArrayJoin => "join",
        M::ArraySort => "sort",
        M::ArrayToSorted => "toSorted",
        M::ArrayReverse => "reverse",
        M::ArrayToReversed => "toReversed",
        M::ArrayPush => "push",
        M::ArrayPop => "pop",
        M::ArrayShift => "shift",
        M::ArrayUnshift => "unshift",
        M::ArraySplice => "splice",
        M::ArrayFlat => "flat",
        M::ArrayFlatMap => "flatMap",
        M::ArrayFill => "fill",
        M::ArrayAt | M::StrAt => "at",
        M::ArrayKeys | M::CollectionKeys => "keys",
        M::ArrayEntries | M::CollectionEntries => "entries",
        M::ArrayWith => "with",
        M::StrTrim => "trim",
        M::StrTrimStart => "trimStart",
        M::StrTrimEnd => "trimEnd",
        M::StrToUpperCase => "toUpperCase",
        M::StrToLowerCase => "toLowerCase",
        M::StrStartsWith => "startsWith",
        M::StrEndsWith => "endsWith",
        M::StrLastIndexOf => "lastIndexOf",
        M::StrSubstring => "substring",
        M::StrSplit => "split",
        M::StrReplace => "replace",
        M::StrReplaceAll => "replaceAll",
        M::StrRepeat => "repeat",
        M::StrPadStart => "padStart",
        M::StrPadEnd => "padEnd",
        M::StrCharAt => "charAt",
        M::StrCharCodeAt => "charCodeAt",
        M::StrLocaleCompare => "localeCompare",
        M::StrCodePointAt => "codePointAt",
        M::NumToFixed => "toFixed",
        M::NumToString | M::ToString | M::DateToString => "toString",
        M::PromiseThen => "then",
        M::PromiseCatch => "catch",
        M::PromiseFinally => "finally",
        M::ResponseJson => "json",
        M::ResponseText => "text",
        M::HeadersGet | M::MapGet => "get",
        M::HeadersHas | M::SetHas => "has",
        M::NodeFocus => "focus",
        M::NodeBlur => "blur",
        M::NodeSelect => "select",
        M::NodeSetSelectionRange => "setSelectionRange",
        M::NodeGetBoundingClientRect => "getBoundingClientRect",
        M::NodeGetClientRects => "getClientRects",
        M::NodeScrollIntoView => "scrollIntoView",
        M::NodeScrollTo => "scrollTo",
        M::NodeScrollBy => "scrollBy",
        M::NodeContains => "contains",
        M::NodeClosest => "closest",
        M::NodeMatches => "matches",
        M::NodeGetAttribute => "getAttribute",
        M::NodeHasAttribute => "hasAttribute",
        M::NodeQuerySelector => "querySelector",
        M::NodeQuerySelectorAll => "querySelectorAll",
        M::EventPreventDefault => "preventDefault",
        M::EventStopPropagation => "stopPropagation",
        M::RegexTest => "test",
        M::RegexExec => "exec",
        M::StrMatch => "match",
        M::StrSearch => "search",
        M::SetAdd => "add",
        M::SetDelete => "delete",
        M::SetClear => "clear",
        M::MapSet => "set",
        M::CollectionForEach => "forEach",
        M::CollectionValues => "values",
        M::DateGetFullYear => "getFullYear",
        M::DateGetMonth => "getMonth",
        M::DateGetDate => "getDate",
        M::DateGetDay => "getDay",
        M::DateGetHours => "getHours",
        M::DateGetMinutes => "getMinutes",
        M::DateGetSeconds => "getSeconds",
        M::DateGetMilliseconds => "getMilliseconds",
        M::DateGetTime => "getTime",
        M::DateGetTimezoneOffset => "getTimezoneOffset",
        M::DateGetYear => "getYear",
        M::DateSetFullYear => "setFullYear",
        M::DateSetMonth => "setMonth",
        M::DateSetDate => "setDate",
        M::DateSetHours => "setHours",
        M::DateSetMinutes => "setMinutes",
        M::DateSetSeconds => "setSeconds",
        M::DateSetMilliseconds => "setMilliseconds",
        M::DateSetTime => "setTime",
        M::DateToISOString => "toISOString",
        M::DateToJSON => "toJSON",
        M::DateToDateString => "toDateString",
        M::DateToTimeString => "toTimeString",
        M::DateToUTCString => "toUTCString",
    }
}

/// Methods of JavaScript's built-in prototypes, by the kind they belong to.
const BUILTIN_METHODS: &[(MethodKind, &[&str])] = &[
    (
        MethodKind::Array,
        &[
            "at",
            "concat",
            "copyWithin",
            "entries",
            "every",
            "fill",
            "filter",
            "find",
            "findIndex",
            "findLast",
            "findLastIndex",
            "flat",
            "flatMap",
            "forEach",
            "includes",
            "indexOf",
            "join",
            "keys",
            "lastIndexOf",
            "map",
            "pop",
            "push",
            "reduce",
            "reduceRight",
            "reverse",
            "shift",
            "slice",
            "some",
            "sort",
            "splice",
            "toLocaleString",
            "toReversed",
            "toSorted",
            "toSpliced",
            "toString",
            "unshift",
            "values",
            "with",
        ],
    ),
    (
        MethodKind::String,
        &[
            "at",
            "charAt",
            "charCodeAt",
            "codePointAt",
            "concat",
            "endsWith",
            "includes",
            "indexOf",
            "isWellFormed",
            "lastIndexOf",
            "localeCompare",
            "match",
            "matchAll",
            "normalize",
            "padEnd",
            "padStart",
            "repeat",
            "replace",
            "replaceAll",
            "search",
            "slice",
            "split",
            "startsWith",
            "substr",
            "substring",
            "toLocaleLowerCase",
            "toLocaleUpperCase",
            "toLowerCase",
            "toString",
            "toUpperCase",
            "toWellFormed",
            "trim",
            "trimEnd",
            "trimStart",
            "valueOf",
        ],
    ),
    (
        MethodKind::Number,
        &[
            "toExponential",
            "toFixed",
            "toLocaleString",
            "toPrecision",
            "toString",
            "valueOf",
        ],
    ),
    (MethodKind::Promise, &["then", "catch", "finally"]),
    (
        MethodKind::Date,
        &[
            "getDate",
            "getDay",
            "getFullYear",
            "getHours",
            "getMilliseconds",
            "getMinutes",
            "getMonth",
            "getSeconds",
            "getTime",
            "getTimezoneOffset",
            "getUTCDate",
            "getUTCDay",
            "getUTCFullYear",
            "getUTCHours",
            "getUTCMilliseconds",
            "getUTCMinutes",
            "getUTCMonth",
            "getUTCSeconds",
            "getYear",
            "setDate",
            "setFullYear",
            "setHours",
            "setMilliseconds",
            "setMinutes",
            "setMonth",
            "setSeconds",
            "setTime",
            "setUTCDate",
            "setUTCFullYear",
            "setUTCHours",
            "setUTCMilliseconds",
            "setUTCMinutes",
            "setUTCMonth",
            "setUTCSeconds",
            "toDateString",
            "toISOString",
            "toJSON",
            "toLocaleDateString",
            "toLocaleString",
            "toLocaleTimeString",
            "toString",
            "toTimeString",
            "toUTCString",
            "valueOf",
        ],
    ),
    (MethodKind::Regex, &["exec", "test", "toString"]),
    (
        MethodKind::Set,
        &[
            "add", "clear", "delete", "entries", "forEach", "has", "keys", "values",
        ],
    ),
    (
        MethodKind::Map,
        &[
            "clear", "delete", "entries", "forEach", "get", "has", "keys", "set", "values",
        ],
    ),
];

/// Whether `name` is a method of some built-in prototype that the runtime does not
/// implement for that kind: a call of it on a receiver whose kind is not known
/// until run time cannot be compiled, since it would fail where JavaScript works.
pub fn unimplemented_builtin(name: &str) -> bool {
    BUILTIN_METHODS
        .iter()
        .any(|(k, names)| names.contains(&name) && method_by_name(*k, name).is_none())
}
