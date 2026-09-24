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

  // analytics/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // analytics/App.tsx
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
  var LayoutDashboard = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "7", height: "9", x: "3", y: "3", rx: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "7", height: "5", x: "14", y: "3", rx: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "7", height: "9", x: "14", y: "12", rx: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "7", height: "5", x: "3", y: "16", rx: "1" })
  ] });
  var ChartColumn = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M3 3v16a2 2 0 0 0 2 2h16" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 17V9" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M13 17V5" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M8 17v-3" })
  ] });
  var Users = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "9", cy: "7", r: "4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M22 21v-2a4 4 0 0 0-3-3.87" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M16 3.13a4 4 0 0 1 0 7.75" })
  ] });
  var ShoppingCart = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "8", cy: "21", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "19", cy: "21", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M2.05 2.05h2l2.66 12.42a2 2 0 0 0 2 1.58h9.78a2 2 0 0 0 1.95-1.57l1.65-7.43H5.12" })
  ] });
  var Settings = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "3" })
  ] });
  var Search = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "11", cy: "11", r: "8" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m21 21-4.3-4.3" })
  ] });
  var Bell = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M10.3 21a1.94 1.94 0 0 0 3.4 0" })
  ] });
  var ChevronDown = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m6 9 6 6 6-6" }) });
  var TrendingUp = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "22 7 13.5 15.5 8.5 10.5 2 17" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "16 7 22 7 22 13" })
  ] });
  var TrendingDown = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "22 17 13.5 8.5 8.5 13.5 2 7" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "16 17 22 17 22 11" })
  ] });
  var DollarSign = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "12", x2: "12", y1: "2", y2: "22" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6" })
  ] });
  var Package = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 22V12" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m3.3 7 7.703 4.734a2 2 0 0 0 1.994 0L20.7 7" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m7.5 4.27 9 5.15" })
  ] });
  var Calendar = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M8 2v4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M16 2v4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "18", height: "18", x: "3", y: "4", rx: "2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M3 10h18" })
  ] });
  var ArrowUpRight = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M7 7h10v10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M7 17 17 7" })
  ] });
  var ArrowDownRight = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m7 7 10 10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M17 7v10H7" })
  ] });
  var Ellipsis = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "19", cy: "12", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "5", cy: "12", r: "1" })
  ] });
  var Download = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "7 10 12 15 17 10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "12", x2: "12", y1: "15", y2: "3" })
  ] });
  var Check = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M20 6 9 17l-5-5" }) });
  var Folder = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" }) });
  var Sparkles = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M20 3v4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M22 5h-4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M4 17v2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 18H3" })
  ] });

  // analytics/data.ts
  var ranges = {
    "7d": {
      label: "Last 7 days",
      phrase: "this week",
      revenue: "$18,240",
      orders: "412",
      customers: "96",
      refunds: "1.8%",
      deltas: [4.2, 2.9, -3.1, -0.4],
      points: [21, 24, 22, 28, 26, 31, 34],
      labels: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    },
    "30d": {
      label: "Last 30 days",
      phrase: "this month",
      revenue: "$84,512",
      orders: "1,906",
      customers: "438",
      refunds: "2.1%",
      deltas: [12.4, 8.1, 5.6, -0.9],
      points: [42, 48, 45, 53, 61, 58, 66, 72],
      labels: ["Apr 1", "Apr 5", "Apr 9", "Apr 13", "Apr 17", "Apr 21", "Apr 25", "Apr 29"]
    },
    "90d": {
      label: "Last 90 days",
      phrase: "this quarter",
      revenue: "$241,090",
      orders: "5,771",
      customers: "1,204",
      refunds: "2.4%",
      deltas: [18.9, 15.2, -2.3, 0.6],
      points: [128, 141, 136, 155, 170, 164, 188, 203, 196, 221, 236, 249],
      labels: ["W1", "W2", "W3", "W4", "W5", "W6", "W7", "W8", "W9", "W10", "W11", "W12"]
    }
  };
  var orders = [
    { id: 3210, customer: "Olivia Martin", email: "olivia@example.com", avatar: "bg-violet-500", date: "Apr 29, 2024", status: "Paid", amount: 1999 },
    { id: 3209, customer: "Jackson Lee", email: "jackson@example.com", avatar: "bg-sky-500", date: "Apr 28, 2024", status: "Pending", amount: 39 },
    { id: 3208, customer: "Isabella Nguyen", email: "isabella@example.com", avatar: "bg-emerald-500", date: "Apr 28, 2024", status: "Paid", amount: 299 },
    { id: 3207, customer: "William Kim", email: "will@example.com", avatar: "bg-amber-500", date: "Apr 27, 2024", status: "Refunded", amount: 99 },
    { id: 3206, customer: "Sofia Davis", email: "sofia@example.com", avatar: "bg-rose-500", date: "Apr 26, 2024", status: "Failed", amount: 450.5 }
  ];
  var channels = [
    { name: "Direct", value: 42, color: "#6366f1" },
    { name: "Search", value: 28, color: "#0ea5e9" },
    { name: "Social", value: 18, color: "#10b981" },
    { name: "Email", value: 12, color: "#f59e0b" }
  ];
  var goals = [
    { name: "Monthly revenue", current: 84, target: 100, color: "bg-indigo-500" },
    { name: "New customers", current: 438, target: 500, color: "bg-emerald-500" },
    { name: "Average order value", current: 44, target: 40, color: "bg-sky-500" },
    { name: "Support tickets closed", current: 172, target: 240, color: "bg-amber-500" }
  ];

  // analytics/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  var nav = [
    { label: "Dashboard", icon: LayoutDashboard, active: true },
    { label: "Reports", icon: ChartColumn },
    { label: "Customers", icon: Users },
    { label: "Orders", icon: ShoppingCart, count: 12 },
    { label: "Products", icon: Package },
    { label: "Settings", icon: Settings }
  ];
  var teams = [
    { name: "Growth", color: "bg-indigo-500" },
    { name: "Retention", color: "bg-emerald-500" },
    { name: "Partnerships", color: "bg-amber-500" }
  ];
  function Sidebar() {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("aside", { className: "fixed inset-y-0 left-0 flex w-64 flex-col border-r border-gray-200 bg-white", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex h-16 items-center gap-2 px-6", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-indigo-500 to-violet-600 text-white shadow-sm", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Sparkles, { className: "h-4 w-4" }) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-lg font-semibold tracking-tight text-gray-900", children: "Lumen" })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("nav", { className: "flex-1 space-y-1 px-3 py-4", children: [
        nav.map(({ label, icon: Icon2, active, count }) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
          "a",
          {
            href: "#",
            className: `group flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${active ? "bg-indigo-50 text-indigo-700" : "text-gray-600 hover:bg-gray-50 hover:text-gray-900"}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { className: `h-5 w-5 ${active ? "text-indigo-600" : "text-gray-400 group-hover:text-gray-500"}` }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "flex-1", children: label }),
              count !== void 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600", children: count })
            ]
          },
          label
        )),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "pt-6", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "px-3 text-xs font-semibold uppercase tracking-wider text-gray-400", children: "Teams" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-2 space-y-1", children: teams.map((t) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("a", { href: "#", className: "flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-gray-600 hover:bg-gray-50", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `h-2 w-2 rounded-full ${t.color}` }),
            t.name
          ] }, t.name)) })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "border-t border-gray-200 p-4", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex h-9 w-9 items-center justify-center rounded-full bg-gradient-to-br from-pink-500 to-orange-400 text-sm font-semibold text-white", children: "AR" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-sm font-medium text-gray-900", children: "Ava Reyes" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-xs text-gray-500", children: "ava@lumen.app" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Folder, { className: "h-4 w-4 text-gray-400" })
      ] }) })
    ] });
  }
  function Header() {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("header", { className: "sticky top-0 z-10 flex h-16 items-center gap-4 border-b border-gray-200 bg-white/80 px-8 backdrop-blur", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative w-full max-w-md", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
          "input",
          {
            type: "search",
            placeholder: "Search orders, customers…",
            className: "w-full rounded-lg border border-gray-200 bg-gray-50 py-2 pl-9 pr-3 text-sm text-gray-900 placeholder:text-gray-400 focus:border-indigo-500 focus:bg-white focus:outline-none focus:ring-2 focus:ring-indigo-500/20"
          }
        )
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "ml-auto flex items-center gap-3", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "relative rounded-full p-2 text-gray-500 hover:bg-gray-100", "aria-label": "Notifications", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Bell, { className: "h-5 w-5" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-rose-500 ring-2 ring-white" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "h-8 w-px bg-gray-200" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-br from-pink-500 to-orange-400 text-xs font-semibold text-white", children: "AR" })
      ] })
    ] });
  }
  function RangePicker({ value, onChange }) {
    const [open, setOpen] = (0, import_react.useState)(false);
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
        "button",
        {
          "aria-label": "Date range",
          onClick: () => setOpen((o) => !o),
          className: "inline-flex items-center gap-2 rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50",
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Calendar, { className: "h-4 w-4 text-gray-400" }),
            ranges[value].label,
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronDown, { className: `h-4 w-4 text-gray-400 transition-transform ${open ? "rotate-180" : ""}` })
          ]
        }
      ),
      open && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "absolute right-0 z-20 mt-2 w-52 origin-top-right rounded-lg bg-white p-1 shadow-lg ring-1 ring-black/5", children: Object.keys(ranges).map((key) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
        "button",
        {
          "data-range": key,
          onClick: () => {
            onChange(key);
            setOpen(false);
          },
          className: `flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm ${key === value ? "bg-indigo-50 text-indigo-700" : "text-gray-700 hover:bg-gray-50"}`,
          children: [
            ranges[key].label,
            key === value && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Check, { className: "h-4 w-4" })
          ]
        },
        key
      )) })
    ] });
  }
  function Kpi({ label, value, delta, icon: Icon2, tint }) {
    const up = delta >= 0;
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rounded-xl border border-gray-200 bg-white p-5 shadow-sm", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `flex h-10 w-10 items-center justify-center rounded-lg ${tint}`, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { className: "h-5 w-5" }) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
          "span",
          {
            className: `inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ${up ? "bg-emerald-50 text-emerald-700" : "bg-rose-50 text-rose-700"}`,
            children: [
              up ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(TrendingUp, { className: "h-3 w-3" }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(TrendingDown, { className: "h-3 w-3" }),
              up ? "+" : "",
              delta.toFixed(1),
              "%"
            ]
          }
        )
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-4 text-sm text-gray-500", children: label }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-2xl font-semibold tracking-tight text-gray-900", children: value })
    ] });
  }
  var W = 640;
  var H = 240;
  var PAD = { top: 16, right: 16, bottom: 28, left: 44 };
  function RevenueChart({ points, labels }) {
    const max = Math.ceil(Math.max(...points) / 10) * 10;
    const innerW = W - PAD.left - PAD.right;
    const innerH = H - PAD.top - PAD.bottom;
    const x = (i) => PAD.left + i * innerW / (points.length - 1);
    const y = (v) => PAD.top + innerH - v / max * innerH;
    const line = points.map((v, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
    const area = `${line} L${x(points.length - 1).toFixed(1)},${PAD.top + innerH} L${PAD.left},${PAD.top + innerH} Z`;
    const ticks = [0, 0.25, 0.5, 0.75, 1].map((t) => Math.round(max * t));
    const last = points.length - 1;
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("svg", { viewBox: `0 0 ${W} ${H}`, className: "h-60 w-full", role: "img", "aria-label": "Revenue over time", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("defs", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("linearGradient", { id: "revenue-fill", x1: "0", y1: "0", x2: "0", y2: "1", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("stop", { offset: "0%", stopColor: "#6366f1", stopOpacity: 0.25 }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("stop", { offset: "100%", stopColor: "#6366f1", stopOpacity: 0 })
      ] }) }),
      ticks.map((t) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("g", { children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("line", { x1: PAD.left, x2: W - PAD.right, y1: y(t), y2: y(t), stroke: "#e5e7eb", strokeDasharray: "4 4" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("text", { x: PAD.left - 8, y: y(t) + 4, textAnchor: "end", fontSize: "11", fill: "#6b7280", children: [
          "$",
          t,
          "k"
        ] })
      ] }, t)),
      labels.map((l, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("text", { x: x(i), y: H - 8, textAnchor: "middle", fontSize: "11", fill: "#6b7280", children: l }, l)),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: area, fill: "url(#revenue-fill)" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: line, fill: "none", stroke: "#6366f1", strokeWidth: 2.5, strokeLinejoin: "round", strokeLinecap: "round" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: x(last), cy: y(points[last]), r: 5, fill: "#fff", stroke: "#6366f1", strokeWidth: 2.5 })
    ] });
  }
  function ChannelBars() {
    const max = Math.max(...channels.map((c) => c.value));
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("svg", { viewBox: "0 0 280 200", className: "h-52 w-full", role: "img", "aria-label": "Sales by channel", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("line", { x1: "0", x2: "280", y1: "170", y2: "170", stroke: "#e5e7eb" }),
      channels.map((c, i) => {
        const h = c.value / max * 140;
        const bx = 16 + i * 66;
        return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("g", { children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { x: bx, y: 170 - h, width: "36", height: h, rx: "6", fill: c.color }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("text", { x: bx + 18, y: 162 - h, textAnchor: "middle", fontSize: "11", fontWeight: "600", fill: "#374151", children: [
            c.value,
            "%"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("text", { x: bx + 18, y: "188", textAnchor: "middle", fontSize: "11", fill: "#6b7280", children: c.name })
        ] }, c.name);
      })
    ] });
  }
  var statusStyles = {
    Paid: "bg-emerald-50 text-emerald-700 ring-emerald-600/20",
    Pending: "bg-amber-50 text-amber-700 ring-amber-600/20",
    Refunded: "bg-gray-50 text-gray-600 ring-gray-500/20",
    Failed: "bg-rose-50 text-rose-700 ring-rose-600/20"
  };
  function OrdersTable() {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "overflow-hidden rounded-xl border border-gray-200 bg-white shadow-sm", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between px-5 py-4", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-gray-900", children: "Recent orders" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { className: "rounded-md p-1 text-gray-400 hover:bg-gray-100", "aria-label": "More", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Ellipsis, { className: "h-5 w-5" }) })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("table", { className: "min-w-full divide-y divide-gray-200 text-sm", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("thead", { className: "bg-gray-50", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("tr", { children: ["Order", "Customer", "Date", "Status", "Amount"].map((h) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
          "th",
          {
            className: `px-5 py-3 text-xs font-medium uppercase tracking-wide text-gray-500 ${h === "Amount" ? "text-right" : "text-left"}`,
            children: h
          },
          h
        )) }) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("tbody", { className: "divide-y divide-gray-100", children: orders.map((o) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { className: "hover:bg-gray-50", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("td", { className: "whitespace-nowrap px-5 py-3 font-medium text-gray-900", children: [
            "#",
            o.id
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "px-5 py-3", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `flex h-8 w-8 items-center justify-center rounded-full text-xs font-semibold text-white ${o.avatar}`, children: o.customer.split(" ").map((p) => p[0]).join("") }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "font-medium text-gray-900", children: o.customer }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-xs text-gray-500", children: o.email })
            ] })
          ] }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "whitespace-nowrap px-5 py-3 text-gray-500", children: o.date }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "px-5 py-3", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ring-1 ring-inset ${statusStyles[o.status]}`, children: o.status }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("td", { className: "whitespace-nowrap px-5 py-3 text-right font-medium tabular-nums text-gray-900", children: [
            "$",
            o.amount.toFixed(2)
          ] })
        ] }, o.id)) })
      ] })
    ] });
  }
  function Donut({ percent }) {
    const r = 52;
    const c = 2 * Math.PI * r;
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("svg", { viewBox: "0 0 128 128", className: "h-32 w-32 -rotate-90", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "64", cy: "64", r, fill: "none", stroke: "#eef2ff", strokeWidth: "12" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
        "circle",
        {
          cx: "64",
          cy: "64",
          r,
          fill: "none",
          stroke: "#6366f1",
          strokeWidth: "12",
          strokeLinecap: "round",
          strokeDasharray: `${(percent / 100 * c).toFixed(1)} ${c.toFixed(1)}`
        }
      )
    ] });
  }
  function Reports() {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "grid grid-cols-3 gap-6", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "col-span-2 rounded-xl border border-gray-200 bg-white p-6 shadow-sm", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-gray-900", children: "Quarterly goals" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Progress toward the targets set in January." }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "mt-6 space-y-5", children: goals.map((g) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between text-sm", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "font-medium text-gray-700", children: g.name }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "tabular-nums text-gray-500", children: [
              g.current,
              " / ",
              g.target
            ] })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-2 h-2 overflow-hidden rounded-full bg-gray-100", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `h-full rounded-full ${g.color}`, style: { width: `${Math.min(100, g.current / g.target * 100)}%` } }) })
        ] }, g.name)) })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex flex-col items-center rounded-xl border border-gray-200 bg-white p-6 shadow-sm", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "self-start text-base font-semibold text-gray-900", children: "Retention" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative mt-6", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Donut, { percent: 72 }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "absolute inset-0 flex flex-col items-center justify-center", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-2xl font-semibold text-gray-900", children: "72%" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-xs text-gray-500", children: "30-day" })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "mt-6 text-center text-sm text-gray-500", children: [
          "Up ",
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "font-medium text-emerald-600", children: "4.1 points" }),
          " since last quarter."
        ] })
      ] })
    ] });
  }
  var tabs = ["Overview", "Reports", "Customers"];
  function App() {
    const [range, setRange] = (0, import_react.useState)("30d");
    const [tab, setTab] = (0, import_react.useState)("Overview");
    const data = ranges[range];
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-h-screen bg-gray-50 font-sans text-gray-900 antialiased", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Sidebar, {}),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "pl-64", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Header, {}),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("main", { className: "px-8 py-8", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-end justify-between", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { className: "text-2xl font-semibold tracking-tight text-gray-900", children: "Overview" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "mt-1 text-sm text-gray-500", children: [
                "Here's what happened with your store ",
                data.phrase,
                "."
              ] })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(RangePicker, { value: range, onChange: setRange }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "inline-flex items-center gap-2 rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Download, { className: "h-4 w-4" }),
                "Export"
              ] })
            ] })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-6 border-b border-gray-200", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("nav", { className: "-mb-px flex gap-6", children: tabs.map((t) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "button",
            {
              "data-tab": t,
              onClick: () => setTab(t),
              className: `border-b-2 px-1 pb-3 text-sm font-medium ${tab === t ? "border-indigo-600 text-indigo-600" : "border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700"}`,
              children: t
            },
            t
          )) }) }),
          tab === "Overview" && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 grid grid-cols-4 gap-6", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Kpi, { label: "Revenue", value: data.revenue, delta: data.deltas[0], icon: DollarSign, tint: "bg-indigo-50 text-indigo-600" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Kpi, { label: "Orders", value: data.orders, delta: data.deltas[1], icon: ShoppingCart, tint: "bg-sky-50 text-sky-600" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Kpi, { label: "New customers", value: data.customers, delta: data.deltas[2], icon: Users, tint: "bg-emerald-50 text-emerald-600" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Kpi, { label: "Refund rate", value: data.refunds, delta: data.deltas[3], icon: Package, tint: "bg-amber-50 text-amber-600" })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 grid grid-cols-3 gap-6", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "col-span-2 rounded-xl border border-gray-200 bg-white p-6 shadow-sm", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-start justify-between", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-gray-900", children: "Revenue" }),
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "mt-1 flex items-center gap-1 text-sm text-gray-500", children: [
                      data.deltas[0] >= 0 ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ArrowUpRight, { className: "h-4 w-4 text-emerald-500" }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ArrowDownRight, { className: "h-4 w-4 text-rose-500" }),
                      data.revenue,
                      " ",
                      data.phrase
                    ] })
                  ] }),
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-2 text-xs text-gray-500", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "h-2 w-2 rounded-full bg-indigo-500" }),
                    "Net sales"
                  ] })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-4", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(RevenueChart, { points: data.points, labels: data.labels }) })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rounded-xl border border-gray-200 bg-white p-6 shadow-sm", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-gray-900", children: "Sales by channel" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Share of orders" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-4", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChannelBars, {}) })
              ] })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-6", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(OrdersTable, {}) })
          ] }),
          tab === "Reports" && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-6", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Reports, {}) }),
          tab === "Customers" && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 rounded-xl border border-dashed border-gray-300 bg-white p-12 text-center", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Users, { className: "mx-auto h-10 w-10 text-gray-300" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "mt-3 text-sm font-semibold text-gray-900", children: "No segments yet" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Create a segment to group customers by behaviour." })
          ] })
        ] })
      ] })
    ] });
  }

  // analytics/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
