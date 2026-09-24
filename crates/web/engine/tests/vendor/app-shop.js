"use strict";
(() => {
  var __create = Object.create;
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __getProtoOf = Object.getPrototypeOf;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __commonJS = (cb, mod) => function __require() {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
    // If the importer is in node compatibility mode or this is not an ESM
    // file that has been converted to a CommonJS file using a Babel-
    // compatible transform (i.e. "__esModule" has not been set), then set
    // "default" to the CommonJS "module.exports" for node compatibility.
    isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
    mod
  ));

  // shims/react/index.js
  var require_react = __commonJS({
    "shims/react/index.js"(exports, module) {
      "use strict";
      module.exports = window.React;
    }
  });

  // shims/react-dom/client.js
  var require_client = __commonJS({
    "shims/react-dom/client.js"(exports, module) {
      "use strict";
      module.exports = window.ReactDOM;
    }
  });

  // shims/react/jsx-runtime.js
  var require_jsx_runtime = __commonJS({
    "shims/react/jsx-runtime.js"(exports) {
      "use strict";
      var React = window.React;
      function jsx4(type, props, key) {
        return React.createElement(type, key === void 0 ? props : { ...props, key });
      }
      exports.jsx = jsx4;
      exports.jsxs = jsx4;
      exports.Fragment = React.Fragment;
    }
  });

  // shop/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // shop/App.tsx
  var import_react = __toESM(require_react());

  // shared/icons.tsx
  var import_jsx_runtime = __toESM(require_jsx_runtime());
  function Icon({ className, children }) {
    return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
      "svg",
      {
        xmlns: "http://www.w3.org/2000/svg",
        width: "24",
        height: "24",
        viewBox: "0 0 24 24",
        fill: "none",
        stroke: "currentColor",
        strokeWidth: 2,
        strokeLinecap: "round",
        strokeLinejoin: "round",
        className,
        "aria-hidden": "true",
        children
      }
    );
  }
  var Search = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "11", cy: "11", r: "8" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m21 21-4.3-4.3" })
  ] });
  var ChevronDown = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m6 9 6 6 6-6" }) });
  var ChevronRight = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 18 6-6-6-6" }) });
  var Check = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M20 6 9 17l-5-5" }) });
  var Plus = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 5v14" })
  ] });
  var X = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 6 6 18" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m6 6 12 12" })
  ] });
  var ShoppingBag = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M6 2 3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4Z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M3 6h18" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M16 10a4 4 0 0 1-8 0" })
  ] });
  var Heart = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z" }) });
  var Minus = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }) });
  var Truck = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M14 18V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v11a1 1 0 0 0 1 1h2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M15 18H9" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M19 18h2a1 1 0 0 0 1-1v-3.65a1 1 0 0 0-.22-.624l-3.48-4.35A1 1 0 0 0 17.52 8H14" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "17", cy: "18", r: "2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "7", cy: "18", r: "2" })
  ] });
  var ShieldCheck = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 12 2 2 4-4" })
  ] });
  var RotateCcw = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M3 3v5h5" })
  ] });
  var Menu = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "4", x2: "20", y1: "12", y2: "12" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "4", x2: "20", y1: "6", y2: "6" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "4", x2: "20", y1: "18", y2: "18" })
  ] });

  // shop/data.ts
  var product = {
    name: "Ridgeline Weekender",
    price: 148,
    was: 185,
    rating: 4.4,
    reviews: 1284,
    breadcrumbs: ["Home", "Bags", "Travel"],
    description: "A roomy carry-on in waxed canvas and leather that fits a long weekend and still slides under the seat in front of you. Structured base, soft sides, and a luggage sleeve that clips over a trolley handle.",
    colours: [
      { id: "ochre", name: "Ochre", hex: "#c98a2b", shade: "#7a4f12", backdrop: "#f7efe2" },
      { id: "sage", name: "Sage", hex: "#7f9c7a", shade: "#3f5a3b", backdrop: "#eef3ec" },
      { id: "navy", name: "Navy", hex: "#2f3e66", shade: "#151d36", backdrop: "#e8ebf3" },
      { id: "clay", name: "Clay", hex: "#b5654a", shade: "#6b2f1d", backdrop: "#f6e9e4" }
    ],
    sizes: [
      { label: "S", inStock: true },
      { label: "M", inStock: true },
      { label: "L", inStock: true },
      { label: "XL", inStock: false }
    ]
  };

  // shop/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  var money = (n) => `$${n.toFixed(2)}`;
  function Stars({ rating }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex items-center", children: [0, 1, 2, 3, 4].map((i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("svg", { viewBox: "0 0 20 20", className: `h-5 w-5 ${i < Math.round(rating) ? "text-amber-400" : "text-gray-200"}`, fill: "currentColor", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M10.868 2.884c-.321-.772-1.415-.772-1.736 0l-1.83 4.401-4.753.381c-.833.067-1.171 1.107-.536 1.651l3.62 3.102-1.106 4.637c-.194.813.691 1.456 1.405 1.02L10 15.591l4.069 2.485c.713.436 1.598-.207 1.404-1.02l-1.106-4.637 3.62-3.102c.635-.544.297-1.584-.536-1.65l-4.752-.382-1.831-4.401z" }) }, i)) });
  }
  function ProductArt({ colour, className }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("svg", { viewBox: "0 0 200 200", className, "aria-hidden": "true", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("defs", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("radialGradient", { id: `glow-${colour.id}`, cx: "50%", cy: "40%", r: "60%", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("stop", { offset: "0%", stopColor: "#ffffff", stopOpacity: 0.9 }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("stop", { offset: "100%", stopColor: "#ffffff", stopOpacity: 0 })
      ] }) }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { width: "200", height: "200", fill: colour.backdrop }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "100", cy: "90", r: "80", fill: `url(#glow-${colour.id})` }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M70 70 C70 40 130 40 130 70", fill: "none", stroke: colour.shade, strokeWidth: "8", strokeLinecap: "round" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { x: "45", y: "70", width: "110", height: "95", rx: "14", fill: colour.hex }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { x: "45", y: "70", width: "110", height: "22", rx: "10", fill: colour.shade, opacity: 0.35 }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { x: "88", y: "100", width: "24", height: "16", rx: "4", fill: "#ffffff", opacity: 0.85 }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ellipse", { cx: "100", cy: "178", rx: "62", ry: "6", fill: "#000000", opacity: 0.08 })
    ] });
  }
  function Accordion({ title, children, defaultOpen = false }) {
    const [open, setOpen] = (0, import_react.useState)(defaultOpen);
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "border-b border-gray-200", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { onClick: () => setOpen(!open), "data-accordion": title, className: "flex w-full items-center justify-between py-4 text-left text-sm font-medium text-gray-900", children: [
        title,
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronDown, { className: `h-5 w-5 text-gray-400 transition-transform duration-200 ${open ? "rotate-180" : ""}` })
      ] }),
      open && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "pb-4 text-sm leading-6 text-gray-600", children })
    ] });
  }
  function CartDrawer({ lines, onClose, onQty }) {
    const subtotal = lines.reduce((s, l) => s + l.price * l.qty, 0);
    const shipping = subtotal >= 150 ? 0 : 9;
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "fixed inset-0 z-40", role: "dialog", "aria-modal": "true", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "absolute inset-0 bg-black/30 transition-opacity", onClick: onClose }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("aside", { className: "absolute inset-y-0 right-0 flex w-full max-w-md flex-col bg-white shadow-xl", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between border-b border-gray-200 px-6 py-5", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-lg font-semibold text-gray-900", children: "Your cart" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { onClick: onClose, className: "-m-2 rounded-md p-2 text-gray-400 hover:text-gray-500", "aria-label": "Close cart", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(X, { className: "h-6 w-6" }) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "flex-1 divide-y divide-gray-200 overflow-y-auto px-6", children: lines.map((l) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { className: "flex gap-4 py-6", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ProductArt, { colour: l.colour, className: "h-24 w-24 shrink-0 rounded-md border border-gray-200" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex flex-1 flex-col", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex justify-between text-sm font-medium text-gray-900", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h3", { children: l.name }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "ml-4 tabular-nums", children: money(l.price * l.qty) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "mt-1 text-sm text-gray-500", children: [
              l.colour.name,
              " · ",
              l.size
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-auto flex items-center justify-between", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center rounded-md border border-gray-300", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { "data-qty": "minus", onClick: () => onQty(l.id, -1), className: "p-1.5 text-gray-500 hover:text-gray-700", "aria-label": "Decrease", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Minus, { className: "h-4 w-4" }) }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "w-8 text-center text-sm tabular-nums", children: l.qty }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { "data-qty": "plus", onClick: () => onQty(l.id, 1), className: "p-1.5 text-gray-500 hover:text-gray-700", "aria-label": "Increase", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Plus, { className: "h-4 w-4" }) })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { className: "text-sm font-medium text-indigo-600 hover:text-indigo-500", children: "Remove" })
            ] })
          ] })
        ] }, l.id)) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "border-t border-gray-200 px-6 py-6", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dl", { className: "space-y-2 text-sm", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex justify-between text-gray-600", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Subtotal" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dd", { className: "tabular-nums", children: money(subtotal) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex justify-between text-gray-600", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Shipping" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dd", { className: "tabular-nums", children: shipping === 0 ? "Free" : money(shipping) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex justify-between border-t border-gray-200 pt-2 text-base font-medium text-gray-900", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Total" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dd", { className: "tabular-nums", children: money(subtotal + shipping) })
            ] })
          ] }),
          shipping > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "mt-2 text-xs text-gray-500", children: [
            "Add ",
            money(150 - subtotal),
            " more for free shipping."
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { className: "mt-6 w-full rounded-lg bg-indigo-600 px-6 py-3 text-base font-medium text-white shadow-sm hover:bg-indigo-700", children: "Checkout" })
        ] })
      ] })
    ] });
  }
  function App() {
    const [colour, setColour] = (0, import_react.useState)(product.colours[0]);
    const [size, setSize] = (0, import_react.useState)(null);
    const [sizeError, setSizeError] = (0, import_react.useState)(false);
    const [liked, setLiked] = (0, import_react.useState)(false);
    const [cart, setCart] = (0, import_react.useState)([]);
    const [cartOpen, setCartOpen] = (0, import_react.useState)(false);
    const count = cart.reduce((n, l) => n + l.qty, 0);
    function add() {
      if (!size) {
        setSizeError(true);
        return;
      }
      const id = `${colour.id}-${size}`;
      setCart(
        (c) => c.some((l) => l.id === id) ? c.map((l) => l.id === id ? { ...l, qty: l.qty + 1 } : l) : [...c, { id, name: product.name, colour, size, price: product.price, qty: 1 }]
      );
      setCartOpen(true);
    }
    function changeQty(id, delta) {
      setCart((c) => c.map((l) => l.id === id ? { ...l, qty: Math.max(1, l.qty + delta) } : l));
    }
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "bg-white font-sans text-gray-900", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "bg-gradient-to-r from-indigo-600 via-purple-600 to-pink-500 px-4 py-2 text-center text-sm font-medium text-white", children: "Free shipping on orders over $150 — this week only" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("header", { className: "sticky top-0 z-30 border-b border-gray-200 bg-white/90 backdrop-blur", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mx-auto flex h-16 max-w-7xl items-center gap-8 px-8", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Menu, { className: "h-6 w-6 text-gray-400" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "text-xl font-bold tracking-tight", children: [
          "trail",
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-indigo-600", children: "&" }),
          "co"
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("nav", { className: "flex gap-6 text-sm font-medium text-gray-700", children: ["Women", "Men", "Bags", "Journal"].map((n) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("a", { href: "#", className: `hover:text-gray-900 ${n === "Bags" ? "text-indigo-600" : ""}`, children: n }, n)) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "ml-auto flex items-center gap-5 text-gray-400", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "h-5 w-5" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { id: "cart", onClick: () => setCartOpen(true), className: "group relative flex items-center gap-1.5 text-gray-700", "aria-label": "Open cart", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ShoppingBag, { className: "h-6 w-6 text-gray-400 group-hover:text-gray-500" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-sm font-medium", children: count }),
            count > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute -right-1 -top-1 h-2.5 w-2.5 rounded-full bg-pink-500 ring-2 ring-white" })
          ] })
        ] })
      ] }) }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("main", { className: "mx-auto max-w-7xl px-8 pb-24 pt-6", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("nav", { "aria-label": "Breadcrumb", className: "flex items-center gap-1 text-sm text-gray-500", children: [
          product.breadcrumbs.map((b) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "flex items-center gap-1", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("a", { href: "#", className: "hover:text-gray-700", children: b }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronRight, { className: "h-4 w-4 text-gray-300" })
          ] }, b)),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "font-medium text-gray-900", children: product.name })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 grid grid-cols-2 gap-12", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative overflow-hidden rounded-2xl bg-gray-100", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ProductArt, { colour, className: "aspect-square w-full" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute left-4 top-4 rounded-full bg-white/90 px-3 py-1 text-xs font-semibold text-gray-900 shadow-sm", children: "New season" })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-4 grid grid-cols-4 gap-4", children: product.colours.map((c) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              "button",
              {
                onClick: () => setColour(c),
                className: `overflow-hidden rounded-lg ring-2 ring-offset-2 ${c.id === colour.id ? "ring-indigo-500" : "ring-transparent"}`,
                "aria-label": `Show ${c.name}`,
                children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ProductArt, { colour: c, className: "aspect-square w-full" })
              },
              c.id
            )) })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { className: "text-3xl font-bold tracking-tight text-gray-900", children: product.name }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-3 flex items-center gap-4", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-3xl tracking-tight text-gray-900", children: money(product.price) }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rounded-md bg-rose-50 px-2 py-1 text-xs font-semibold text-rose-600", children: "-20%" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-lg text-gray-400 line-through", children: money(product.was) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-3 flex items-center gap-2", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Stars, { rating: product.rating }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("a", { href: "#", className: "text-sm font-medium text-indigo-600 hover:text-indigo-500", children: [
                product.reviews,
                " reviews"
              ] })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-6 text-base leading-7 text-gray-600", children: product.description }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-8", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("h2", { className: "text-sm font-medium text-gray-900", children: [
                "Colour ",
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "font-normal text-gray-500", children: [
                  "— ",
                  colour.name
                ] })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-3 flex gap-3", children: product.colours.map((c) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                "button",
                {
                  "data-colour": c.id,
                  onClick: () => setColour(c),
                  "aria-label": c.name,
                  className: `relative flex h-9 w-9 items-center justify-center rounded-full ring-offset-2 ${c.id === colour.id ? "ring-2 ring-gray-900" : "ring-1 ring-black/10"}`,
                  style: { backgroundColor: c.hex },
                  children: c.id === colour.id && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Check, { className: "h-4 w-4 text-white" })
                },
                c.id
              )) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-8", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-sm font-medium text-gray-900", children: "Size" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("a", { href: "#", className: "text-sm font-medium text-indigo-600 hover:text-indigo-500", children: "Size guide" })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-3 grid grid-cols-4 gap-3", children: product.sizes.map((s) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                "button",
                {
                  "data-size": s.label,
                  disabled: !s.inStock,
                  onClick: () => {
                    setSize(s.label);
                    setSizeError(false);
                  },
                  className: `rounded-md border px-4 py-3 text-sm font-medium uppercase ${!s.inStock ? "cursor-not-allowed border-gray-200 bg-gray-50 text-gray-300 line-through" : size === s.label ? "border-transparent bg-indigo-600 text-white" : "border-gray-200 bg-white text-gray-900 hover:bg-gray-50"}`,
                  children: s.label
                },
                s.label
              )) }),
              sizeError && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-2 text-sm text-rose-600", children: "Please choose a size." })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-8 flex gap-3", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { id: "add", onClick: add, className: "flex flex-1 items-center justify-center gap-2 rounded-lg bg-indigo-600 px-8 py-3 text-base font-medium text-white shadow-sm hover:bg-indigo-700", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ShoppingBag, { className: "h-5 w-5" }),
                "Add to bag"
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                "button",
                {
                  onClick: () => setLiked(!liked),
                  className: `rounded-lg border px-3 py-3 ${liked ? "border-rose-200 bg-rose-50 text-rose-500" : "border-gray-200 text-gray-400 hover:bg-gray-50"}`,
                  "aria-label": "Save",
                  children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Heart, { className: "h-6 w-6" })
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "mt-8 grid grid-cols-3 gap-4 text-center text-xs text-gray-600", children: [
              { icon: Truck, text: "Free delivery over $150" },
              { icon: RotateCcw, text: "60-day returns" },
              { icon: ShieldCheck, text: "5-year warranty" }
            ].map(({ icon: Icon2, text }) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { className: "rounded-lg bg-gray-50 px-3 py-4", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { className: "mx-auto h-6 w-6 text-gray-400" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-2", children: text })
            ] }, text)) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-8 border-t border-gray-200", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Accordion, { title: "Features", defaultOpen: true, children: "Water-resistant waxed canvas, full-grain leather base, padded 16-inch laptop sleeve, two hidden quick-access pockets." }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Accordion, { title: "Care", children: "Brush off dry dirt, spot clean with a damp cloth and re-wax once a year." }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Accordion, { title: "Shipping", children: "Ships in 1-2 business days from our Portland warehouse." })
            ] })
          ] })
        ] })
      ] }),
      cartOpen && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CartDrawer, { lines: cart, onClose: () => setCartOpen(false), onQty: changeQty })
    ] });
  }

  // shop/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
