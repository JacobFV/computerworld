//! What the island can run. A module there (a package's, or one of the app's
//! outside the compiled subset) runs on the jsvm VM behind cw-ui's React shim,
//! which is not a browser: it has the language's globals, jsvm's Intl, URL and
//! text codecs, and the shim's React, timers, console and a small `document`.
//! A module that needs more (the page's `fetch`, `localStorage`, `history`,
//! `matchMedia`, `requestAnimationFrame`, portals, class components, raw HTML)
//! would build and then behave differently from the page's React, so it is
//! refused here, and the app takes its React fallback whole.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Class, Expression, IdentifierReference, StaticMemberExpression, UnaryExpression, UnaryOperator,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_semantic::{Scoping, SemanticBuilder};

use crate::{Diagnostic, Source};

/// Node's globals, which a browser lacks too: `typeof process` is `"undefined"`
/// on the island as on the page, so only such a test may name them.
const NODE_ONLY: &[&str] = &[
    "process",
    "Buffer",
    "global",
    "require",
    "module",
    "exports",
    "setImmediate",
    "clearImmediate",
    "__dirname",
    "__filename",
];

/// The page's own globals the island lacks: the engine Realm's `window` (its own and inherited property names at this commit) less the island's. A module that names one needs the page.
const PAGE_GLOBALS: &[&str] = &[
    "AbstractRange",
    "Animation",
    "AnimationEvent",
    "Attr",
    "Audio",
    "BeforeUnloadEvent",
    "BroadcastChannel",
    "CDATASection",
    "CSS",
    "CSSConditionRule",
    "CSSFontFaceRule",
    "CSSGroupingRule",
    "CSSImportRule",
    "CSSKeyframeRule",
    "CSSKeyframesRule",
    "CSSLayerBlockRule",
    "CSSLayerStatementRule",
    "CSSMediaRule",
    "CSSNamespaceRule",
    "CSSPageRule",
    "CSSRule",
    "CSSRuleList",
    "CSSStyleDeclaration",
    "CSSStyleRule",
    "CSSStyleSheet",
    "CSSSupportsRule",
    "CanvasGradient",
    "CanvasPattern",
    "CanvasRenderingContext2D",
    "CharacterData",
    "ClipboardEvent",
    "CloseEvent",
    "Comment",
    "CompositionEvent",
    "CustomElementRegistry",
    "CustomEvent",
    "DOMImplementation",
    "DOMParser",
    "DOMPoint",
    "DOMPointReadOnly",
    "DOMRect",
    "DOMRectList",
    "DOMRectReadOnly",
    "DOMStringMap",
    "DOMTokenList",
    "Document",
    "DocumentFragment",
    "DocumentType",
    "DragEvent",
    "Element",
    "ElementInternals",
    "ErrorEvent",
    "EventSource",
    "FileList",
    "FocusEvent",
    "FontFace",
    "FontFaceSet",
    "FormData",
    "FormDataEvent",
    "HTMLAnchorElement",
    "HTMLAreaElement",
    "HTMLAudioElement",
    "HTMLBRElement",
    "HTMLBaseElement",
    "HTMLBodyElement",
    "HTMLButtonElement",
    "HTMLCanvasElement",
    "HTMLCollection",
    "HTMLDListElement",
    "HTMLDataElement",
    "HTMLDataListElement",
    "HTMLDetailsElement",
    "HTMLDialogElement",
    "HTMLDirectoryElement",
    "HTMLDivElement",
    "HTMLDocument",
    "HTMLElement",
    "HTMLEmbedElement",
    "HTMLFieldSetElement",
    "HTMLFontElement",
    "HTMLFormControlsCollection",
    "HTMLFormElement",
    "HTMLFrameElement",
    "HTMLFrameSetElement",
    "HTMLHRElement",
    "HTMLHeadElement",
    "HTMLHeadingElement",
    "HTMLHtmlElement",
    "HTMLIFrameElement",
    "HTMLImageElement",
    "HTMLInputElement",
    "HTMLLIElement",
    "HTMLLabelElement",
    "HTMLLegendElement",
    "HTMLLinkElement",
    "HTMLMapElement",
    "HTMLMarqueeElement",
    "HTMLMediaElement",
    "HTMLMenuElement",
    "HTMLMetaElement",
    "HTMLMeterElement",
    "HTMLModElement",
    "HTMLOListElement",
    "HTMLObjectElement",
    "HTMLOptGroupElement",
    "HTMLOptionElement",
    "HTMLOptionsCollection",
    "HTMLOutputElement",
    "HTMLParagraphElement",
    "HTMLParamElement",
    "HTMLPictureElement",
    "HTMLPreElement",
    "HTMLProgressElement",
    "HTMLQuoteElement",
    "HTMLScriptElement",
    "HTMLSearchElement",
    "HTMLSelectElement",
    "HTMLSlotElement",
    "HTMLSourceElement",
    "HTMLSpanElement",
    "HTMLStyleElement",
    "HTMLTableCaptionElement",
    "HTMLTableCellElement",
    "HTMLTableColElement",
    "HTMLTableElement",
    "HTMLTableRowElement",
    "HTMLTableSectionElement",
    "HTMLTemplateElement",
    "HTMLTextAreaElement",
    "HTMLTimeElement",
    "HTMLTitleElement",
    "HTMLTrackElement",
    "HTMLUListElement",
    "HTMLUnknownElement",
    "HTMLVideoElement",
    "HashChangeEvent",
    "Image",
    "ImageData",
    "InputEvent",
    "IntersectionObserver",
    "IntersectionObserverEntry",
    "KeyboardEvent",
    "MediaList",
    "MediaQueryList",
    "MediaQueryListEvent",
    "MessageChannel",
    "MessageEvent",
    "MessagePort",
    "MouseEvent",
    "MutationObserver",
    "MutationRecord",
    "NamedNodeMap",
    "Node",
    "NodeFilter",
    "NodeIterator",
    "NodeList",
    "Option",
    "PageTransitionEvent",
    "Path2D",
    "PerformanceObserver",
    "PointerEvent",
    "PopStateEvent",
    "ProcessingInstruction",
    "ProgressEvent",
    "PromiseRejectionEvent",
    "RadioNodeList",
    "Range",
    "ReadableStream",
    "ReportingObserver",
    "ResizeObserver",
    "ResizeObserverEntry",
    "SVGAnimatedLength",
    "SVGAnimatedString",
    "SVGElement",
    "SVGGeometryElement",
    "SVGGraphicsElement",
    "SVGPathElement",
    "SVGSVGElement",
    "SVGUseElement",
    "SecurityPolicyViolationEvent",
    "Selection",
    "ShadowRoot",
    "StaticRange",
    "Storage",
    "StorageEvent",
    "StyleSheet",
    "StyleSheetList",
    "SubmitEvent",
    "Text",
    "TextMetrics",
    "ToggleEvent",
    "TouchEvent",
    "TransitionEvent",
    "TreeWalker",
    "UIEvent",
    "WebSocket",
    "WheelEvent",
    "Window",
    "XMLDocument",
    "XMLHttpRequest",
    "XMLHttpRequestEventTarget",
    "XMLHttpRequestUpload",
    "XMLSerializer",
    "__cw_host",
    "blur",
    "cancelIdleCallback",
    "captureEvents",
    "clientInformation",
    "close",
    "closed",
    "createImageBitmap",
    "crossOriginIsolated",
    "customElements",
    "devicePixelRatio",
    "dispatchEvent",
    "find",
    "focus",
    "frames",
    "getComputedStyle",
    "getScreenDetails",
    "getSelection",
    "isSecureContext",
    "length",
    "moveBy",
    "moveTo",
    "name",
    "navigator",
    "onabort",
    "onafterprint",
    "onanimationcancel",
    "onanimationend",
    "onanimationiteration",
    "onanimationstart",
    "onauxclick",
    "onbeforeinput",
    "onbeforeprint",
    "onbeforetoggle",
    "onbeforeunload",
    "onblur",
    "oncancel",
    "oncanplay",
    "oncanplaythrough",
    "onchange",
    "onclick",
    "onclose",
    "oncontextmenu",
    "oncopy",
    "oncuechange",
    "oncut",
    "ondblclick",
    "ondrag",
    "ondragend",
    "ondragenter",
    "ondragleave",
    "ondragover",
    "ondragstart",
    "ondrop",
    "ondurationchange",
    "onemptied",
    "onended",
    "onerror",
    "onfocus",
    "onfocusin",
    "onfocusout",
    "onformdata",
    "ongamepadconnected",
    "ongamepaddisconnected",
    "ongotpointercapture",
    "onhashchange",
    "oninput",
    "oninvalid",
    "onkeydown",
    "onkeypress",
    "onkeyup",
    "onlanguagechange",
    "onload",
    "onloadeddata",
    "onloadedmetadata",
    "onloadstart",
    "onlostpointercapture",
    "onmessage",
    "onmessageerror",
    "onmousedown",
    "onmouseenter",
    "onmouseleave",
    "onmousemove",
    "onmouseout",
    "onmouseover",
    "onmouseup",
    "onmousewheel",
    "onoffline",
    "ononline",
    "onpagehide",
    "onpageshow",
    "onpaste",
    "onpause",
    "onplay",
    "onplaying",
    "onpointercancel",
    "onpointerdown",
    "onpointerenter",
    "onpointerleave",
    "onpointermove",
    "onpointerout",
    "onpointerover",
    "onpointerup",
    "onpopstate",
    "onprogress",
    "onratechange",
    "onrejectionhandled",
    "onreset",
    "onresize",
    "onscroll",
    "onscrollend",
    "onsecuritypolicyviolation",
    "onseeked",
    "onseeking",
    "onselect",
    "onselectionchange",
    "onselectstart",
    "onslotchange",
    "onstalled",
    "onstorage",
    "onsubmit",
    "onsuspend",
    "ontimeupdate",
    "ontoggle",
    "ontouchcancel",
    "ontouchend",
    "ontouchmove",
    "ontouchstart",
    "ontransitioncancel",
    "ontransitionend",
    "ontransitionrun",
    "ontransitionstart",
    "onunhandledrejection",
    "onunload",
    "onvolumechange",
    "onwaiting",
    "onwebkitanimationend",
    "onwebkitanimationiteration",
    "onwebkitanimationstart",
    "onwebkittransitionend",
    "onwheel",
    "open",
    "opener",
    "origin",
    "outerHeight",
    "outerWidth",
    "parent",
    "postMessage",
    "print",
    "releaseEvents",
    "reportError",
    "requestIdleCallback",
    "resizeBy",
    "resizeTo",
    "screen",
    "screenLeft",
    "screenTop",
    "screenX",
    "screenY",
    "status",
    "stop",
    "top",
    "visualViewport",
    "webkitCancelAnimationFrame",
    "webkitRequestAnimationFrame",
];

/// The members of the Realm's `document` the shim's lacks.
const PAGE_DOCUMENT: &[&str] = &[
    "%invoke",
    "ATTRIBUTE_NODE",
    "CDATA_SECTION_NODE",
    "COMMENT_NODE",
    "DOCUMENT_FRAGMENT_NODE",
    "DOCUMENT_NODE",
    "DOCUMENT_POSITION_CONTAINED_BY",
    "DOCUMENT_POSITION_CONTAINS",
    "DOCUMENT_POSITION_DISCONNECTED",
    "DOCUMENT_POSITION_FOLLOWING",
    "DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC",
    "DOCUMENT_POSITION_PRECEDING",
    "DOCUMENT_TYPE_NODE",
    "ELEMENT_NODE",
    "ENTITY_NODE",
    "ENTITY_REFERENCE_NODE",
    "NOTATION_NODE",
    "PROCESSING_INSTRUCTION_NODE",
    "TEXT_NODE",
    "URL",
    "adoptNode",
    "adoptedStyleSheets",
    "all",
    "anchors",
    "append",
    "appendChild",
    "baseURI",
    "caretPositionFromPoint",
    "caretRangeFromPoint",
    "characterSet",
    "charset",
    "childElementCount",
    "childNodes",
    "children",
    "cloneNode",
    "close",
    "compareDocumentPosition",
    "compatMode",
    "contains",
    "contentType",
    "cookie",
    "createAttribute",
    "createAttributeNS",
    "createCDATASection",
    "createComment",
    "createDocumentFragment",
    "createElement",
    "createElementNS",
    "createEvent",
    "createNodeIterator",
    "createProcessingInstruction",
    "createRange",
    "createTextNode",
    "createTreeWalker",
    "currentScript",
    "designMode",
    "dir",
    "dispatchEvent",
    "doctype",
    "documentURI",
    "domain",
    "elementFromPoint",
    "elementsFromPoint",
    "embeds",
    "execCommand",
    "exitFullscreen",
    "exitPointerLock",
    "firstChild",
    "firstElementChild",
    "fonts",
    "forms",
    "fullscreenElement",
    "fullscreenEnabled",
    "getElementsByClassName",
    "getElementsByName",
    "getElementsByTagName",
    "getElementsByTagNameNS",
    "getRootNode",
    "getSelection",
    "hasChildNodes",
    "hasFocus",
    "hasStorageAccess",
    "hidden",
    "images",
    "implementation",
    "importNode",
    "inputEncoding",
    "insertBefore",
    "isConnected",
    "isDefaultNamespace",
    "isEqualNode",
    "isSameNode",
    "lastChild",
    "lastElementChild",
    "links",
    "lookupNamespaceURI",
    "lookupPrefix",
    "nextSibling",
    "nodeName",
    "nodeType",
    "nodeValue",
    "normalize",
    "onDOMContentLoaded",
    "onabort",
    "onanimationcancel",
    "onanimationend",
    "onanimationiteration",
    "onanimationstart",
    "onauxclick",
    "onbeforeinput",
    "onbeforetoggle",
    "onblur",
    "oncancel",
    "oncanplay",
    "oncanplaythrough",
    "onchange",
    "onclick",
    "onclose",
    "oncontextmenu",
    "oncopy",
    "oncuechange",
    "oncut",
    "ondblclick",
    "ondrag",
    "ondragend",
    "ondragenter",
    "ondragleave",
    "ondragover",
    "ondragstart",
    "ondrop",
    "ondurationchange",
    "onemptied",
    "onended",
    "onerror",
    "onfocus",
    "onfocusin",
    "onfocusout",
    "onformdata",
    "onfullscreenchange",
    "onfullscreenerror",
    "ongotpointercapture",
    "oninput",
    "oninvalid",
    "onkeydown",
    "onkeypress",
    "onkeyup",
    "onload",
    "onloadeddata",
    "onloadedmetadata",
    "onloadstart",
    "onlostpointercapture",
    "onmousedown",
    "onmouseenter",
    "onmouseleave",
    "onmousemove",
    "onmouseout",
    "onmouseover",
    "onmouseup",
    "onmousewheel",
    "onpaste",
    "onpause",
    "onplay",
    "onplaying",
    "onpointercancel",
    "onpointerdown",
    "onpointerenter",
    "onpointerleave",
    "onpointerlockchange",
    "onpointerlockerror",
    "onpointermove",
    "onpointerout",
    "onpointerover",
    "onpointerup",
    "onprogress",
    "onratechange",
    "onreadystatechange",
    "onreset",
    "onresize",
    "onscroll",
    "onscrollend",
    "onsecuritypolicyviolation",
    "onseeked",
    "onseeking",
    "onselect",
    "onselectionchange",
    "onselectstart",
    "onslotchange",
    "onstalled",
    "onsubmit",
    "onsuspend",
    "ontimeupdate",
    "ontoggle",
    "ontouchcancel",
    "ontouchend",
    "ontouchmove",
    "ontouchstart",
    "ontransitioncancel",
    "ontransitionend",
    "ontransitionrun",
    "ontransitionstart",
    "onvisibilitychange",
    "onvolumechange",
    "onwaiting",
    "onwebkitanimationend",
    "onwebkitanimationiteration",
    "onwebkitanimationstart",
    "onwebkittransitionend",
    "onwheel",
    "open",
    "ownerDocument",
    "parentElement",
    "parentNode",
    "plugins",
    "pointerLockElement",
    "prepend",
    "previousSibling",
    "queryCommandEnabled",
    "queryCommandState",
    "queryCommandSupported",
    "queryCommandValue",
    "readyState",
    "referrer",
    "removeChild",
    "replaceChild",
    "replaceChildren",
    "requestStorageAccess",
    "scripts",
    "scrollingElement",
    "startViewTransition",
    "styleSheets",
    "textContent",
    "timeline",
    "visibilityState",
    "write",
    "writeln",
];

/// What the shim's `location` reads (it navigates nowhere).
const LOCATION: &[&str] = &[
    "href", "origin", "protocol", "host", "hostname", "port", "pathname", "search", "hash",
    "toString", "assign", "replace", "reload",
];

/// Why each module the island would run cannot run there (empty: it can).
pub fn check(src: &Source) -> Vec<Diagnostic> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &src.text, crate::source_type(&src.file)).parse();
    if !ret.diagnostics.is_empty() {
        // The bundle reports these.
        return Vec::new();
    }
    let semantic = SemanticBuilder::new().build(&ret.program).semantic;
    let scoping = semantic.scoping();
    let mut v = Check {
        src,
        scoping,
        out: Vec::new(),
        allowed: Vec::new(),
    };
    v.visit_program(&ret.program);
    v.out.sort_by_key(|d| (d.line, d.col));
    v.out.dedup_by(|a, b| a.message == b.message);
    v.out
}

struct Check<'s> {
    src: &'s Source,
    scoping: &'s Scoping,
    out: Vec<Diagnostic>,
    /// Spans of references already judged by their context (`typeof process`,
    /// `process.env.NODE_ENV`, `window.x`).
    allowed: Vec<oxc_span::Span>,
}

impl Check<'_> {
    fn refuse(&mut self, at: u32, what: String) {
        let mut d = Diagnostic::at(&self.src.text, at, what);
        d.file = self.src.file.clone();
        self.out.push(d);
    }

    fn is_free(&self, id: &IdentifierReference) -> bool {
        match id.reference_id.get() {
            Some(r) => {
                let r = self.scoping.get_reference(r);
                r.symbol_id().is_none() && r.flags().is_value()
            }
            None => false,
        }
    }

    fn free_ident<'e, 'x>(&self, e: &'e Expression<'x>) -> Option<&'e IdentifierReference<'x>> {
        match e.without_parentheses() {
            Expression::Identifier(id) if self.is_free(id) => Some(id),
            _ => None,
        }
    }
}

impl Check<'_> {
    /// Whether `e` is the page's `location` (`location`, `window.location`).
    fn is_location(&self, e: &Expression<'_>) -> bool {
        match e.without_parentheses() {
            Expression::Identifier(id) => id.name == "location" && self.is_free(id),
            Expression::StaticMemberExpression(m) => {
                m.property.name == "location"
                    && self.free_ident(&m.object).is_some_and(|w| {
                        matches!(
                            w.name.as_str(),
                            "window" | "self" | "globalThis" | "document"
                        )
                    })
            }
            _ => false,
        }
    }
}

impl<'a> Visit<'a> for Check<'_> {
    fn visit_identifier_reference(&mut self, id: &IdentifierReference<'a>) {
        if self.allowed.contains(&id.span) || !self.is_free(id) {
            return;
        }
        let name = id.name.as_str();
        // A global the page has and the island does not. (A name neither has
        // throws on both, the same.)
        let cjs = self.src.commonjs && matches!(name, "module" | "exports" | "require");
        if PAGE_GLOBALS.contains(&name) && !cjs {
            self.refuse(
                id.span.start,
                format!("`{name}` is not on the island (its React shim is no browser page)"),
            );
        }
    }

    fn visit_unary_expression(&mut self, u: &UnaryExpression<'a>) {
        if u.operator == UnaryOperator::Typeof {
            if let Some(id) = self.free_ident(&u.argument) {
                if NODE_ONLY.contains(&id.name.as_str()) {
                    self.allowed.push(id.span);
                }
            }
        }
        walk::walk_unary_expression(self, u);
    }

    fn visit_static_member_expression(&mut self, m: &StaticMemberExpression<'a>) {
        let prop = m.property.name.as_str();
        // `process.env`: what the build defines, as a bundler replaces it.
        if prop == "env" {
            if let Some(id) = self.free_ident(&m.object) {
                if id.name == "process" {
                    self.allowed.push(id.span);
                    return;
                }
            }
        }
        if self.is_location(&m.object) && !LOCATION.contains(&prop) {
            self.refuse(
                m.span.start,
                format!("`location.{prop}` is not on the island (its location only reads)"),
            );
        }
        if let Some(id) = self.free_ident(&m.object) {
            let ok = match id.name.as_str() {
                "window" | "self" | "globalThis" => !PAGE_GLOBALS.contains(&prop),
                "document" => !PAGE_DOCUMENT.contains(&prop),
                _ => true,
            };
            if !ok {
                self.refuse(
                    m.span.start,
                    format!(
                        "`{}.{prop}` is not on the island (its React shim is no browser page)",
                        id.name
                    ),
                );
            }
        }
        walk::walk_static_member_expression(self, m);
    }

    fn visit_class(&mut self, c: &Class<'a>) {
        // A class component runs over hooks (the shim's classHost): the phases
        // hooks do not have are refused, an error boundary's among them.
        let component = c.heritage.as_ref().is_some_and(|h| {
            let name = match h.expression.without_parentheses() {
                Expression::Identifier(id) => Some(id.name.as_str()),
                Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
                _ => None,
            };
            matches!(name, Some("Component" | "PureComponent"))
        });
        if component {
            for e in &c.body.body {
                if let oxc_ast::ast::ClassElement::MethodDefinition(m) = e {
                    let Some(name) = m.key.static_name() else {
                        continue;
                    };
                    if matches!(
                        &*name,
                        "getDerivedStateFromError"
                            | "componentDidCatch"
                            | "getSnapshotBeforeUpdate"
                            | "componentWillMount"
                            | "componentWillReceiveProps"
                            | "componentWillUpdate"
                    ) || name.starts_with("UNSAFE_")
                    {
                        self.refuse(
                            m.span.start,
                            format!("a class component's `{name}` on the island"),
                        );
                    }
                }
            }
        }
        walk::walk_class(self, c);
    }
}
