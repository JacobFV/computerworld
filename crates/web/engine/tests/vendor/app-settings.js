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

  // settings/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // settings/App.tsx
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
  var Bell = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M10.3 21a1.94 1.94 0 0 0 3.4 0" })
  ] });
  var X = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 6 6 18" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m6 6 12 12" })
  ] });
  var User = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "7", r: "4" })
  ] });
  var Lock = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "18", height: "11", x: "3", y: "11", rx: "2", ry: "2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M7 11V7a5 5 0 0 1 10 0v4" })
  ] });
  var Camera = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "13", r: "3" })
  ] });
  var CircleAlert = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "12", x2: "12", y1: "8", y2: "12" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "12", x2: "12.01", y1: "16", y2: "16" })
  ] });
  var CircleCheck = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 12 2 2 4-4" })
  ] });
  var Mail = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { width: "20", height: "16", x: "2", y: "4", rx: "2" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7" })
  ] });
  var Globe = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M2 12h20" })
  ] });

  // settings/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  var initialProfile = {
    name: "Maya Chen",
    username: "mayachen",
    email: "maya@studio.design",
    website: "",
    bio: "Product designer. Previously at Figma and Linear. I like type, trains and tidy spreadsheets.",
    timezone: "Europe/Lisbon"
  };
  var BIO_LIMIT = 160;
  function validate(p) {
    const errors = {};
    if (!p.name.trim()) errors.name = "Your name is required.";
    if (!/^[a-z0-9_]+$/i.test(p.username)) errors.username = "Usernames can only contain letters, numbers and underscores.";
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(p.email)) errors.email = "Enter a valid email address.";
    if (p.website && !/^https:\/\/\S+\.\S+$/.test(p.website)) errors.website = "Enter a URL that starts with https://";
    if (p.bio.length > BIO_LIMIT) errors.bio = `Keep your bio under ${BIO_LIMIT} characters.`;
    return errors;
  }
  var tabs = [
    { id: "profile", label: "Profile", icon: User },
    { id: "account", label: "Account", icon: Lock },
    { id: "notifications", label: "Notifications", icon: Bell }
  ];
  function Field({ label, htmlFor, hint, error, children }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("label", { htmlFor, className: "block text-sm font-medium text-gray-900", children: label }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "relative mt-2", children }),
      error ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "mt-2 flex items-center gap-1.5 text-sm text-red-600", id: `${htmlFor}-error`, children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CircleAlert, { className: "h-4 w-4 shrink-0" }),
        error
      ] }) : hint && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-2 text-sm text-gray-500", children: hint })
    ] });
  }
  function inputClass(error) {
    return `block w-full rounded-md border-0 px-3 py-2 text-sm text-gray-900 shadow-sm ring-1 ring-inset placeholder:text-gray-400 focus:ring-2 focus:ring-inset ${error ? "pr-10 text-red-900 ring-red-300 focus:ring-red-500" : "ring-gray-300 focus:ring-indigo-600"}`;
  }
  function ProfileForm({ onSaved }) {
    const [profile, setProfile] = (0, import_react.useState)(initialProfile);
    const [errors, setErrors] = (0, import_react.useState)({});
    const [submitted, setSubmitted] = (0, import_react.useState)(false);
    function update(key, value) {
      const next = { ...profile, [key]: value };
      setProfile(next);
      if (submitted) setErrors(validate(next));
    }
    function submit(e) {
      e.preventDefault();
      const found = validate(profile);
      setErrors(found);
      setSubmitted(true);
      if (Object.keys(found).length === 0) onSaved();
    }
    const errorCount = Object.keys(errors).length;
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("form", { onSubmit: submit, noValidate: true, className: "divide-y divide-gray-200 rounded-xl bg-white shadow-sm ring-1 ring-gray-900/5", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "px-8 py-6", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-gray-900", children: "Public profile" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "This information will be shown on your profile and in comments." }),
        errorCount > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-6 rounded-lg border border-red-200 bg-red-50 p-4", role: "alert", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex gap-3", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CircleAlert, { className: "h-5 w-5 shrink-0 text-red-500" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("h3", { className: "text-sm font-medium text-red-800", children: [
              "There ",
              errorCount === 1 ? "is 1 problem" : `are ${errorCount} problems`,
              " with your profile"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "mt-2 list-disc space-y-1 pl-5 text-sm text-red-700", children: Object.values(errors).map((m) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: m }, m)) })
          ] })
        ] }) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 flex items-center gap-5", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex h-16 w-16 items-center justify-center rounded-full bg-gradient-to-br from-fuchsia-500 via-purple-500 to-indigo-500 text-xl font-semibold text-white", children: "MC" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute -bottom-0.5 -right-0.5 flex h-6 w-6 items-center justify-center rounded-full bg-white text-gray-600 shadow ring-1 ring-gray-200", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Camera, { className: "h-3.5 w-3.5" }) })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex gap-3", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", className: "rounded-md bg-white px-3 py-1.5 text-sm font-medium text-gray-900 shadow-sm ring-1 ring-inset ring-gray-300 hover:bg-gray-50", children: "Change photo" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", className: "rounded-md px-3 py-1.5 text-sm font-medium text-gray-600 hover:text-gray-900", children: "Remove" })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-2 text-xs text-gray-500", children: "JPG, PNG or GIF. 1 MB max." })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-8 grid grid-cols-6 gap-x-6 gap-y-6", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "col-span-3", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Field, { label: "Full name", htmlFor: "name", error: errors.name, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { id: "name", value: profile.name, onChange: (e) => update("name", e.target.value), className: inputClass(errors.name) }) }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "col-span-3", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Field, { label: "Username", htmlFor: "username", error: errors.username, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex rounded-md shadow-sm", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "inline-flex items-center rounded-l-md border border-r-0 border-gray-300 bg-gray-50 px-3 text-sm text-gray-500", children: "studio.design/" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              "input",
              {
                id: "username",
                value: profile.username,
                onChange: (e) => update("username", e.target.value),
                className: `${inputClass(errors.username)} rounded-l-none`
              }
            )
          ] }) }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "col-span-4", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Field, { label: "Email address", htmlFor: "email", error: errors.email, hint: "We'll only use this to send you receipts.", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Mail, { className: "pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-gray-400" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { id: "email", type: "email", value: profile.email, onChange: (e) => update("email", e.target.value), className: `${inputClass(errors.email)} pl-9` })
          ] }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "col-span-2", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Field, { label: "Time zone", htmlFor: "timezone", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("select", { id: "timezone", value: profile.timezone, onChange: (e) => update("timezone", e.target.value), className: inputClass(), children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("option", { children: "Europe/Lisbon" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("option", { children: "America/New_York" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("option", { children: "Asia/Tokyo" })
          ] }) }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "col-span-6", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Field, { label: "Website", htmlFor: "website", error: errors.website, children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Globe, { className: "pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-gray-400" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
              "input",
              {
                id: "website",
                value: profile.website,
                placeholder: "https://example.com",
                onChange: (e) => update("website", e.target.value),
                className: `${inputClass(errors.website)} pl-9`
              }
            ),
            errors.website && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CircleAlert, { className: "pointer-events-none absolute right-3 top-2.5 h-4 w-4 text-red-500" })
          ] }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "col-span-6", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Field, { label: "About", htmlFor: "bio", error: errors.bio, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("textarea", { id: "bio", rows: 3, value: profile.bio, onChange: (e) => update("bio", e.target.value), className: inputClass(errors.bio) }) }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: `mt-1 text-right text-xs ${profile.bio.length > BIO_LIMIT ? "text-red-600" : "text-gray-400"}`, children: [
              profile.bio.length,
              "/",
              BIO_LIMIT
            ] })
          ] })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "sticky bottom-0 flex items-center justify-end gap-3 rounded-b-xl bg-white/90 px-8 py-4 backdrop-blur", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", className: "rounded-md px-3 py-2 text-sm font-semibold text-gray-700 hover:bg-gray-50", children: "Cancel" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "submit", id: "save", className: "rounded-md bg-indigo-600 px-4 py-2 text-sm font-semibold text-white shadow-sm hover:bg-indigo-500", children: "Save changes" })
      ] })
    ] });
  }
  function Toggle({ id, checked, onChange }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "button",
      {
        type: "button",
        role: "switch",
        id,
        "aria-checked": checked,
        onClick: () => onChange(!checked),
        className: `relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ${checked ? "bg-indigo-600" : "bg-gray-200"}`,
        children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
          "span",
          {
            className: `pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ${checked ? "translate-x-5" : "translate-x-0"}`
          }
        )
      }
    );
  }
  var notificationOptions = [
    { id: "comments", title: "Comments", body: "When someone comments on a file you own." },
    { id: "mentions", title: "Mentions", body: "When someone @mentions you anywhere." },
    { id: "digest", title: "Weekly digest", body: "A summary of activity across your projects, every Monday." },
    { id: "product", title: "Product updates", body: "Occasional news about new features." }
  ];
  function Notifications() {
    const [on, setOn] = (0, import_react.useState)({ comments: true, mentions: true, digest: false, product: false });
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rounded-xl bg-white shadow-sm ring-1 ring-gray-900/5", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "px-8 py-6", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-gray-900", children: "Email notifications" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Choose what we email you about. You can unsubscribe at any time." })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "divide-y divide-gray-100 border-t border-gray-100", children: notificationOptions.map((o) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { className: "flex items-center justify-between gap-6 px-8 py-4", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-sm font-medium text-gray-900", children: o.title }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-sm text-gray-500", children: o.body })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Toggle, { id: `toggle-${o.id}`, checked: on[o.id], onChange: (v) => setOn({ ...on, [o.id]: v }) })
      ] }, o.id)) })
    ] });
  }
  function Account() {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "space-y-6", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rounded-xl bg-white px-8 py-6 shadow-sm ring-1 ring-gray-900/5", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-gray-900", children: "Password" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Last changed 3 months ago." }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", className: "mt-4 rounded-md bg-white px-3 py-2 text-sm font-semibold text-gray-900 shadow-sm ring-1 ring-inset ring-gray-300", children: "Change password" })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rounded-xl border border-red-200 bg-white px-8 py-6", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-base font-semibold text-red-700", children: "Delete account" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Permanently remove your account and all of its content. This cannot be undone." }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", className: "mt-4 rounded-md bg-red-600 px-3 py-2 text-sm font-semibold text-white shadow-sm hover:bg-red-500", children: "Delete my account" })
      ] })
    ] });
  }
  function App() {
    const [tab, setTab] = (0, import_react.useState)("profile");
    const [toast, setToast] = (0, import_react.useState)(false);
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-h-screen bg-gray-50 font-sans", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mx-auto max-w-5xl px-8 py-10", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { className: "text-2xl font-bold tracking-tight text-gray-900", children: "Settings" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Manage your profile, account and notifications." }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-8 flex gap-10", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("nav", { className: "w-52 shrink-0 space-y-1", children: tabs.map(({ id, label, icon: Icon2 }) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
            "button",
            {
              id: `tab-${id}`,
              onClick: () => setTab(id),
              className: `flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium ${tab === id ? "bg-white text-indigo-600 shadow-sm ring-1 ring-gray-900/5" : "text-gray-600 hover:bg-gray-100 hover:text-gray-900"}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { className: `h-4 w-4 ${tab === id ? "text-indigo-600" : "text-gray-400"}` }),
                label
              ]
            },
            id
          )) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
            tab === "profile" && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ProfileForm, { onSaved: () => setToast(true) }),
            tab === "account" && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Account, {}),
            tab === "notifications" && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Notifications, {})
          ] })
        ] })
      ] }),
      toast && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "fixed bottom-6 right-6 z-50 flex w-80 items-start gap-3 rounded-xl bg-white p-4 shadow-lg ring-1 ring-black/5", role: "status", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CircleCheck, { className: "h-5 w-5 shrink-0 text-emerald-500" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex-1", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-sm font-medium text-gray-900", children: "Profile saved" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Your changes are live." })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { onClick: () => setToast(false), className: "rounded-md text-gray-400 hover:text-gray-500", "aria-label": "Dismiss", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(X, { className: "h-4 w-4" }) })
      ] })
    ] });
  }

  // settings/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
