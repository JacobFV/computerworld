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

  // calendar/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // calendar/App.tsx
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
  var ChevronLeft = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m15 18-6-6 6-6" }) });
  var ChevronRight = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 18 6-6-6-6" }) });
  var Calendar = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M8 2v4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M16 2v4" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "18", height: "18", x: "3", y: "4", rx: "2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M3 10h18" })
  ] });
  var Check = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M20 6 9 17l-5-5" }) });
  var Plus = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 5v14" })
  ] });
  var X = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 6 6 18" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m6 6 12 12" })
  ] });
  var CircleCheck = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 12 2 2 4-4" })
  ] });
  var Globe = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M2 12h20" })
  ] });
  var Video = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.87a.5.5 0 0 0-.752-.432L16 10.5" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { x: "2", y: "6", width: "14", height: "12", rx: "2" })
  ] });
  var Menu = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "4", x2: "20", y1: "12", y2: "12" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "4", x2: "20", y1: "6", y2: "6" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "4", x2: "20", y1: "18", y2: "18" })
  ] });
  var Clock = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "12 6 12 12 16 14" })
  ] });

  // calendar/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  var TODAY = new Date(2026, 8, 24);
  var NOW_HOURS = 11 + 40 / 60;
  var HOUR = 52;
  var calendars = [
    {
      id: "work",
      name: "Work",
      dot: "bg-indigo-500",
      check: "border-indigo-500 bg-indigo-500",
      block: "border-indigo-500 bg-indigo-50 text-indigo-900 hover:bg-indigo-100",
      sub: "text-indigo-600",
      fill: "bg-indigo-500"
    },
    {
      id: "team",
      name: "Team events",
      dot: "bg-sky-500",
      check: "border-sky-500 bg-sky-500",
      block: "border-sky-500 bg-sky-50 text-sky-900 hover:bg-sky-100",
      sub: "text-sky-600",
      fill: "bg-sky-500"
    },
    {
      id: "personal",
      name: "Personal",
      dot: "bg-emerald-500",
      check: "border-emerald-500 bg-emerald-500",
      block: "border-emerald-500 bg-emerald-50 text-emerald-900 hover:bg-emerald-100",
      sub: "text-emerald-600",
      fill: "bg-emerald-500"
    },
    {
      id: "focus",
      name: "Focus time",
      dot: "bg-amber-500",
      check: "border-amber-500 bg-amber-500",
      block: "border-amber-500 bg-amber-50 text-amber-900 hover:bg-amber-100",
      sub: "text-amber-600",
      fill: "bg-amber-500"
    },
    {
      id: "holidays",
      name: "Holidays",
      dot: "bg-rose-500",
      check: "border-rose-500 bg-rose-500",
      block: "border-rose-500 bg-rose-50 text-rose-900 hover:bg-rose-100",
      sub: "text-rose-600",
      fill: "bg-rose-500"
    }
  ];
  var calendarById = Object.fromEntries(calendars.map((c) => [c.id, c]));
  var MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
  var WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  function addDays(d, n) {
    return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
  }
  function startOfWeek(d) {
    return addDays(d, -d.getDay());
  }
  function dateKey(d) {
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  }
  function sameDay(a, b) {
    return dateKey(a) === dateKey(b);
  }
  function monthGrid(year, month) {
    const first = startOfWeek(new Date(year, month, 1));
    return Array.from({ length: 42 }, (_, i) => addDays(first, i));
  }
  function isoWeek(d) {
    const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
    const day = t.getUTCDay() || 7;
    t.setUTCDate(t.getUTCDate() + 4 - day);
    const yearStart = Date.UTC(t.getUTCFullYear(), 0, 1);
    return Math.ceil(((t.getTime() - yearStart) / 864e5 + 1) / 7);
  }
  function formatTime(h, withSuffix = true) {
    const hour = Math.floor(h);
    const min = Math.round((h - hour) * 60);
    const h12 = hour % 12 === 0 ? 12 : hour % 12;
    const suffix = hour < 12 || hour === 24 ? "AM" : "PM";
    return `${h12}:${String(min).padStart(2, "0")}${withSuffix ? ` ${suffix}` : ""}`;
  }
  function formatRange(start, end) {
    const isAm = (h) => h < 12 || h >= 24;
    const sameHalf = isAm(start) === isAm(end);
    return `${formatTime(start, !sameHalf)} – ${formatTime(end)}`;
  }
  function shortTime(h) {
    const hour = Math.floor(h);
    const min = Math.round((h - hour) * 60);
    const h12 = hour % 12 === 0 ? 12 : hour % 12;
    return `${h12}${min ? `:${String(min).padStart(2, "0")}` : ""}${hour < 12 ? "am" : "pm"}`;
  }
  function hourLabel(h) {
    if (h === 0) return "";
    const h12 = h % 12 === 0 ? 12 : h % 12;
    return `${h12} ${h < 12 ? "AM" : "PM"}`;
  }
  function seedEvents() {
    const list = [];
    for (let d = 1; d <= 30; d++) {
      const date = new Date(2026, 8, d);
      const wd = date.getDay();
      if (wd === 0 || wd === 6 || d === 7) continue;
      list.push({ title: "Daily standup", date: dateKey(date), calendar: "team", start: 9, end: 9.5, location: "Zoom" });
    }
    list.push(
      { title: "Welcome lunch for Jonas", date: "2026-08-31", calendar: "team", start: 12, end: 13 },
      { title: "Q4 planning kickoff", date: "2026-09-01", calendar: "work", start: 10, end: 11.5 },
      { title: "Dentist", date: "2026-09-03", calendar: "personal", start: 16, end: 17 },
      { title: "Labor Day", date: "2026-09-07", calendar: "holidays" },
      { title: "Board deck review", date: "2026-09-10", calendar: "work", start: 14, end: 15 },
      { title: "Team offsite", date: "2026-09-11", calendar: "team" },
      { title: "Design critique", date: "2026-09-14", calendar: "team", start: 15, end: 16 },
      { title: "Quarterly business review", date: "2026-09-17", calendar: "work", start: 11, end: 12.5 },
      { title: "The National — Greek Theatre", date: "2026-09-18", calendar: "personal", start: 20, end: 22 },
      // The week of 20 September.
      { title: "Long run", date: "2026-09-20", calendar: "personal", start: 8, end: 9.5, location: "Crissy Field" },
      { title: "Meal prep", date: "2026-09-20", calendar: "personal", start: 16, end: 17.5 },
      { title: "Design review: onboarding", date: "2026-09-21", calendar: "work", start: 10, end: 11.5, location: "Room 4B" },
      { title: "Lunch with Priya", date: "2026-09-21", calendar: "personal", start: 12.5, end: 13.5, location: "Tartine" },
      { title: "Deep work", date: "2026-09-21", calendar: "focus", start: 14, end: 16 },
      { title: "Autumn equinox", date: "2026-09-22", calendar: "holidays" },
      { title: "1:1 with Marcus", date: "2026-09-22", calendar: "work", start: 11, end: 12 },
      { title: "Sprint planning", date: "2026-09-22", calendar: "team", start: 13, end: 14.5, location: "Zoom" },
      { title: "Yoga", date: "2026-09-22", calendar: "personal", start: 17.5, end: 18.5 },
      { title: "Focus: pricing page", date: "2026-09-23", calendar: "focus", start: 9.5, end: 11.5 },
      { title: "Customer call — Acme", date: "2026-09-23", calendar: "work", start: 14, end: 15, location: "Google Meet" },
      { title: "Hiring sync", date: "2026-09-23", calendar: "team", start: 15.5, end: 16.25 },
      { title: "Roadmap review", date: "2026-09-24", calendar: "work", start: 10, end: 11, location: "Room 2A" },
      { title: "Interview: frontend", date: "2026-09-24", calendar: "team", start: 10.5, end: 11.5, location: "Zoom" },
      { title: "Lunch & learn", date: "2026-09-24", calendar: "team", start: 13, end: 14, location: "Atrium" },
      { title: "Deep work", date: "2026-09-24", calendar: "focus", start: 15, end: 17 },
      { title: "Demo day prep", date: "2026-09-25", calendar: "work", start: 11, end: 12 },
      { title: "Team retro", date: "2026-09-25", calendar: "team", start: 16, end: 17, location: "Room 4B" },
      { title: "Dinner at Nopa", date: "2026-09-25", calendar: "personal", start: 19, end: 21 },
      { title: "Farmers market", date: "2026-09-26", calendar: "personal", start: 10, end: 11.5, location: "Ferry Building" },
      { title: "v2.4 release", date: "2026-09-29", calendar: "work", start: 10, end: 11 },
      { title: "Month-end close", date: "2026-09-30", calendar: "work", start: 16, end: 17 },
      { title: "Weekend in Tahoe", date: "2026-10-02", calendar: "personal" }
    );
    return list.map((e, i) => ({ ...e, id: i + 1 }));
  }
  function sortEvents(list) {
    return [...list].sort((a, b) => {
      if (a.start === void 0 && b.start !== void 0) return -1;
      if (b.start === void 0 && a.start !== void 0) return 1;
      return (a.start ?? 0) - (b.start ?? 0) || (b.end ?? 0) - (a.end ?? 0) || a.id - b.id;
    });
  }
  function layoutDay(events) {
    const sorted = sortEvents(events.filter((e) => e.start !== void 0));
    const placed = [];
    let cluster = [];
    let columnsEnd = [];
    let clusterEnd = -1;
    const flush = () => {
      for (const p of cluster) p.cols = columnsEnd.length;
      placed.push(...cluster);
      cluster = [];
      columnsEnd = [];
    };
    for (const event of sorted) {
      if (event.start >= clusterEnd) flush();
      let col = columnsEnd.findIndex((end) => end <= event.start);
      if (col === -1) {
        col = columnsEnd.length;
        columnsEnd.push(event.end);
      } else {
        columnsEnd[col] = event.end;
      }
      cluster.push({ event, col, cols: 1 });
      clusterEnd = Math.max(clusterEnd, event.end);
    }
    flush();
    return placed;
  }
  function MiniMonth({ anchor, onPick }) {
    const [shown, setShown] = (0, import_react.useState)({ year: anchor.getFullYear(), month: anchor.getMonth() });
    const days = monthGrid(shown.year, shown.month);
    const weekStart = dateKey(startOfWeek(anchor));
    function shift(n) {
      const d = new Date(shown.year, shown.month + n, 1);
      setShown({ year: d.getFullYear(), month: d.getMonth() });
    }
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between px-1", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "text-sm font-semibold text-gray-900", children: [
          MONTHS[shown.month],
          " ",
          shown.year
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { onClick: () => shift(-1), className: "rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700", "aria-label": "Previous month", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronLeft, { className: "h-4 w-4" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { onClick: () => shift(1), className: "rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700", "aria-label": "Next month", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronRight, { className: "h-4 w-4" }) })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-2 grid grid-cols-7 text-center text-[11px] font-medium text-gray-400", children: WEEKDAYS.map((w) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "py-1", children: w[0] }, w)) }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "grid grid-cols-7 gap-y-0.5 text-center text-xs", children: days.map((d, i) => {
        const inWeek = dateKey(startOfWeek(d)) === weekStart;
        const isToday = sameDay(d, TODAY);
        const outside = d.getMonth() !== shown.month;
        const rounded = inWeek ? i % 7 === 0 ? "rounded-l-md" : i % 7 === 6 ? "rounded-r-md" : "" : "";
        return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `py-0.5 ${inWeek ? "bg-indigo-50" : ""} ${rounded}`, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
          "button",
          {
            onClick: () => onPick(d),
            className: `mx-auto flex h-7 w-7 items-center justify-center rounded-full ${isToday ? "bg-indigo-600 font-semibold text-white" : outside ? "text-gray-300 hover:bg-gray-100" : inWeek ? "font-medium text-indigo-700 hover:bg-indigo-100" : "text-gray-700 hover:bg-gray-100"}`,
            children: d.getDate()
          }
        ) }, dateKey(d));
      }) })
    ] });
  }
  function CalendarToggle({ cal, on, onToggle }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
      "button",
      {
        id: `cal-toggle-${cal.id}`,
        role: "checkbox",
        "aria-checked": on,
        onClick: onToggle,
        className: "flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left text-sm text-gray-700 hover:bg-gray-50",
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `flex h-4 w-4 shrink-0 items-center justify-center rounded border-2 ${on ? cal.check : "border-gray-300 bg-white"}`, children: on && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Check, { className: "h-3 w-3 text-white" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `flex-1 ${on ? "" : "text-gray-400"}`, children: cal.name })
        ]
      }
    );
  }
  function EventBlock({ placed }) {
    const { event, col, cols } = placed;
    const cal = calendarById[event.calendar];
    const duration = event.end - event.start;
    const short = duration <= 0.5;
    const left = col / cols * 60;
    const width = col === cols - 1 ? 100 - left : Math.min(100 / cols * 1.7, 100 - left);
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "button",
      {
        "data-event": event.id,
        className: `absolute overflow-hidden rounded-md border-l-[3px] px-2 text-left shadow-sm ring-1 ring-white ${cal.block} ${short ? "py-0.5" : "py-1"}`,
        style: {
          top: event.start * HOUR + 1,
          height: duration * HOUR - 2,
          left: `calc(${left}% + 2px)`,
          width: `calc(${width}% - 4px)`,
          zIndex: 1 + col
        },
        children: short ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "truncate text-[11px] leading-4", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "font-semibold", children: event.title }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `ml-1 ${cal.sub}`, children: formatTime(event.start) })
        ] }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-xs font-semibold leading-4", children: event.title }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: `truncate text-[11px] leading-4 ${cal.sub}`, children: formatRange(event.start, event.end) }),
          event.location && duration >= 1 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: `truncate text-[11px] leading-4 ${cal.sub} opacity-80`, children: event.location })
        ] })
      }
    );
  }
  function WeekView({ anchor, events }) {
    const scroller = (0, import_react.useRef)(null);
    const days = Array.from({ length: 7 }, (_, i) => addDays(startOfWeek(anchor), i));
    const hours = Array.from({ length: 24 }, (_, h) => h);
    (0, import_react.useLayoutEffect)(() => {
      if (scroller.current) scroller.current.scrollTop = 7.5 * HOUR;
    }, []);
    const byDay = days.map((d) => events.filter((e) => e.date === dateKey(d)));
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { ref: scroller, className: "flex-1 overflow-y-auto", id: "week-scroller", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "sticky top-0 z-20 border-b border-gray-200 bg-white", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "grid grid-cols-[4rem_repeat(7,minmax(0,1fr))]", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex items-end justify-end pb-2 pr-2 text-[10px] font-medium text-gray-400", children: "GMT-7" }),
          days.map((d) => {
            const isToday = sameDay(d, TODAY);
            return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex flex-col items-center border-l border-gray-100 py-2", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `text-[11px] font-semibold uppercase tracking-wide ${isToday ? "text-indigo-600" : "text-gray-500"}`, children: WEEKDAYS[d.getDay()] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                "span",
                {
                  className: `mt-0.5 flex h-8 w-8 items-center justify-center rounded-full text-lg ${isToday ? "bg-indigo-600 font-semibold text-white" : "font-medium text-gray-900"}`,
                  children: d.getDate()
                }
              )
            ] }, dateKey(d));
          })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "grid grid-cols-[4rem_repeat(7,minmax(0,1fr))] border-t border-gray-100", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex items-center justify-end pr-2 text-[10px] font-medium text-gray-400", children: "all-day" }),
          byDay.map((list, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "min-h-[28px] space-y-0.5 border-l border-gray-100 p-0.5", children: list.filter((e) => e.start === void 0).map((e) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `truncate rounded px-1.5 py-0.5 text-[11px] font-semibold text-white ${calendarById[e.calendar].fill}`, children: e.title }, e.id)) }, i))
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "grid grid-cols-[4rem_repeat(7,minmax(0,1fr))]", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "relative", children: hours.map((h) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "relative", style: { height: HOUR }, children: h > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute -top-2 right-2 text-[11px] text-gray-400", children: hourLabel(h) }) }, h)) }),
        days.map((d, i) => {
          const isToday = sameDay(d, TODAY);
          return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: `relative border-l border-gray-100 ${isToday ? "bg-indigo-50/40" : ""}`, children: [
            hours.map((h) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "border-t border-gray-100", style: { height: HOUR } }, h)),
            layoutDay(byDay[i]).map((p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(EventBlock, { placed: p }, p.event.id)),
            isToday && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "pointer-events-none absolute inset-x-0 z-10 flex items-center", style: { top: NOW_HOURS * HOUR - 5 }, children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "-ml-[5px] h-2.5 w-2.5 rounded-full bg-rose-500" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "h-0.5 flex-1 bg-rose-500" })
            ] })
          ] }, dateKey(d));
        })
      ] })
    ] });
  }
  function MonthView({ anchor, events, onPickDay }) {
    const first = new Date(anchor.getFullYear(), anchor.getMonth(), 1);
    const daysInMonth = new Date(anchor.getFullYear(), anchor.getMonth() + 1, 0).getDate();
    const rows = Math.ceil((first.getDay() + daysInMonth) / 7);
    const days = monthGrid(anchor.getFullYear(), anchor.getMonth()).slice(0, rows * 7);
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex flex-1 flex-col", id: "month-grid", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "grid grid-cols-7 border-b border-gray-200", children: WEEKDAYS.map((w) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "border-l border-gray-100 py-2 text-center text-[11px] font-semibold uppercase tracking-wide text-gray-500 first:border-l-0", children: w }, w)) }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "grid min-h-0 flex-1 grid-cols-7", style: { gridTemplateRows: `repeat(${rows}, minmax(0, 1fr))` }, children: days.map((d, i) => {
        const list = sortEvents(events.filter((e) => e.date === dateKey(d)));
        const outside = d.getMonth() !== anchor.getMonth();
        const isToday = sameDay(d, TODAY);
        const visible = list.length > 3 ? list.slice(0, 2) : list;
        return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
          "div",
          {
            className: `min-h-0 overflow-hidden border-b border-gray-100 p-1.5 ${i % 7 === 0 ? "" : "border-l"} ${outside ? "bg-gray-50/70" : "bg-white"}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                "button",
                {
                  onClick: () => onPickDay(d),
                  className: `flex h-6 min-w-[1.5rem] items-center justify-center rounded-full px-1 text-xs ${isToday ? "bg-indigo-600 font-semibold text-white" : outside ? "text-gray-400 hover:bg-gray-100" : "font-medium text-gray-700 hover:bg-gray-100"}`,
                  children: d.getDate() === 1 ? `${MONTHS[d.getMonth()].slice(0, 3)} 1` : d.getDate()
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-1 space-y-0.5", children: [
                visible.map((e) => {
                  const cal = calendarById[e.calendar];
                  return e.start === void 0 ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `truncate rounded px-1.5 py-px text-[11px] font-semibold text-white ${cal.fill}`, children: e.title }, e.id) : /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-1.5 rounded px-1 py-px text-[11px] text-gray-700 hover:bg-gray-100", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `h-1.5 w-1.5 shrink-0 rounded-full ${cal.dot}` }),
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "shrink-0 text-gray-500", children: shortTime(e.start) }),
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "truncate font-medium", children: e.title })
                  ] }, e.id);
                }),
                list.length > visible.length && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "px-1 text-[11px] font-semibold text-gray-500", children: [
                  list.length - visible.length,
                  " more"
                ] })
              ] })
            ]
          },
          dateKey(d)
        );
      }) })
    ] });
  }
  function SelectField({ id, label, value, onChange, options }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("label", { htmlFor: id, className: "block text-xs font-medium text-gray-600", children: label }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative mt-1", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
          "select",
          {
            id,
            value,
            onChange: (e) => onChange(e.target.value),
            className: "block w-full appearance-none rounded-lg border border-gray-300 bg-white py-2 pl-3 pr-8 text-sm text-gray-900 shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500/30",
            children: options.map((o) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("option", { value: o.value, children: o.label }, o.value))
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronDown, { className: "pointer-events-none absolute right-2.5 top-2.5 h-4 w-4 text-gray-400" })
      ] })
    ] });
  }
  function NewEventModal({ days, onClose, onSave }) {
    const defaultDay = days.find((d) => sameDay(d, TODAY)) ?? days[1];
    const [draft, setDraft] = (0, import_react.useState)({ title: "", date: dateKey(defaultDay), start: 12, end: 13, calendar: "work" });
    const [touched, setTouched] = (0, import_react.useState)(false);
    const error = touched && !draft.title.trim() ? "Add a title for the event." : null;
    const slots = Array.from({ length: 48 }, (_, i) => i / 2);
    const startOptions = slots.map((h) => ({ value: String(h), label: formatTime(h) }));
    const endOptions = slots.filter((h) => h > draft.start).concat([24]).map((h) => ({ value: String(h), label: h === 24 ? "12:00 AM" : formatTime(h) }));
    function setStart(v) {
      const start = Number(v);
      setDraft((d) => ({ ...d, start, end: d.end <= start ? Math.min(start + 1, 24) : d.end }));
    }
    function submit(e) {
      e.preventDefault();
      setTouched(true);
      if (!draft.title.trim()) return;
      onSave({ ...draft, title: draft.title.trim() });
    }
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "fixed inset-0 z-50 flex items-center justify-center p-4", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "absolute inset-0 bg-gray-900/40 backdrop-blur-sm", onClick: onClose }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("form", { onSubmit: submit, role: "dialog", "aria-modal": "true", "aria-labelledby": "new-event-heading", className: "relative w-full max-w-md rounded-2xl bg-white shadow-2xl ring-1 ring-gray-900/5", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between border-b border-gray-100 px-6 py-4", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { id: "new-event-heading", className: "text-base font-semibold text-gray-900", children: "New event" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", onClick: onClose, className: "rounded-md p-1 text-gray-400 hover:bg-gray-100", "aria-label": "Close", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(X, { className: "h-5 w-5" }) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "space-y-4 px-6 py-5", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("label", { htmlFor: "event-title", className: "block text-xs font-medium text-gray-600", children: "Title" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              "input",
              {
                id: "event-title",
                autoComplete: "off",
                value: draft.title,
                onChange: (e) => setDraft((d) => ({ ...d, title: e.target.value })),
                placeholder: "e.g. Coffee with Sam",
                className: `mt-1 block w-full rounded-lg border px-3 py-2 text-sm shadow-sm focus:outline-none focus:ring-2 ${error ? "border-rose-300 focus:ring-rose-500/30" : "border-gray-300 focus:border-indigo-500 focus:ring-indigo-500/30"}`
              }
            ),
            error && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1.5 text-xs text-rose-600", children: error })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "grid grid-cols-[1.3fr_1fr_1fr] gap-3", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              SelectField,
              {
                id: "event-day",
                label: "Day",
                value: draft.date,
                onChange: (v) => setDraft((d) => ({ ...d, date: v })),
                options: days.map((d) => ({ value: dateKey(d), label: `${WEEKDAYS[d.getDay()]}, ${MONTHS[d.getMonth()].slice(0, 3)} ${d.getDate()}` }))
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(SelectField, { id: "event-start", label: "Starts", value: String(draft.start), onChange: setStart, options: startOptions }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(SelectField, { id: "event-end", label: "Ends", value: String(draft.end), onChange: (v) => setDraft((d) => ({ ...d, end: Number(v) })), options: endOptions })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "block text-xs font-medium text-gray-600", children: "Calendar" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-1.5 flex flex-wrap gap-2", children: calendars.filter((c) => c.id !== "holidays").map((c) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
              "button",
              {
                type: "button",
                "data-calendar-option": c.id,
                onClick: () => setDraft((d) => ({ ...d, calendar: c.id })),
                className: `inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium ${draft.calendar === c.id ? "border-indigo-500 bg-indigo-50 text-indigo-700" : "border-gray-200 text-gray-600 hover:bg-gray-50"}`,
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `h-2 w-2 rounded-full ${c.dot}` }),
                  c.name
                ]
              },
              c.id
            )) })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-2 rounded-lg bg-gray-50 px-3 py-2 text-xs text-gray-500", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Globe, { className: "h-4 w-4 shrink-0 text-gray-400" }),
            "Pacific Time — San Francisco (GMT-7)"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex justify-end gap-3 rounded-b-2xl bg-gray-50 px-6 py-4", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", onClick: onClose, className: "rounded-lg px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100", children: "Cancel" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "submit", id: "save-event", className: "rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500", children: "Save event" })
        ] })
      ] })
    ] });
  }
  function App() {
    const [events, setEvents] = (0, import_react.useState)(seedEvents);
    const [anchor, setAnchor] = (0, import_react.useState)(TODAY);
    const [view, setView] = (0, import_react.useState)("week");
    const [visible, setVisible] = (0, import_react.useState)({ work: true, team: true, personal: true, focus: true, holidays: true });
    const [showModal, setShowModal] = (0, import_react.useState)(false);
    const [toast, setToast] = (0, import_react.useState)(null);
    const shown = (0, import_react.useMemo)(() => events.filter((e) => visible[e.calendar]), [events, visible]);
    const weekDays = Array.from({ length: 7 }, (_, i) => addDays(startOfWeek(anchor), i));
    const upNext = sortEvents(shown.filter((e) => e.date === dateKey(TODAY) && e.start !== void 0 && e.start >= NOW_HOURS)).slice(0, 2);
    function step(n) {
      if (view === "week") setAnchor((a) => addDays(a, 7 * n));
      else setAnchor((a) => new Date(a.getFullYear(), a.getMonth() + n, 1));
    }
    function save(d) {
      setEvents((es) => [...es, { id: Math.max(...es.map((e) => e.id)) + 1, title: d.title, date: d.date, start: d.start, end: d.end, calendar: d.calendar }]);
      setVisible((v) => ({ ...v, [d.calendar]: true }));
      setShowModal(false);
      const [y, m, day] = d.date.split("-").map(Number);
      const date = new Date(y, m - 1, day);
      setToast(`${d.title} · ${WEEKDAYS[date.getDay()]}, ${MONTHS[date.getMonth()].slice(0, 3)} ${date.getDate()}, ${formatRange(d.start, d.end)}`);
    }
    const first = weekDays[0];
    const last = weekDays[6];
    const title = view === "month" ? `${MONTHS[anchor.getMonth()]} ${anchor.getFullYear()}` : first.getMonth() === last.getMonth() ? `${MONTHS[first.getMonth()]} ${first.getFullYear()}` : `${MONTHS[first.getMonth()].slice(0, 3)} – ${MONTHS[last.getMonth()].slice(0, 3)} ${last.getFullYear()}`;
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex h-screen flex-col overflow-hidden bg-white font-sans text-gray-900", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("header", { className: "flex h-14 shrink-0 items-center gap-4 border-b border-gray-200 px-4", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { className: "rounded-md p-2 text-gray-500 hover:bg-gray-100", "aria-label": "Menu", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Menu, { className: "h-5 w-5" }) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-2", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-indigo-500 to-violet-600 text-white shadow-sm", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Calendar, { className: "h-4 w-4" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-base font-semibold tracking-tight", children: "Cadence" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative ml-8", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "absolute left-3 top-2.5 h-4 w-4 text-gray-400" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { placeholder: "Search events, people, rooms", className: "w-80 rounded-lg border border-gray-200 bg-gray-50 py-2 pl-9 pr-3 text-sm placeholder:text-gray-400 focus:bg-white focus:outline-none" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "ml-auto flex items-center gap-1", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { className: "rounded-md p-2 text-gray-500 hover:bg-gray-100", "aria-label": "Settings", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Settings, { className: "h-5 w-5" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "relative rounded-md p-2 text-gray-500 hover:bg-gray-100", "aria-label": "Notifications", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Bell, { className: "h-5 w-5" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute right-2 top-2 h-2 w-2 rounded-full bg-rose-500 ring-2 ring-white" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "ml-2 inline-flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-br from-pink-500 to-orange-400 text-xs font-semibold text-white", children: "MC" })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex min-h-0 flex-1", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("aside", { className: "flex w-64 shrink-0 flex-col gap-6 overflow-y-auto border-r border-gray-200 p-4", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
            "button",
            {
              id: "new-event",
              onClick: () => setShowModal(true),
              className: "inline-flex items-center justify-center gap-2 rounded-lg bg-indigo-600 px-4 py-2.5 text-sm font-medium text-white shadow-sm hover:bg-indigo-500",
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Plus, { className: "h-4 w-4" }),
                "New event"
              ]
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(MiniMonth, { anchor, onPick: (d) => setAnchor(d) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between px-2", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h3", { className: "text-xs font-semibold uppercase tracking-wide text-gray-500", children: "My calendars" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { className: "rounded p-0.5 text-gray-400 hover:bg-gray-100 hover:text-gray-700", "aria-label": "Add calendar", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Plus, { className: "h-4 w-4" }) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-2 space-y-0.5", children: calendars.map((c) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CalendarToggle, { cal: c, on: visible[c.id], onToggle: () => setVisible((v) => ({ ...v, [c.id]: !v[c.id] })) }, c.id)) })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-auto rounded-xl border border-gray-200 p-3", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-gray-500", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Clock, { className: "h-3.5 w-3.5" }),
              "Up next today"
            ] }),
            upNext.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-2 text-sm text-gray-500", children: "Nothing else today." }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "mt-2 space-y-2", children: upNext.map((e) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { className: "flex gap-2.5", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `mt-1 h-8 w-1 shrink-0 rounded-full ${calendarById[e.calendar].dot}` }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-sm font-medium text-gray-900", children: e.title }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "flex items-center gap-1 truncate text-xs text-gray-500", children: [
                  formatRange(e.start, e.end),
                  e.location && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "text-gray-400", children: [
                    "· ",
                    e.location
                  ] })
                ] })
              ] })
            ] }, e.id)) }),
            upNext[0]?.location === "Zoom" && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "mt-3 inline-flex w-full items-center justify-center gap-1.5 rounded-md bg-gray-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-gray-800", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Video, { className: "h-3.5 w-3.5" }),
              "Join call"
            ] })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("main", { className: "flex min-w-0 flex-1 flex-col", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex shrink-0 items-center gap-3 border-b border-gray-200 px-6 py-3", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              "button",
              {
                id: "today",
                onClick: () => setAnchor(TODAY),
                className: "rounded-lg border border-gray-200 px-3 py-1.5 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50",
                children: "Today"
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { id: "prev", onClick: () => step(-1), className: "rounded-md p-1.5 text-gray-500 hover:bg-gray-100", "aria-label": "Previous", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronLeft, { className: "h-5 w-5" }) }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { id: "next", onClick: () => step(1), className: "rounded-md p-1.5 text-gray-500 hover:bg-gray-100", "aria-label": "Next", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronRight, { className: "h-5 w-5" }) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-xl font-semibold text-gray-900", children: title }),
            view === "week" && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600", children: [
              "Week ",
              isoWeek(weekDays[1])
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "ml-auto inline-flex rounded-lg bg-gray-100 p-1", role: "tablist", children: ["week", "month"].map((v) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              "button",
              {
                id: `view-${v}`,
                role: "tab",
                "aria-selected": view === v,
                onClick: () => setView(v),
                className: `rounded-md px-3 py-1 text-sm font-medium capitalize ${view === v ? "bg-white text-gray-900 shadow-sm" : "text-gray-500 hover:text-gray-700"}`,
                children: v
              },
              v
            )) })
          ] }),
          view === "week" ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(WeekView, { anchor, events: shown }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            MonthView,
            {
              anchor,
              events: shown,
              onPickDay: (d) => {
                setAnchor(d);
                setView("week");
              }
            }
          )
        ] })
      ] }),
      showModal && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(NewEventModal, { days: weekDays, onClose: () => setShowModal(false), onSave: save }),
      toast && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "fixed bottom-6 right-6 z-40 flex w-96 items-start gap-3 rounded-xl bg-white p-4 shadow-lg ring-1 ring-black/5", role: "status", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CircleCheck, { className: "h-5 w-5 shrink-0 text-emerald-500" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-sm font-medium text-gray-900", children: "Event created" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-0.5 truncate text-sm text-gray-500", children: toast })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { onClick: () => setToast(null), className: "rounded p-0.5 text-gray-400 hover:bg-gray-100", "aria-label": "Dismiss", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(X, { className: "h-4 w-4" }) })
      ] })
    ] });
  }

  // calendar/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
