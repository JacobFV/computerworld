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

  // chat/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // chat/App.tsx
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
  var EllipsisVertical = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "5", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "19", r: "1" })
  ] });
  var Paperclip = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" }) });
  var Phone = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z" }) });
  var Video = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.87a.5.5 0 0 0-.752-.432L16 10.5" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { x: "2", y: "6", width: "14", height: "12", rx: "2" })
  ] });
  var Send = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m21.854 2.147-10.94 10.939" })
  ] });
  var Smile = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M8 14s1.5 2 4 2 4-2 4-2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "9", x2: "9.01", y1: "9", y2: "9" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "15", x2: "15.01", y1: "9", y2: "9" })
  ] });
  var CheckCheck = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 6 7 17l-5-5" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m22 10-7.5 7.5L13 16" })
  ] });

  // chat/data.ts
  var me = "me";
  var short = (from, text, time) => [{ from, text, time }];
  var conversations = [
    {
      id: "priya",
      name: "Priya Raman",
      initials: "PR",
      color: "bg-rose-500",
      online: true,
      unread: 2,
      messages: [
        { from: "priya", text: "Morning! Did you get a chance to look at the onboarding flow?", time: "09:12" },
        { from: me, text: "Yes, going through it now. The first two screens feel great.", time: "09:15" },
        { from: me, text: "The permissions step is where I got lost, though.", time: "09:15" },
        { from: "priya", text: "Same feedback from the usability sessions. Four of six people stalled there.", time: "09:20" },
        { from: "priya", text: "I was thinking we split it: ask for notifications later, only when they first follow someone.", time: "09:21" },
        { from: me, text: "That makes sense. Contextual permission prompts convert much better anyway.", time: "09:30" },
        { from: "priya", text: "Exactly. I mocked up a version, sending the link in a sec.", time: "09:32" },
        { from: me, text: "Perfect, I can review before standup.", time: "09:33" },
        { from: "priya", text: "Here it is — the prototype is on the second page of the file.", time: "10:05" },
        { from: "priya", text: "Let me know what you think about the illustration on the last step too!", time: "10:06" }
      ]
    },
    { id: "tom", name: "Tom Becker", initials: "TB", color: "bg-sky-500", online: true, unread: 0, messages: [{ from: "tom", text: "Deploy is green, shipping at 3.", time: "09:58" }, { from: me, text: "Great, thanks for the heads-up!", time: "10:01" }] },
    { id: "design", name: "Design team", initials: "DT", color: "bg-violet-500", online: false, unread: 5, messages: short("lena", "Lena: Updated the icon set in the library", "09:47") },
    { id: "amara", name: "Amara Okoye", initials: "AO", color: "bg-emerald-500", online: false, unread: 0, messages: short(me, "Sounds good, see you Thursday", "Yesterday") },
    { id: "lucas", name: "Lucas Moreau", initials: "LM", color: "bg-amber-500", online: true, unread: 1, messages: short("lucas", "Can you share the Q2 numbers?", "Yesterday") },
    { id: "hana", name: "Hana Sato", initials: "HS", color: "bg-pink-500", online: false, unread: 0, messages: short("hana", "Thanks for the intro!", "Mon") },
    { id: "ops", name: "Ops alerts", initials: "OA", color: "bg-slate-600", online: false, unread: 0, messages: short("ops", "Disk usage on db-2 back under 70%", "Mon") },
    { id: "noah", name: "Noah Fischer", initials: "NF", color: "bg-teal-500", online: false, unread: 0, messages: short(me, "I will send the contract over tonight", "Sun") },
    { id: "isla", name: "Isla MacLeod", initials: "IM", color: "bg-orange-500", online: true, unread: 0, messages: short("isla", "Loved the talk, the slides were beautiful", "Sat") },
    { id: "ben", name: "Ben Adeyemi", initials: "BA", color: "bg-indigo-500", online: false, unread: 0, messages: short("ben", "Pushed a fix for the flaky test", "Fri") },
    { id: "zoe", name: "Zoë Laurent", initials: "ZL", color: "bg-fuchsia-500", online: false, unread: 0, messages: short(me, "Happy birthday!!", "Thu") },
    { id: "kai", name: "Kai Nakamura", initials: "KN", color: "bg-cyan-500", online: false, unread: 0, messages: short("kai", "Can we push our 1:1 to next week?", "Wed") }
  ];

  // chat/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  function Avatar({ c, size = "h-10 w-10", dot = false }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative shrink-0", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `flex ${size} items-center justify-center rounded-full ${c.color} text-sm font-semibold text-white`, children: c.initials }),
      dot && c.online && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute bottom-0 right-0 block h-2.5 w-2.5 rounded-full bg-emerald-500 ring-2 ring-white" })
    ] });
  }
  function ConversationList({ items, activeId, onSelect }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("aside", { className: "flex w-80 shrink-0 flex-col border-r border-slate-200 bg-white", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "border-b border-slate-200 p-4", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { className: "text-lg font-semibold text-slate-900", children: "Messages" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rounded-full bg-indigo-100 px-2 py-0.5 text-xs font-semibold text-indigo-700", children: [
            items.reduce((n, c) => n + c.unread, 0),
            " new"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative mt-3", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { placeholder: "Search", className: "w-full rounded-full bg-slate-100 py-2 pl-9 pr-4 text-sm text-slate-900 placeholder:text-slate-400 focus:outline-none focus:ring-2 focus:ring-indigo-500" })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "flex-1 overflow-y-auto", children: items.map((c) => {
        const last = c.messages[c.messages.length - 1];
        const active = c.id === activeId;
        return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
          "button",
          {
            "data-conversation": c.id,
            onClick: () => onSelect(c.id),
            className: `flex w-full items-center gap-3 border-l-2 px-4 py-3 text-left ${active ? "border-indigo-500 bg-indigo-50/60" : "border-transparent hover:bg-slate-50"}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Avatar, { c, dot: true }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-baseline justify-between gap-2", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-sm font-medium text-slate-900", children: c.name }),
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "shrink-0 text-xs text-slate-400", children: last.time })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-0.5 flex items-center justify-between gap-2", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: `truncate text-sm ${c.unread ? "font-medium text-slate-700" : "text-slate-500"}`, children: [
                    last.from === me ? "You: " : "",
                    last.text
                  ] }),
                  c.unread > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "flex h-5 min-w-[1.25rem] items-center justify-center rounded-full bg-indigo-600 px-1.5 text-[11px] font-semibold text-white", children: c.unread })
                ] })
              ] })
            ]
          }
        ) }, c.id);
      }) })
    ] });
  }
  function Bubble({ m, mine }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `flex ${mine ? "justify-end" : "justify-start"}`, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: `max-w-md ${mine ? "items-end" : "items-start"} flex flex-col`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
        "div",
        {
          className: `rounded-2xl px-4 py-2 text-sm leading-relaxed shadow-sm ${mine ? "rounded-br-md bg-indigo-600 text-white" : "rounded-bl-md bg-white text-slate-800 ring-1 ring-slate-200"}`,
          children: m.text
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "mt-1 flex items-center gap-1 text-[11px] text-slate-400", children: [
        m.time,
        mine && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CheckCheck, { className: "h-3.5 w-3.5 text-indigo-500" })
      ] })
    ] }) });
  }
  function App() {
    const [items, setItems] = (0, import_react.useState)(conversations);
    const [activeId, setActiveId] = (0, import_react.useState)(conversations[0].id);
    const [draft, setDraft] = (0, import_react.useState)("");
    const scroller = (0, import_react.useRef)(null);
    const active = items.find((c) => c.id === activeId);
    (0, import_react.useEffect)(() => {
      const el = scroller.current;
      if (el) el.scrollTop = el.scrollHeight;
    }, [activeId, active.messages.length]);
    function select(id) {
      setActiveId(id);
      setItems((cs) => cs.map((c) => c.id === id ? { ...c, unread: 0 } : c));
    }
    function send(e) {
      e.preventDefault();
      const text = draft.trim();
      if (!text) return;
      setItems((cs) => cs.map((c) => c.id === activeId ? { ...c, messages: [...c.messages, { from: me, text, time: "10:42" }] } : c));
      setDraft("");
    }
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex h-screen bg-slate-50 font-sans text-slate-900", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ConversationList, { items, activeId, onSelect: select }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("section", { className: "flex min-w-0 flex-1 flex-col", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("header", { className: "flex h-16 shrink-0 items-center gap-3 border-b border-slate-200 bg-white px-6", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Avatar, { c: active, size: "h-9 w-9" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "truncate text-sm font-semibold text-slate-900", children: active.name }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: `text-xs ${active.online ? "text-emerald-600" : "text-slate-400"}`, children: active.online ? "Online" : "Last seen yesterday" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex items-center gap-1 text-slate-500", children: [Phone, Video, EllipsisVertical].map((Icon2, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { className: "rounded-lg p-2 hover:bg-slate-100", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { className: "h-5 w-5" }) }, i)) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { ref: scroller, id: "thread", className: "flex-1 space-y-4 overflow-y-auto px-6 py-6", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-4", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "h-px flex-1 bg-slate-200" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-xs font-medium text-slate-400", children: "Today" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "h-px flex-1 bg-slate-200" })
          ] }),
          active.messages.map((m, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Bubble, { m, mine: m.from === me }, i))
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("form", { onSubmit: send, className: "shrink-0 border-t border-slate-200 bg-white p-4", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 focus-within:border-indigo-400 focus-within:ring-2 focus-within:ring-indigo-100", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", className: "text-slate-400 hover:text-slate-600", "aria-label": "Attach", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Paperclip, { className: "h-5 w-5" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "input",
            {
              id: "composer",
              value: draft,
              onChange: (e) => setDraft(e.target.value),
              placeholder: `Message ${active.name.split(" ")[0]}…`,
              className: "flex-1 bg-transparent py-1 text-sm placeholder:text-slate-400 focus:outline-none"
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", className: "text-slate-400 hover:text-slate-600", "aria-label": "Emoji", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Smile, { className: "h-5 w-5" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "button",
            {
              type: "submit",
              id: "send",
              disabled: !draft.trim(),
              className: "flex h-8 w-8 items-center justify-center rounded-lg bg-indigo-600 text-white disabled:bg-slate-300",
              "aria-label": "Send",
              children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Send, { className: "h-4 w-4" })
            }
          )
        ] }) })
      ] })
    ] });
  }

  // chat/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
