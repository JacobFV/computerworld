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

  // datatable/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // datatable/App.tsx
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
  var ChevronUp = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m18 15-6-6-6 6" }) });
  var ChevronLeft = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m15 18-6-6 6-6" }) });
  var ChevronRight = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 18 6-6-6-6" }) });
  var ChevronsUpDown = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m7 15 5 5 5-5" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m7 9 5-5 5 5" })
  ] });
  var Download = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "7 10 12 15 17 10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "12", x2: "12", y1: "15", y2: "3" })
  ] });
  var Plus = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 5v14" })
  ] });
  var Mail = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "20", height: "16", x: "2", y: "4", rx: "2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7" })
  ] });

  // datatable/data.ts
  var tones = ["bg-indigo-100 text-indigo-700", "bg-amber-100 text-amber-700", "bg-emerald-100 text-emerald-700", "bg-rose-100 text-rose-700", "bg-sky-100 text-sky-700"];
  var rows = [
    ["Lindsay Walton", "Owner", "Active", "Just now", 14],
    ["Courtney Henry", "Admin", "Active", "3 minutes ago", 9],
    ["Tom Cook", "Editor", "Active", "1 hour ago", 6],
    ["Whitney Francis", "Editor", "Invited", "Never", 0],
    ["Leonard Krasner", "Viewer", "Active", "2 hours ago", 2],
    ["Floyd Miles", "Editor", "Suspended", "3 weeks ago", 4],
    ["Emily Selman", "Admin", "Active", "Yesterday", 11],
    ["Kristin Watson", "Viewer", "Active", "4 days ago", 1],
    ["Emma Dorsey", "Editor", "Active", "5 hours ago", 7],
    ["Alicia Bell", "Viewer", "Invited", "Never", 0],
    ["Jenny Wilson", "Editor", "Active", "12 minutes ago", 5],
    ["Anna Roberts", "Viewer", "Active", "Last week", 3],
    ["Benjamin Russel", "Editor", "Suspended", "2 months ago", 8],
    ["Dries Vincent", "Admin", "Active", "6 hours ago", 12],
    ["Hector Gibbons", "Viewer", "Active", "Yesterday", 2],
    ["Jeffrey Webb", "Editor", "Invited", "Never", 0],
    ["Michael Foster", "Editor", "Active", "20 minutes ago", 10],
    ["Rebecca Nguyen", "Viewer", "Active", "3 days ago", 1],
    ["Sofia Mendes", "Editor", "Active", "9 hours ago", 6],
    ["Victor Allen", "Viewer", "Suspended", "5 weeks ago", 2],
    ["Wade Cooper", "Editor", "Active", "2 days ago", 5],
    ["Yusuf Hassan", "Viewer", "Invited", "Never", 0]
  ];
  var members = rows.map(([name, role, status, lastActive, seats], i) => ({
    id: i + 1,
    name,
    email: `${name.split(" ")[0].toLowerCase()}@northwind.io`,
    role,
    status,
    lastActive,
    seats,
    tone: tones[i % tones.length]
  }));

  // datatable/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  var PAGE_SIZE = 8;
  var statusBadge = {
    Active: "bg-green-50 text-green-700 ring-green-600/20",
    Invited: "bg-blue-50 text-blue-700 ring-blue-700/10",
    Suspended: "bg-red-50 text-red-700 ring-red-600/10"
  };
  var statusDot = {
    Active: "bg-green-500",
    Invited: "bg-blue-500",
    Suspended: "bg-red-500"
  };
  var roleBadge = {
    Owner: "bg-purple-100 text-purple-800",
    Admin: "bg-indigo-100 text-indigo-800",
    Editor: "bg-gray-100 text-gray-800",
    Viewer: "bg-gray-50 text-gray-600"
  };
  var columns = [
    { key: "name", label: "Name" },
    { key: "role", label: "Role" },
    { key: "status", label: "Status" },
    { key: "lastActive", label: "Last active" },
    { key: "seats", label: "Projects", className: "text-right" }
  ];
  function compare(a, b, key) {
    const x = a[key];
    const y = b[key];
    return typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y));
  }
  function SortIcon({ active, dir }) {
    if (!active) return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronsUpDown, { className: "h-3.5 w-3.5 text-gray-300 group-hover:text-gray-400" });
    return dir === "asc" ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronUp, { className: "h-3.5 w-3.5 text-gray-700" }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronDown, { className: "h-3.5 w-3.5 text-gray-700" });
  }
  function App() {
    const [query, setQuery] = (0, import_react.useState)("");
    const [status, setStatus] = (0, import_react.useState)("All");
    const [sort, setSort] = (0, import_react.useState)({ key: "name", dir: "asc" });
    const [page, setPage] = (0, import_react.useState)(0);
    const [selected, setSelected] = (0, import_react.useState)(/* @__PURE__ */ new Set());
    const rows2 = (0, import_react.useMemo)(() => {
      const q = query.trim().toLowerCase();
      const filtered = members.filter(
        (m) => (status === "All" || m.status === status) && (!q || m.name.toLowerCase().includes(q) || m.email.toLowerCase().includes(q))
      );
      const sorted = [...filtered].sort((a, b) => compare(a, b, sort.key));
      return sort.dir === "asc" ? sorted : sorted.reverse();
    }, [query, status, sort]);
    const pages = Math.max(1, Math.ceil(rows2.length / PAGE_SIZE));
    const current = Math.min(page, pages - 1);
    const visible = rows2.slice(current * PAGE_SIZE, current * PAGE_SIZE + PAGE_SIZE);
    const allVisibleSelected = visible.length > 0 && visible.every((m) => selected.has(m.id));
    function toggleSort(key) {
      setSort((s) => s.key === key ? { key, dir: s.dir === "asc" ? "desc" : "asc" } : { key, dir: "asc" });
    }
    function toggle(id) {
      setSelected((s) => {
        const next = new Set(s);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
    }
    function toggleAll() {
      setSelected((s) => {
        const next = new Set(s);
        for (const m of visible) {
          if (allVisibleSelected) next.delete(m.id);
          else next.add(m.id);
        }
        return next;
      });
    }
    const counts = {
      All: members.length,
      Active: members.filter((m) => m.status === "Active").length,
      Invited: members.filter((m) => m.status === "Invited").length,
      Suspended: members.filter((m) => m.status === "Suspended").length
    };
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "min-h-screen bg-white font-sans text-gray-900", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mx-auto max-w-6xl px-8 py-8", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { className: "text-xl font-semibold text-gray-900", children: "Team members" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-600", children: "Everyone with access to the Northwind workspace, their role and when they were last seen." })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex gap-2", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "inline-flex items-center gap-2 rounded-md bg-white px-3 py-2 text-sm font-semibold text-gray-900 shadow-sm ring-1 ring-inset ring-gray-300 hover:bg-gray-50", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Download, { className: "h-4 w-4 text-gray-400" }),
            "Export CSV"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "inline-flex items-center gap-2 rounded-md bg-gray-900 px-3 py-2 text-sm font-semibold text-white shadow-sm hover:bg-gray-700", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Plus, { className: "h-4 w-4" }),
            "Invite"
          ] })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 flex items-center justify-between gap-4", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "inline-flex rounded-lg bg-gray-100 p-1", children: Object.keys(counts).map((s) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
          "button",
          {
            "data-status": s,
            onClick: () => {
              setStatus(s);
              setPage(0);
            },
            className: `rounded-md px-3 py-1.5 text-sm font-medium ${status === s ? "bg-white text-gray-900 shadow-sm" : "text-gray-500 hover:text-gray-700"}`,
            children: [
              s,
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `ml-1.5 rounded-full px-1.5 text-xs ${status === s ? "bg-gray-100 text-gray-700" : "text-gray-400"}`, children: counts[s] })
            ]
          },
          s
        )) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative w-72", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "pointer-events-none absolute inset-y-0 left-3 my-auto h-4 w-4 text-gray-400" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "input",
            {
              id: "filter",
              value: query,
              onChange: (e) => {
                setQuery(e.target.value);
                setPage(0);
              },
              placeholder: "Filter by name or email",
              className: "block w-full rounded-md border-0 py-2 pl-9 pr-3 text-sm ring-1 ring-inset ring-gray-300 placeholder:text-gray-400 focus:ring-2 focus:ring-inset focus:ring-gray-900"
            }
          )
        ] })
      ] }),
      selected.size > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-4 flex items-center gap-3 rounded-lg bg-gray-900 px-4 py-2 text-sm text-white", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "font-medium", children: [
          selected.size,
          " selected"
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "h-4 w-px bg-gray-600" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "inline-flex items-center gap-1.5 text-gray-300 hover:text-white", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Mail, { className: "h-4 w-4" }),
          "Email"
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-4 max-h-[480px] overflow-auto rounded-lg ring-1 ring-gray-200", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("table", { className: "min-w-full border-separate border-spacing-0 text-sm", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("th", { className: "sticky top-0 z-10 w-12 border-b border-gray-200 bg-gray-50/95 px-4 py-3 text-left", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { type: "checkbox", "aria-label": "Select all", checked: allVisibleSelected, onChange: toggleAll, className: "h-4 w-4 rounded border-gray-300" }) }),
          columns.map((c) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("th", { className: `sticky top-0 z-10 border-b border-gray-200 bg-gray-50/95 px-4 py-3 font-semibold text-gray-900 ${c.className ?? "text-left"}`, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { "data-sort": c.key, onClick: () => toggleSort(c.key), className: "group inline-flex items-center gap-1", children: [
            c.label,
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(SortIcon, { active: sort.key === c.key, dir: sort.dir })
          ] }) }, c.key))
        ] }) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tbody", { children: [
          visible.map((m) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { className: selected.has(m.id) ? "bg-gray-50" : "hover:bg-gray-50", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "border-b border-gray-100 px-4 py-3", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { type: "checkbox", "data-row": m.id, checked: selected.has(m.id), onChange: () => toggle(m.id), className: "h-4 w-4 rounded border-gray-300" }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "whitespace-nowrap border-b border-gray-100 px-4 py-3", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `flex h-9 w-9 items-center justify-center rounded-full text-xs font-semibold ${m.tone}`, children: m.name.split(" ").map((p) => p[0]).join("") }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "font-medium text-gray-900", children: m.name }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "text-gray-500", children: m.email })
              ] })
            ] }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "whitespace-nowrap border-b border-gray-100 px-4 py-3", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `rounded px-2 py-0.5 text-xs font-medium ${roleBadge[m.role]}`, children: m.role }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "whitespace-nowrap border-b border-gray-100 px-4 py-3", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: `inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium ring-1 ring-inset ${statusBadge[m.status]}`, children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `h-1.5 w-1.5 rounded-full ${statusDot[m.status]}` }),
              m.status
            ] }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "whitespace-nowrap border-b border-gray-100 px-4 py-3 text-gray-500", children: m.lastActive }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "whitespace-nowrap border-b border-gray-100 px-4 py-3 text-right tabular-nums text-gray-900", children: m.seats })
          ] }, m.id)),
          visible.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("tr", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("td", { colSpan: 6, className: "px-4 py-12 text-center text-gray-500", children: [
            "No members match “",
            query,
            "”."
          ] }) })
        ] })
      ] }) }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("nav", { className: "mt-4 flex items-center justify-between", "aria-label": "Pagination", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "text-sm text-gray-600", children: [
          "Showing ",
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "font-medium text-gray-900", children: rows2.length === 0 ? 0 : current * PAGE_SIZE + 1 }),
          " to",
          " ",
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "font-medium text-gray-900", children: Math.min(rows2.length, (current + 1) * PAGE_SIZE) }),
          " of",
          " ",
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "font-medium text-gray-900", children: rows2.length }),
          " members"
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-1", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "button",
            {
              id: "prev",
              disabled: current === 0,
              onClick: () => setPage(current - 1),
              className: "inline-flex h-8 w-8 items-center justify-center rounded-md ring-1 ring-inset ring-gray-300 hover:bg-gray-50 disabled:opacity-40",
              "aria-label": "Previous page",
              children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronLeft, { className: "h-4 w-4" })
            }
          ),
          Array.from({ length: pages }, (_, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "button",
            {
              "data-page": i + 1,
              onClick: () => setPage(i),
              className: `h-8 min-w-[2rem] rounded-md px-2 text-sm font-medium ${i === current ? "bg-gray-900 text-white" : "text-gray-700 ring-1 ring-inset ring-gray-300 hover:bg-gray-50"}`,
              children: i + 1
            },
            i
          )),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "button",
            {
              id: "next",
              disabled: current === pages - 1,
              onClick: () => setPage(current + 1),
              className: "inline-flex h-8 w-8 items-center justify-center rounded-md ring-1 ring-inset ring-gray-300 hover:bg-gray-50 disabled:opacity-40",
              "aria-label": "Next page",
              children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronRight, { className: "h-4 w-4" })
            }
          )
        ] })
      ] })
    ] }) });
  }

  // datatable/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
