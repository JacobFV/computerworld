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

  // music/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // music/App.tsx
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
  var ChevronLeft = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m15 18-6-6 6-6" }) });
  var ChevronRight = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 18 6-6-6-6" }) });
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
  var Clock = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "12 6 12 12 16 14" })
  ] });

  // music/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  function Icon2({ className, filled, children }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "svg",
      {
        xmlns: "http://www.w3.org/2000/svg",
        width: "24",
        height: "24",
        viewBox: "0 0 24 24",
        fill: filled ? "currentColor" : "none",
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
  var Play = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { ...p, filled: true, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("polygon", { points: "6 3 20 12 6 21 6 3" }) });
  var Pause = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, filled: true, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { x: "14", y: "4", width: "4", height: "16", rx: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { x: "6", y: "4", width: "4", height: "16", rx: "1" })
  ] });
  var SkipBack = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, filled: true, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("polygon", { points: "19 20 9 12 19 4 19 20" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("line", { x1: "5", x2: "5", y1: "19", y2: "5" })
  ] });
  var SkipForward = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, filled: true, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("polygon", { points: "5 4 15 12 5 20 5 4" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("line", { x1: "19", x2: "19", y1: "5", y2: "19" })
  ] });
  var Shuffle = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "m18 14 4 4-4 4" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "m18 2 4 4-4 4" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-7.6a4 4 0 0 1 3.3-1.7H22" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M2 6h1.972a4 4 0 0 1 3.6 2.2" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45" })
  ] });
  var Repeat = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "m17 2 4 4-4 4" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M3 11v-1a4 4 0 0 1 4-4h14" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "m7 22-4-4 4-4" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M21 13v1a4 4 0 0 1-4 4H3" })
  ] });
  var VolumeIcon = ({ level, className }) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { className, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("polygon", { points: "11 5 6 9 2 9 2 15 6 15 11 19 11 5" }),
    level === 0 ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("line", { x1: "22", x2: "16", y1: "9", y2: "15" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("line", { x1: "16", x2: "22", y1: "9", y2: "15" })
    ] }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M15.54 8.46a5 5 0 0 1 0 7.07" }),
      level > 50 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M19.07 4.93a10 10 0 0 1 0 14.14" })
    ] })
  ] });
  var ListMusic = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M21 15V6" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M18.5 18a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M12 12H3" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M16 6H3" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M12 18H3" })
  ] });
  var Library = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "m16 6 4 14" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M12 6v14" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M8 8v12" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M4 4v16" })
  ] });
  var House = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" })
  ] });
  var MonitorSpeaker = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M5.5 20H8" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M17 9h.01" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { width: "10", height: "16", x: "12", y: "4", rx: "2" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M8 6H4a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h4" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "17", cy: "15", r: "1" })
  ] });
  var Mic = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "m12 8-9.04 9.06a2.82 2.82 0 1 0 3.98 3.98L16 12" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "17", cy: "7", r: "5" })
  ] });
  var Music = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M9 18V5l12-2v13" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "6", cy: "18", r: "3" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "18", cy: "16", r: "3" })
  ] });
  var Heart = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z" }) });
  var playlists = [
    {
      id: "late-night",
      name: "Late Night Drive",
      description: "Synthwave, city pop and slow-burning grooves for empty highways after midnight.",
      owner: "Maya Chen",
      likes: "2,418",
      cover: "from-indigo-500 via-purple-600 to-pink-500",
      wash: "from-indigo-800",
      tracks: [
        { id: 101, title: "Neon Harbor", artist: "Kavya Lights", album: "Afterglow Avenue", seconds: 228 },
        { id: 102, title: "Glass Skyline", artist: "The Midnight Parade", album: "Skyline Tapes", seconds: 254 },
        { id: 103, title: "Velvet Overpass", artist: "Sora Kimura", album: "Plastic Moon", seconds: 197 },
        { id: 104, title: "Chrome Hearts Club", artist: "Delta Fontaine", album: "Night Market", seconds: 241 },
        { id: 105, title: "Tail Lights in Rain", artist: "Kavya Lights", album: "Afterglow Avenue", seconds: 276 },
        { id: 106, title: "Mirror Tunnel", artist: "Halcyon Drive", album: "Signal Loss", seconds: 213 },
        { id: 107, title: "Palm Static", artist: "Juno & The Coast", album: "Low Tide FM", seconds: 188 },
        { id: 108, title: "Last Exit to Osaka", artist: "Sora Kimura", album: "Plastic Moon", seconds: 302 },
        { id: 109, title: "Satellite Hotel", artist: "The Midnight Parade", album: "Skyline Tapes", seconds: 234 },
        { id: 110, title: "Four A.M. Diner", artist: "Delta Fontaine", album: "Night Market", seconds: 219 }
      ]
    },
    {
      id: "focus",
      name: "Deep Focus",
      description: "Ambient textures and soft piano to keep you in the zone for hours.",
      owner: "Soundwave",
      likes: "48,902",
      cover: "from-emerald-400 via-teal-500 to-cyan-700",
      wash: "from-teal-800",
      tracks: [
        { id: 201, title: "Rain on Cedar", artist: "Ólafur Brenna", album: "Quiet Rooms", seconds: 245 },
        { id: 202, title: "Paper Lanterns", artist: "Mira Ostrowski", album: "Drift", seconds: 312 },
        { id: 203, title: "Slow Orbit", artist: "Field Notes", album: "Weightless", seconds: 287 },
        { id: 204, title: "Morning Fog", artist: "Ólafur Brenna", album: "Quiet Rooms", seconds: 198 },
        { id: 205, title: "Rainfall Study No. 2", artist: "Aiko Mori", album: "Etudes for Weather", seconds: 264 },
        { id: 206, title: "Soft Machinery", artist: "Field Notes", album: "Weightless", seconds: 331 },
        { id: 207, title: "Low Light", artist: "Mira Ostrowski", album: "Drift", seconds: 223 },
        { id: 208, title: "Tidal Memory", artist: "Aiko Mori", album: "Etudes for Weather", seconds: 276 }
      ]
    },
    {
      id: "sunday",
      name: "Sunday Morning",
      description: "Warm acoustic songs for slow breakfasts and open windows.",
      owner: "Maya Chen",
      likes: "613",
      cover: "from-amber-300 via-orange-400 to-rose-500",
      wash: "from-orange-800",
      tracks: [
        { id: 301, title: "Honey & Toast", artist: "The Linden Trees", album: "Porch Songs", seconds: 186 },
        { id: 302, title: "Open Window", artist: "Clara Vale", album: "Wildflower", seconds: 214 },
        { id: 303, title: "Coffee for Two", artist: "Ben Arlo", album: "Kitchen Radio", seconds: 172 },
        { id: 304, title: "Sunlit Room", artist: "Clara Vale", album: "Wildflower", seconds: 238 },
        { id: 305, title: "Garden Path", artist: "The Linden Trees", album: "Porch Songs", seconds: 205 },
        { id: 306, title: "Lazy River", artist: "Ben Arlo", album: "Kitchen Radio", seconds: 227 }
      ]
    },
    {
      id: "run",
      name: "Run Club 170 BPM",
      description: "High-tempo tracks locked to your stride. No skips needed.",
      owner: "Soundwave",
      likes: "12,077",
      cover: "from-rose-500 via-red-500 to-orange-500",
      wash: "from-red-900",
      tracks: [
        { id: 401, title: "Redline", artist: "Volt Theory", album: "Pulse", seconds: 201 },
        { id: 402, title: "Second Wind", artist: "Nia Blaze", album: "Stride", seconds: 189 },
        { id: 403, title: "Pacesetter", artist: "Volt Theory", album: "Pulse", seconds: 214 },
        { id: 404, title: "Uphill", artist: "Kilo Echo", album: "Tempo Run", seconds: 176 },
        { id: 405, title: "Finish Line", artist: "Nia Blaze", album: "Stride", seconds: 233 }
      ]
    },
    {
      id: "discover",
      name: "Discover Weekly",
      description: "Your weekly mixtape of fresh music, picked just for you.",
      owner: "Soundwave",
      likes: "—",
      cover: "from-sky-400 via-blue-500 to-violet-600",
      wash: "from-blue-900",
      tracks: [
        { id: 501, title: "Paper Planes Home", artist: "Lumen Kid", album: "Fold", seconds: 207 },
        { id: 502, title: "Stillwater", artist: "Harper Quinn", album: "Currents", seconds: 243 },
        { id: 503, title: "Kite String", artist: "Lumen Kid", album: "Fold", seconds: 192 },
        { id: 504, title: "Blue Hour", artist: "Otis Rowe", album: "Dusk Sessions", seconds: 259 }
      ]
    }
  ];
  var allTracks = /* @__PURE__ */ new Map();
  for (const p of playlists) for (const t of p.tracks) allTracks.set(t.id, { track: t, playlist: p });
  var formatTime = (s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  function formatTotal(seconds) {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor(seconds % 3600 / 60);
    return h > 0 ? `${h} hr ${m} min` : `${m} min ${seconds % 60} sec`;
  }
  function Cover({ playlist, size }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "div",
      {
        className: `flex shrink-0 items-center justify-center rounded-md bg-gradient-to-br ${playlist.cover} ${size}`,
        children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Music, { className: "h-1/2 w-1/2 text-white/80" })
      }
    );
  }
  function Equalizer() {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "inline-flex h-3.5 items-end gap-0.5", "aria-label": "Now playing", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "h-2 w-[3px] rounded-sm bg-emerald-400" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "h-3.5 w-[3px] rounded-sm bg-emerald-400" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "h-1.5 w-[3px] rounded-sm bg-emerald-400" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "h-3 w-[3px] rounded-sm bg-emerald-400" })
    ] });
  }
  function Sidebar({ activeId, onSelect }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("aside", { className: "flex w-72 shrink-0 flex-col gap-2", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("nav", { className: "rounded-lg bg-neutral-900 px-3 py-2", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("a", { href: "#", className: "flex items-center gap-4 rounded-md px-3 py-2.5 text-sm font-semibold text-white", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(House, { className: "h-6 w-6" }),
          "Home"
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("a", { href: "#", className: "flex items-center gap-4 rounded-md px-3 py-2.5 text-sm font-semibold text-neutral-400", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "h-6 w-6" }),
          "Search"
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("section", { className: "flex min-h-0 flex-1 flex-col rounded-lg bg-neutral-900", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("header", { className: "flex items-center justify-between px-6 pb-2 pt-4", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("h2", { className: "flex items-center gap-3 text-sm font-semibold text-neutral-300", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Library, { className: "h-6 w-6" }),
            "Your Library"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "button",
            {
              type: "button",
              "aria-label": "Create playlist",
              className: "flex h-8 w-8 items-center justify-center rounded-full text-neutral-400",
              children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Plus, { className: "h-5 w-5" })
            }
          )
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex gap-2 px-4 py-2", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rounded-full bg-white px-3 py-1 text-xs font-medium text-black", children: "Playlists" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rounded-full bg-neutral-800 px-3 py-1 text-xs font-medium text-white", children: "Artists" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rounded-full bg-neutral-800 px-3 py-1 text-xs font-medium text-white", children: "Albums" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("ul", { className: "min-h-0 flex-1 space-y-0.5 overflow-y-auto px-2 pb-2", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3 rounded-md p-2", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex h-12 w-12 shrink-0 items-center justify-center rounded-md bg-gradient-to-br from-violet-700 to-sky-300", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Heart, { className: "h-5 w-5 fill-white text-white" }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-sm font-medium text-white", children: "Liked Songs" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-xs text-neutral-400", children: "Playlist · 214 songs" })
            ] })
          ] }) }),
          playlists.map((p) => {
            const active = p.id === activeId;
            return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
              "button",
              {
                type: "button",
                "data-playlist": p.id,
                onClick: () => onSelect(p.id),
                className: `flex w-full items-center gap-3 rounded-md p-2 text-left ${active ? "bg-white/10" : ""}`,
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Cover, { playlist: p, size: "h-12 w-12" }),
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: `truncate text-sm font-medium ${active ? "text-emerald-400" : "text-white"}`, children: p.name }),
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "truncate text-xs text-neutral-400", children: [
                      "Playlist · ",
                      p.owner
                    ] })
                  ] })
                ]
              }
            ) }, p.id);
          })
        ] })
      ] })
    ] });
  }
  function VolumeSlider({ value, onChange }) {
    const onClick = (e) => {
      const rect = e.currentTarget.getBoundingClientRect();
      const fraction = (e.clientX - rect.left) / rect.width;
      onChange(Math.max(0, Math.min(100, Math.round(fraction * 20) * 5)));
    };
    const onKeyDown = (e) => {
      if (e.key === "ArrowRight" || e.key === "ArrowUp") onChange(Math.min(100, value + 10));
      else if (e.key === "ArrowLeft" || e.key === "ArrowDown") onChange(Math.max(0, value - 10));
      else return;
      e.preventDefault();
    };
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "div",
      {
        id: "volume",
        role: "slider",
        tabIndex: 0,
        "aria-label": "Volume",
        "aria-valuemin": 0,
        "aria-valuemax": 100,
        "aria-valuenow": value,
        onClick,
        onKeyDown,
        className: "group flex h-4 w-28 cursor-pointer items-center outline-none",
        children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative h-1 w-full rounded-full bg-neutral-600", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "absolute inset-y-0 left-0 rounded-full bg-emerald-400", style: { width: `${value}%` } }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "div",
            {
              className: "absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow",
              style: { left: `${value}%` }
            }
          )
        ] })
      }
    );
  }
  function App() {
    const [playlistId, setPlaylistId] = (0, import_react.useState)("late-night");
    const [currentId, setCurrentId] = (0, import_react.useState)(101);
    const [playing, setPlaying] = (0, import_react.useState)(false);
    const [position, setPosition] = (0, import_react.useState)(83);
    const [liked, setLiked] = (0, import_react.useState)(() => /* @__PURE__ */ new Set([102, 105, 203]));
    const [queueOpen, setQueueOpen] = (0, import_react.useState)(false);
    const [volume, setVolume] = (0, import_react.useState)(70);
    const [filter, setFilter] = (0, import_react.useState)("");
    const playlist = playlists.find((p) => p.id === playlistId);
    const current = allTracks.get(currentId);
    const visible = (0, import_react.useMemo)(() => {
      const q = filter.trim().toLowerCase();
      if (!q) return playlist.tracks;
      return playlist.tracks.filter(
        (t) => [t.title, t.artist, t.album].some((field) => field.toLowerCase().includes(q))
      );
    }, [playlist, filter]);
    const totalSeconds = playlist.tracks.reduce((sum, t) => sum + t.seconds, 0);
    const playlistIsPlaying = playing && current.playlist.id === playlist.id;
    const play = (id) => {
      if (id === currentId) {
        setPlaying((p) => !p);
        return;
      }
      setCurrentId(id);
      setPosition(0);
      setPlaying(true);
    };
    const toggleLike = (id) => setLiked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    const selectPlaylist = (id) => {
      setPlaylistId(id);
      setFilter("");
    };
    const playAlbum = () => {
      if (current.playlist.id === playlist.id) setPlaying((p) => !p);
      else play(playlist.tracks[0].id);
    };
    const step = (delta) => {
      const list = current.playlist.tracks;
      const i = list.findIndex((t) => t.id === currentId);
      const next = list[(i + delta + list.length) % list.length];
      setCurrentId(next.id);
      setPosition(0);
      setPlaying(true);
    };
    const upNext = (() => {
      const list = current.playlist.tracks;
      const i = list.findIndex((t) => t.id === currentId);
      return list.slice(i + 1).concat(list.slice(0, i));
    })();
    const progress = position / current.track.seconds * 100;
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex h-screen flex-col overflow-hidden bg-black font-sans text-white antialiased", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex min-h-0 flex-1 gap-2 p-2 pb-0", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Sidebar, { activeId: playlistId, onSelect: selectPlaylist }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("main", { className: "relative min-w-0 flex-1 overflow-y-auto rounded-lg bg-neutral-900", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: `bg-gradient-to-b ${playlist.wash} to-neutral-900 px-6 pb-6 pt-4`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex gap-2", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", "aria-label": "Back", className: "flex h-8 w-8 items-center justify-center rounded-full bg-black/40", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronLeft, { className: "h-5 w-5" }) }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", "aria-label": "Forward", className: "flex h-8 w-8 items-center justify-center rounded-full bg-black/40 text-neutral-400", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronRight, { className: "h-5 w-5" }) })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-2 rounded-full bg-black/40 p-0.5 pr-3", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "flex h-7 w-7 items-center justify-center rounded-full bg-gradient-to-br from-pink-500 to-orange-400 text-xs font-bold", children: "MC" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-sm font-semibold", children: "Maya Chen" })
              ] })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 flex items-end gap-6", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: `flex h-48 w-48 shrink-0 items-center justify-center rounded-md bg-gradient-to-br ${playlist.cover} shadow-2xl shadow-black/50`, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Music, { className: "h-20 w-20 text-white/80" }) }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 pb-1", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-xs font-semibold uppercase tracking-wider", children: "Playlist" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { id: "playlist-title", className: "mt-2 truncate text-6xl font-bold tracking-tight", children: playlist.name }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-4 max-w-xl text-sm text-white/70", children: playlist.description }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "mt-2 text-sm", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "font-semibold", children: playlist.owner }),
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "text-white/70", children: [
                    " ",
                    "· ",
                    playlist.likes,
                    " saves · ",
                    playlist.tracks.length,
                    " songs, ",
                    formatTotal(totalSeconds)
                  ] })
                ] })
              ] })
            ] })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-6 px-6 py-4", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              "button",
              {
                id: "play-album",
                type: "button",
                "aria-label": playlistIsPlaying ? "Pause" : "Play",
                onClick: playAlbum,
                className: "flex h-14 w-14 items-center justify-center rounded-full bg-emerald-400 text-black shadow-lg",
                children: playlistIsPlaying ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Pause, { className: "h-6 w-6" }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Play, { className: "ml-0.5 h-6 w-6" })
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Shuffle, { className: "h-7 w-7 text-neutral-400" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Heart, { className: "h-7 w-7 text-neutral-400" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Ellipsis, { className: "h-7 w-7 text-neutral-400" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("label", { className: "ml-auto flex w-56 items-center gap-2 rounded-md bg-white/10 px-3 py-1.5 text-sm text-neutral-400 focus-within:ring-2 focus-within:ring-white/30", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "h-4 w-4 shrink-0" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                "input",
                {
                  id: "filter-tracks",
                  value: filter,
                  onChange: (e) => setFilter(e.target.value),
                  onKeyDown: (e) => {
                    if (e.key === "Enter" && visible.length > 0) play(visible[0].id);
                  },
                  placeholder: "Search in playlist",
                  className: "w-full bg-transparent text-white placeholder-neutral-400 outline-none"
                }
              )
            ] })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "px-6 pb-8", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("table", { className: "w-full table-fixed text-left text-sm", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { className: "border-b border-white/10 text-xs uppercase tracking-wider text-neutral-400", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("th", { className: "w-12 py-2 pl-4 font-normal", children: "#" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("th", { className: "py-2 font-normal", children: "Title" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("th", { className: "w-[22%] py-2 font-normal", children: "Artist" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("th", { className: "w-[22%] py-2 font-normal", children: "Album" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("th", { className: "w-12 py-2 font-normal", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "sr-only", children: "Liked" }) }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("th", { className: "w-20 py-2 pr-4 font-normal", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Clock, { className: "ml-auto h-4 w-4" }) })
              ] }) }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tbody", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("tr", { "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { colSpan: 6, className: "h-2" }) }),
                visible.map((t) => {
                  const index = playlist.tracks.indexOf(t) + 1;
                  const isCurrent = t.id === currentId;
                  const isLiked = liked.has(t.id);
                  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
                    "tr",
                    {
                      "data-track": t.id,
                      onClick: () => play(t.id),
                      className: `cursor-pointer ${isCurrent ? "bg-white/10" : ""}`,
                      children: [
                        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "rounded-l-md py-2 pl-4 tabular-nums text-neutral-400", children: isCurrent && playing ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Equalizer, {}) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: isCurrent ? "text-emerald-400" : "", children: index }) }),
                        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "py-2 pr-4", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3", children: [
                          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Cover, { playlist, size: "h-10 w-10" }),
                          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0", children: [
                            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: `truncate font-medium ${isCurrent ? "text-emerald-400" : "text-white"}`, children: t.title }),
                            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-xs text-neutral-400", children: t.artist })
                          ] })
                        ] }) }),
                        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "truncate py-2 pr-4 text-neutral-400", children: t.artist }),
                        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "truncate py-2 pr-4 text-neutral-400", children: t.album }),
                        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "py-2", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                          "button",
                          {
                            type: "button",
                            "data-like": t.id,
                            "aria-pressed": isLiked,
                            "aria-label": isLiked ? `Remove ${t.title} from Liked Songs` : `Save ${t.title} to Liked Songs`,
                            onClick: (e) => {
                              e.stopPropagation();
                              toggleLike(t.id);
                            },
                            className: `flex h-8 w-8 items-center justify-center ${isLiked ? "text-emerald-400" : "text-neutral-600"}`,
                            children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Heart, { className: `h-4 w-4 ${isLiked ? "fill-emerald-400" : ""}` })
                          }
                        ) }),
                        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { className: "rounded-r-md py-2 pr-4 text-right tabular-nums text-neutral-400", children: formatTime(t.seconds) })
                      ]
                    },
                    t.id
                  );
                })
              ] })
            ] }),
            filter && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "mt-4 text-xs text-neutral-400", children: [
              visible.length,
              " of ",
              playlist.tracks.length,
              " songs match “",
              filter,
              "”"
            ] })
          ] })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("footer", { className: "grid h-[88px] shrink-0 grid-cols-[1fr_minmax(0,2fr)_1fr] items-center gap-4 px-4", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex min-w-0 items-center gap-3", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Cover, { playlist: current.playlist, size: "h-14 w-14" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { id: "now-playing-title", className: "truncate text-sm font-medium text-white", children: current.track.title }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-xs text-neutral-400", children: current.track.artist })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "button",
            {
              type: "button",
              "data-like": `bar-${current.track.id}`,
              "aria-label": "Save to Liked Songs",
              onClick: () => toggleLike(current.track.id),
              className: `ml-2 shrink-0 ${liked.has(current.track.id) ? "text-emerald-400" : "text-neutral-400"}`,
              children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Heart, { className: `h-4 w-4 ${liked.has(current.track.id) ? "fill-emerald-400" : ""}` })
            }
          )
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex flex-col items-center gap-2", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-6", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", "aria-label": "Shuffle", className: "text-neutral-400", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Shuffle, { className: "h-4 w-4" }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { id: "prev", type: "button", "aria-label": "Previous", onClick: () => step(-1), className: "text-neutral-300", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(SkipBack, { className: "h-4 w-4" }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              "button",
              {
                id: "play-pause",
                type: "button",
                "aria-label": playing ? "Pause" : "Play",
                onClick: () => setPlaying((p) => !p),
                className: "flex h-8 w-8 items-center justify-center rounded-full bg-white text-black",
                children: playing ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Pause, { className: "h-4 w-4" }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Play, { className: "ml-0.5 h-4 w-4" })
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { id: "next", type: "button", "aria-label": "Next", onClick: () => step(1), className: "text-neutral-300", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(SkipForward, { className: "h-4 w-4" }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", "aria-label": "Repeat", className: "text-emerald-400", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Repeat, { className: "h-4 w-4" }) })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex w-full max-w-xl items-center gap-2 text-[11px] tabular-nums text-neutral-400", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "w-10 text-right", children: formatTime(position) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "relative h-1 flex-1 rounded-full bg-neutral-600", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "absolute inset-y-0 left-0 rounded-full bg-white", style: { width: `${progress}%` } }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "w-10", children: formatTime(current.track.seconds) })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-end gap-4 text-neutral-400", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Mic, { className: "h-4 w-4" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
            "button",
            {
              id: "queue-toggle",
              type: "button",
              "aria-label": "Queue",
              "aria-pressed": queueOpen,
              onClick: () => setQueueOpen((o) => !o),
              className: `relative ${queueOpen ? "text-emerald-400" : ""}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ListMusic, { className: "h-4 w-4" }),
                queueOpen && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute -bottom-2 left-1/2 h-1 w-1 -translate-x-1/2 rounded-full bg-emerald-400" })
              ]
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(MonitorSpeaker, { className: "h-4 w-4" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-2", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(VolumeIcon, { level: volume, className: "h-4 w-4" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(VolumeSlider, { value: volume, onChange: setVolume })
          ] })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
        "aside",
        {
          id: "queue",
          "aria-hidden": !queueOpen,
          className: `fixed bottom-[88px] right-2 top-2 flex w-80 flex-col rounded-lg border border-white/10 bg-neutral-900 shadow-2xl shadow-black transition-transform duration-300 ease-out ${queueOpen ? "translate-x-0" : "translate-x-[110%]"}`,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("header", { className: "flex items-center justify-between px-4 pb-2 pt-4", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-bold", children: "Queue" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                "button",
                {
                  id: "queue-close",
                  type: "button",
                  "aria-label": "Close queue",
                  onClick: () => setQueueOpen(false),
                  className: "flex h-8 w-8 items-center justify-center rounded-full text-neutral-400",
                  children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(X, { className: "h-4 w-4" })
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-h-0 flex-1 overflow-y-auto px-2 pb-4", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h3", { className: "px-2 pb-2 pt-2 text-sm font-bold", children: "Now playing" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3 rounded-md bg-white/5 p-2", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Cover, { playlist: current.playlist, size: "h-10 w-10" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-sm font-medium text-emerald-400", children: current.track.title }),
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-xs text-neutral-400", children: current.track.artist })
                ] }),
                playing && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Equalizer, {})
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("h3", { className: "px-2 pb-2 pt-5 text-sm font-bold", children: [
                "Next from: ",
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-neutral-400", children: current.playlist.name })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "space-y-0.5", children: upNext.map((t) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
                "button",
                {
                  type: "button",
                  "data-queue": t.id,
                  onClick: () => play(t.id),
                  className: "flex w-full items-center gap-3 rounded-md p-2 text-left",
                  children: [
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Cover, { playlist: current.playlist, size: "h-10 w-10" }),
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-sm font-medium text-white", children: t.title }),
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-xs text-neutral-400", children: t.artist })
                    ] }),
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-xs tabular-nums text-neutral-500", children: formatTime(t.seconds) })
                  ]
                }
              ) }, t.id)) })
            ] })
          ]
        }
      )
    ] });
  }

  // music/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
