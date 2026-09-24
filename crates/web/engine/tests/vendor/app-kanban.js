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

  // kanban/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // kanban/App.tsx
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
  var Calendar = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M8 2v4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M16 2v4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "18", height: "18", x: "3", y: "4", rx: "2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M3 10h18" })
  ] });
  var Ellipsis = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "19", cy: "12", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "5", cy: "12", r: "1" })
  ] });
  var Plus = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 5v14" })
  ] });
  var X = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 6 6 18" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m6 6 12 12" })
  ] });
  var MessageSquare = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" }) });
  var Paperclip = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" }) });
  var Flag = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "4", x2: "4", y1: "22", y2: "15" })
  ] });
  var ArrowRight = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m12 5 7 7-7 7" })
  ] });
  var Filter = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polygon", { points: "22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" }) });

  // kanban/data.ts
  var people = {
    ar: { name: "Ava Reyes", initials: "AR", gradient: "from-pink-500 to-orange-400" },
    jk: { name: "Jun Kato", initials: "JK", gradient: "from-sky-500 to-indigo-500" },
    ml: { name: "Mia Lopez", initials: "ML", gradient: "from-emerald-500 to-teal-400" },
    do: { name: "Dev Okafor", initials: "DO", gradient: "from-violet-500 to-fuchsia-500" }
  };
  var initialCards = [
    { id: 1, title: "Audit the pricing page copy", column: "todo", priority: "Low", labels: ["Research"], assignees: ["ml"], due: "May 3", comments: 2, files: 0 },
    { id: 2, title: "Hero illustration for the relaunch", column: "todo", priority: "Medium", labels: ["Design"], assignees: ["ar", "do"], due: "May 6", comments: 5, files: 3 },
    { id: 3, title: "Checkout crashes on Safari 16 when the coupon field is empty", column: "todo", priority: "High", labels: ["Bug", "Frontend"], assignees: ["jk"], comments: 8, files: 1 },
    { id: 4, title: "Migrate the blog to the new CMS", column: "progress", priority: "Medium", labels: ["Backend"], assignees: ["do"], due: "May 9", comments: 1, files: 0, progress: 60 },
    { id: 5, title: "Responsive navigation", column: "progress", priority: "High", labels: ["Frontend", "Design"], assignees: ["jk", "ar"], comments: 4, files: 2, progress: 35 },
    { id: 6, title: "Rate-limit the signup endpoint", column: "review", priority: "High", labels: ["Backend"], assignees: ["ml", "jk"], due: "Apr 30", comments: 3, files: 0 },
    { id: 7, title: "Customer interview synthesis", column: "done", priority: "Low", labels: ["Research"], assignees: ["ar"], comments: 0, files: 4 }
  ];

  // kanban/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  var columns = [
    { id: "todo", title: "To do", dot: "bg-gray-400" },
    { id: "progress", title: "In progress", dot: "bg-sky-500" },
    { id: "review", title: "In review", dot: "bg-amber-500" },
    { id: "done", title: "Done", dot: "bg-emerald-500" }
  ];
  var nextColumn = {
    todo: "progress",
    progress: "review",
    review: "done",
    done: null
  };
  var priorityStyles = {
    Low: "text-gray-500",
    Medium: "text-amber-500",
    High: "text-rose-500"
  };
  var labelStyles = {
    Design: "bg-pink-50 text-pink-700",
    Frontend: "bg-indigo-50 text-indigo-700",
    Backend: "bg-emerald-50 text-emerald-700",
    Research: "bg-amber-50 text-amber-700",
    Bug: "bg-rose-50 text-rose-700"
  };
  function Avatar({ id, size = "h-6 w-6" }) {
    const person = people[id];
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "span",
      {
        title: person.name,
        className: `inline-flex ${size} items-center justify-center rounded-full bg-gradient-to-br ${person.gradient} text-[10px] font-semibold text-white ring-2 ring-white`,
        children: person.initials
      }
    );
  }
  function TaskCard({ card, onAdvance }) {
    const next = nextColumn[card.column];
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("article", { className: "group rounded-lg border border-gray-200 bg-white p-3 shadow-sm transition-shadow hover:shadow-md", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-start justify-between gap-2", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex flex-wrap gap-1", children: card.labels.map((l) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `rounded px-1.5 py-0.5 text-[11px] font-medium ${labelStyles[l]}`, children: l }, l)) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Flag, { className: `h-3.5 w-3.5 shrink-0 ${priorityStyles[card.priority]}` })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h3", { className: "mt-2 text-sm font-medium leading-snug text-gray-900", children: card.title }),
      card.progress !== void 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-3", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex justify-between text-[11px] text-gray-500", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { children: "Progress" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { children: [
            card.progress,
            "%"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-1 h-1.5 rounded-full bg-gray-100", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "h-1.5 rounded-full bg-sky-500", style: { width: `${card.progress}%` } }) })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-3 flex items-center justify-between", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex -space-x-1.5", children: card.assignees.map((a) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Avatar, { id: a }, a)) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3 text-xs text-gray-400", children: [
          card.due && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "flex items-center gap-1", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Calendar, { className: "h-3.5 w-3.5" }),
            card.due
          ] }),
          card.comments > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "flex items-center gap-1", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(MessageSquare, { className: "h-3.5 w-3.5" }),
            card.comments
          ] }),
          card.files > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "flex items-center gap-1", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Paperclip, { className: "h-3.5 w-3.5" }),
            card.files
          ] }),
          next && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "button",
            {
              onClick: () => onAdvance(card.id),
              "aria-label": `Move ${card.title}`,
              "data-card": card.id,
              className: "rounded p-0.5 text-gray-400 hover:bg-gray-100 hover:text-gray-700",
              children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ArrowRight, { className: "h-3.5 w-3.5" })
            }
          )
        ] })
      ] })
    ] });
  }
  function NewTaskModal({ onClose, onCreate }) {
    const [title, setTitle] = (0, import_react.useState)("");
    const [priority, setPriority] = (0, import_react.useState)("Medium");
    const [touched, setTouched] = (0, import_react.useState)(false);
    const error = touched && title.trim().length < 3 ? "Give the task a title of at least 3 characters." : null;
    function submit(e) {
      e.preventDefault();
      setTouched(true);
      if (title.trim().length < 3) return;
      onCreate(title.trim(), priority);
    }
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "fixed inset-0 z-50 flex items-center justify-center p-4", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "absolute inset-0 bg-gray-900/40 backdrop-blur-sm", onClick: onClose }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
        "form",
        {
          onSubmit: submit,
          role: "dialog",
          "aria-modal": "true",
          className: "relative w-full max-w-md rounded-2xl bg-white shadow-2xl ring-1 ring-gray-900/5",
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between border-b border-gray-100 px-6 py-4", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-gray-900", children: "New task" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", onClick: onClose, className: "rounded-md p-1 text-gray-400 hover:bg-gray-100", "aria-label": "Close", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(X, { className: "h-5 w-5" }) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "space-y-4 px-6 py-5", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("label", { htmlFor: "task-title", className: "block text-sm font-medium text-gray-700", children: "Title" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                  "input",
                  {
                    id: "task-title",
                    autoComplete: "off",
                    value: title,
                    onChange: (e) => setTitle(e.target.value),
                    placeholder: "e.g. Draft the onboarding email",
                    className: `mt-1.5 block w-full rounded-lg border px-3 py-2 text-sm shadow-sm focus:outline-none focus:ring-2 ${error ? "border-rose-300 focus:ring-rose-500/30" : "border-gray-300 focus:border-indigo-500 focus:ring-indigo-500/30"}`
                  }
                ),
                error && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1.5 text-xs text-rose-600", children: error })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "block text-sm font-medium text-gray-700", children: "Priority" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-1.5 grid grid-cols-3 gap-2", children: ["Low", "Medium", "High"].map((p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
                  "button",
                  {
                    type: "button",
                    "data-priority": p,
                    onClick: () => setPriority(p),
                    className: `flex items-center justify-center gap-1.5 rounded-lg border px-3 py-2 text-sm font-medium ${priority === p ? "border-indigo-500 bg-indigo-50 text-indigo-700" : "border-gray-200 text-gray-600 hover:bg-gray-50"}`,
                    children: [
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Flag, { className: `h-3.5 w-3.5 ${priorityStyles[p]}` }),
                      p
                    ]
                  },
                  p
                )) })
              ] })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex justify-end gap-3 rounded-b-2xl bg-gray-50 px-6 py-4", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", onClick: onClose, className: "rounded-lg px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100", children: "Cancel" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "submit", id: "create-task", className: "rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500", children: "Create task" })
            ] })
          ]
        }
      )
    ] });
  }
  function App() {
    const [cards, setCards] = (0, import_react.useState)(initialCards);
    const [showModal, setShowModal] = (0, import_react.useState)(false);
    function advance(id) {
      setCards((cs) => cs.map((c) => c.id === id && nextColumn[c.column] ? { ...c, column: nextColumn[c.column] } : c));
    }
    function create(title, priority) {
      setCards((cs) => [
        ...cs,
        { id: Math.max(...cs.map((c) => c.id)) + 1, title, priority, column: "todo", labels: ["Research"], assignees: ["ar"], comments: 0, files: 0 }
      ]);
      setShowModal(false);
    }
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex h-screen flex-col bg-gray-50 font-sans text-gray-900", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("header", { className: "border-b border-gray-200 bg-white", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between px-6 py-4", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("nav", { className: "text-xs text-gray-500", children: [
              "Projects ",
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "mx-1 text-gray-300", children: "/" }),
              " Website relaunch"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { className: "mt-1 text-xl font-semibold text-gray-900", children: "Sprint 14" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex -space-x-2", children: Object.keys(people).map((id) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Avatar, { id, size: "h-8 w-8" }, id)) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
              "button",
              {
                id: "new-task",
                onClick: () => setShowModal(true),
                className: "inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3.5 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500",
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Plus, { className: "h-4 w-4" }),
                  "New task"
                ]
              }
            )
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3 px-6 pb-3", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "absolute left-2.5 top-2 h-4 w-4 text-gray-400" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { placeholder: "Filter cards", className: "w-56 rounded-md border border-gray-200 py-1.5 pl-8 pr-3 text-sm placeholder:text-gray-400" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "inline-flex items-center gap-1.5 rounded-md border border-gray-200 px-2.5 py-1.5 text-sm text-gray-600 hover:bg-gray-50", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Filter, { className: "h-4 w-4" }),
            "Filters"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "ml-auto text-sm text-gray-500", children: [
            cards.filter((c) => c.column === "done").length,
            " of ",
            cards.length,
            " done"
          ] })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("main", { className: "flex flex-1 gap-4 overflow-x-auto p-6", children: columns.map((col) => {
        const list = cards.filter((c) => c.column === col.id);
        return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("section", { className: "flex w-72 shrink-0 flex-col rounded-xl bg-gray-100/80", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between px-3 py-3", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-2", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `h-2 w-2 rounded-full ${col.dot}` }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-sm font-semibold text-gray-700", children: col.title }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rounded-full bg-white px-2 text-xs font-medium text-gray-500 shadow-sm", children: list.length })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { className: "rounded p-1 text-gray-400 hover:bg-white", "aria-label": `${col.title} options`, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Ellipsis, { className: "h-4 w-4" }) })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex-1 space-y-2 overflow-y-auto px-2 pb-2", children: [
            list.map((card) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(TaskCard, { card, onAdvance: advance }, card.id)),
            list.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rounded-lg border-2 border-dashed border-gray-200 p-6 text-center text-xs text-gray-400", children: "Drop cards here" })
          ] })
        ] }, col.id);
      }) }),
      showModal && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(NewTaskModal, { onClose: () => setShowModal(false), onCreate: create })
    ] });
  }

  // kanban/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
