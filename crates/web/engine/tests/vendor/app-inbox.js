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

  // inbox/main.tsx
  var import_react2 = __toESM(require_react());
  var import_client = __toESM(require_client());

  // inbox/App.tsx
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
  var ChevronLeft = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m15 18-6-6 6-6" }) });
  var ChevronRight = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 18 6-6-6-6" }) });
  var EllipsisVertical = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "5", r: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "19", r: "1" })
  ] });
  var X = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 6 6 18" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m6 6 12 12" })
  ] });
  var Paperclip = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" }) });
  var CircleAlert = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "12", x2: "12", y1: "8", y2: "12" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", { x1: "12", x2: "12.01", y1: "16", y2: "16" })
  ] });
  var Send = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m21.854 2.147-10.94 10.939" })
  ] });
  var Star = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { ...p, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" }) });
  var Inbox = (p) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Icon, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", { points: "22 12 16 12 14 15 10 15 8 12 2 12" }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" })
  ] });

  // inbox/App.tsx
  var import_jsx_runtime2 = __toESM(require_jsx_runtime());
  function Icon2({ className, children }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
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
  var Archive = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { width: "20", height: "5", x: "2", y: "3", rx: "1" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M10 12h4" })
  ] });
  var Trash = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M3 6h18" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" })
  ] });
  var FileText = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M14 2v4a2 2 0 0 0 2 2h4" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M10 9H8" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M16 13H8" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M16 17H8" })
  ] });
  var OctagonAlert = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M12 16h.01" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M12 8v4" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M15.312 2a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586l-4.688-4.688A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2z" })
  ] });
  var Reply = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("polyline", { points: "9 17 4 12 9 7" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M20 18v-2a4 4 0 0 0-4-4H4" })
  ] });
  var Forward = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("polyline", { points: "15 17 20 12 15 7" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M4 18v-2a4 4 0 0 1 4-4h12" })
  ] });
  var MailOpen = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M21.2 8.4c.5.38.8.97.8 1.6v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V10a2 2 0 0 1 .8-1.6l8-6a2 2 0 0 1 2.4 0l8 6Z" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "m22 10-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 10" })
  ] });
  var PenLine = (p) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Icon2, { ...p, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M12 20h9" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M16.376 3.622a1 1 0 0 1 3.002 3.002L7.368 18.635a2 2 0 0 1-.855.506l-2.872.838a.5.5 0 0 1-.62-.62l.838-2.872a2 2 0 0 1 .506-.854z" })
  ] });
  var initialMessages = [
    {
      id: 1,
      folder: "inbox",
      from: "Priya Natarajan",
      email: "priya@northwind.io",
      initials: "PN",
      gradient: "from-violet-500 to-fuchsia-500",
      subject: "Kickoff notes and next steps for the Q4 redesign",
      body: [
        "Hi Sam,",
        "Thanks again for running such a focused kickoff yesterday. I pulled together the notes and the decisions we landed on so everyone is working from the same page.",
        "The short version: we keep the current navigation for launch, move the pricing experiment to November, and the design team owns the new onboarding flow end to end. Engineering will scope the account settings rework by Friday.",
        "Could you confirm the review dates on your side? I pencilled in Tuesday the 8th for the first design review and would love to lock it before the calendars fill up.",
        "Best,\nPriya"
      ],
      time: "9:41 AM",
      date: "Tue, Sep 24, 9:41 AM",
      unread: false,
      starred: true,
      label: "Clients"
    },
    {
      id: 2,
      folder: "inbox",
      from: "Stripe",
      email: "receipts@stripe.com",
      initials: "S",
      gradient: "from-indigo-500 to-sky-500",
      subject: "Your invoice #INV-20417 from Acme Cloud is ready",
      body: [
        "Hello,",
        "Your invoice #INV-20417 for September is now available. The total of $1,248.00 will be charged to the Visa ending in 4242 on October 1.",
        "You can download the PDF below or view the itemised breakdown in your billing dashboard at any time.",
        "Thanks for your business,\nThe Acme Cloud team"
      ],
      time: "8:15 AM",
      date: "Tue, Sep 24, 8:15 AM",
      unread: true,
      starred: false,
      label: "Finance",
      attachment: { name: "INV-20417.pdf", size: "184 KB" }
    },
    {
      id: 3,
      folder: "inbox",
      from: "Marcus Webb",
      email: "marcus.webb@hey.com",
      initials: "MW",
      gradient: "from-emerald-500 to-teal-400",
      subject: "Re: Photos from the offsite",
      body: [
        "Hey Sam!",
        "Finally got through the photos from the offsite. There are some great ones of the hike and the dinner on the last night. I uploaded everything to the shared album so grab whatever you want for the recap post.",
        "Also, the team voted and we are doing the lake house again next spring. Start practising your paddleboarding.",
        "Cheers,\nMarcus"
      ],
      time: "7:02 AM",
      date: "Tue, Sep 24, 7:02 AM",
      unread: true,
      starred: false,
      label: "Team"
    },
    {
      id: 4,
      folder: "inbox",
      from: "Elena García",
      email: "elena@studiofolk.com",
      initials: "EG",
      gradient: "from-pink-500 to-orange-400",
      subject: "Contract draft for review",
      body: [
        "Hi Sam,",
        "Attached is the revised contract with the changes from our call. The main edits are in sections 4 (payment schedule) and 7 (IP ownership). Let me know if legal has any questions.",
        "Elena"
      ],
      time: "Yesterday",
      date: "Mon, Sep 23, 4:37 PM",
      unread: true,
      starred: true,
      label: "Clients",
      attachment: { name: "Studio-Folk-MSA-v3.docx", size: "92 KB" }
    },
    {
      id: 5,
      folder: "inbox",
      from: "GitHub",
      email: "noreply@github.com",
      initials: "GH",
      gradient: "from-gray-700 to-gray-500",
      subject: "[acme/web] Release v2.14.0 published",
      body: ["A new release v2.14.0 was published by jkato. It includes 23 merged pull requests and fixes for the checkout flow on Safari."],
      time: "Yesterday",
      date: "Mon, Sep 23, 2:10 PM",
      unread: false,
      starred: false
    },
    {
      id: 6,
      folder: "inbox",
      from: "Harbor Accounting",
      email: "billing@harbor.co",
      initials: "HA",
      gradient: "from-amber-500 to-yellow-400",
      subject: "Overdue invoice reminder: August retainer",
      body: [
        "Hi Sam,",
        "A friendly reminder that invoice #H-0893 for the August retainer is now 14 days overdue. Please arrange payment at your earliest convenience or reply if anything looks off.",
        "Kind regards,\nHarbor Accounting"
      ],
      time: "Sep 22",
      date: "Sun, Sep 22, 10:03 AM",
      unread: true,
      starred: false,
      label: "Finance"
    },
    {
      id: 7,
      folder: "inbox",
      from: "Jun Kato",
      email: "jun@acme.dev",
      initials: "JK",
      gradient: "from-sky-500 to-indigo-500",
      subject: "Standup moved to 10:30 tomorrow",
      body: ["Quick heads up: the design crit ran long so standup is at 10:30 tomorrow instead of 10. Same room."],
      time: "Sep 21",
      date: "Sat, Sep 21, 6:48 PM",
      unread: false,
      starred: false,
      label: "Team"
    },
    {
      id: 8,
      folder: "inbox",
      from: "Figma",
      email: "no-reply@figma.com",
      initials: "F",
      gradient: "from-rose-500 to-pink-500",
      subject: "Ava Reyes commented on Onboarding v4",
      body: ['Ava Reyes left a comment: "Can we try the illustration on the left and shorten the headline to one line?"'],
      time: "Sep 20",
      date: "Fri, Sep 20, 11:26 AM",
      unread: false,
      starred: false
    },
    {
      id: 9,
      folder: "inbox",
      from: "Linear",
      email: "notifications@linear.app",
      initials: "L",
      gradient: "from-indigo-600 to-violet-500",
      subject: "Weekly summary: 18 issues closed",
      body: ["Your team closed 18 issues and opened 11 this week. Cycle 14 is 72% complete with 4 days remaining."],
      time: "Sep 19",
      date: "Thu, Sep 19, 9:00 AM",
      unread: false,
      starred: false
    },
    {
      id: 10,
      folder: "sent",
      from: "Sam Carter",
      email: "sam@acme.dev",
      initials: "SC",
      gradient: "from-teal-500 to-emerald-400",
      subject: "Invoice question for September",
      body: ["Hi, could you resend the September invoice with our new billing address? Thanks!"],
      time: "Sep 18",
      date: "Wed, Sep 18, 3:12 PM",
      unread: false,
      starred: false,
      label: "Finance"
    },
    {
      id: 11,
      folder: "drafts",
      from: "Sam Carter",
      email: "sam@acme.dev",
      initials: "SC",
      gradient: "from-teal-500 to-emerald-400",
      subject: "Offsite budget proposal",
      body: ["Draft: rough numbers for spring offsite, still waiting on the venue quote."],
      time: "Sep 17",
      date: "Tue, Sep 17, 5:40 PM",
      unread: false,
      starred: false
    },
    {
      id: 12,
      folder: "spam",
      from: "Prize Center",
      email: "win@prize-center.biz",
      initials: "PC",
      gradient: "from-lime-500 to-green-500",
      subject: "You have been selected!!!",
      body: ["Claim your reward today."],
      time: "Sep 16",
      date: "Mon, Sep 16, 1:01 AM",
      unread: true,
      starred: false
    }
  ];
  var folders = [
    { id: "inbox", name: "Inbox", icon: Inbox },
    { id: "starred", name: "Starred", icon: Star },
    { id: "sent", name: "Sent", icon: Send },
    { id: "drafts", name: "Drafts", icon: FileText },
    { id: "archive", name: "Archive", icon: Archive },
    { id: "spam", name: "Spam", icon: OctagonAlert },
    { id: "trash", name: "Trash", icon: Trash }
  ];
  var labelStyles = {
    Clients: { dot: "bg-violet-500", chip: "bg-violet-50 text-violet-700 ring-violet-600/20" },
    Finance: { dot: "bg-emerald-500", chip: "bg-emerald-50 text-emerald-700 ring-emerald-600/20" },
    Team: { dot: "bg-sky-500", chip: "bg-sky-50 text-sky-700 ring-sky-600/20" }
  };
  function inFolder(m, folder) {
    return folder === "starred" ? m.starred && m.folder !== "spam" && m.folder !== "trash" : m.folder === folder;
  }
  function Avatar({ m, size = "h-9 w-9 text-xs" }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "span",
      {
        className: `inline-flex ${size} shrink-0 items-center justify-center rounded-full bg-gradient-to-br ${m.gradient} font-semibold text-white`,
        children: m.initials
      }
    );
  }
  function StarButton({ m, onToggle }) {
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "button",
      {
        type: "button",
        "data-star": m.id,
        "aria-label": m.starred ? "Unstar" : "Star",
        onClick: (e) => {
          e.stopPropagation();
          onToggle(m.id);
        },
        className: "rounded p-0.5 hover:bg-gray-100",
        children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Star, { className: `h-4 w-4 ${m.starred ? "fill-amber-400 text-amber-400" : "text-gray-300 hover:text-gray-400"}` })
      }
    );
  }
  function validate(d) {
    const errors = {};
    if (!d.to.trim()) errors.to = "Add at least one recipient.";
    else if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(d.to.trim())) errors.to = `"${d.to.trim()}" is not a valid email address.`;
    if (!d.subject.trim()) errors.subject = "Subject is required.";
    if (d.body.trim().length < 5) errors.body = "Write a message before sending.";
    return errors;
  }
  function ComposeModal({ onClose, onSend }) {
    const [draft, setDraft] = (0, import_react.useState)({ to: "", subject: "", body: "" });
    const [submitted, setSubmitted] = (0, import_react.useState)(false);
    const errors = submitted ? validate(draft) : {};
    const count = Object.keys(errors).length;
    function submit(e) {
      e.preventDefault();
      setSubmitted(true);
      if (Object.keys(validate(draft)).length === 0) onSend(draft);
    }
    const field = (key) => (e) => setDraft((d) => ({ ...d, [key]: e.target.value }));
    const border = (key) => errors[key] ? "border-rose-300 focus:ring-rose-500/30" : "border-gray-300 focus:border-indigo-500 focus:ring-indigo-500/30";
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "fixed inset-0 z-50 flex items-center justify-center p-4", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "absolute inset-0 bg-gray-900/40 backdrop-blur-sm", onClick: onClose }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
        "form",
        {
          onSubmit: submit,
          noValidate: true,
          role: "dialog",
          "aria-modal": "true",
          "aria-labelledby": "compose-title",
          className: "relative w-full max-w-xl rounded-2xl bg-white shadow-2xl ring-1 ring-gray-900/5",
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between border-b border-gray-100 px-6 py-4", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { id: "compose-title", className: "text-base font-semibold text-gray-900", children: "New message" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", onClick: onClose, className: "rounded-md p-1 text-gray-400 hover:bg-gray-100", "aria-label": "Close", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(X, { className: "h-5 w-5" }) })
            ] }),
            count > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mx-6 mt-5 flex items-start gap-2.5 rounded-lg bg-rose-50 px-3.5 py-3 text-sm text-rose-800 ring-1 ring-inset ring-rose-200", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(CircleAlert, { className: "mt-0.5 h-4 w-4 shrink-0 text-rose-500" }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { children: [
                count === 1 ? "There is 1 problem" : `There are ${count} problems`,
                " with this message. Fix ",
                count === 1 ? "it" : "them",
                " and try again."
              ] })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "space-y-4 px-6 py-5", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("label", { htmlFor: "compose-to", className: "block text-sm font-medium text-gray-700", children: "To" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                  "input",
                  {
                    id: "compose-to",
                    type: "email",
                    autoComplete: "off",
                    value: draft.to,
                    onChange: field("to"),
                    placeholder: "name@company.com",
                    className: `mt-1.5 block w-full rounded-lg border px-3 py-2 text-sm shadow-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 ${border("to")}`
                  }
                ),
                errors.to && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1.5 text-xs text-rose-600", children: errors.to })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("label", { htmlFor: "compose-subject", className: "block text-sm font-medium text-gray-700", children: "Subject" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                  "input",
                  {
                    id: "compose-subject",
                    autoComplete: "off",
                    value: draft.subject,
                    onChange: field("subject"),
                    placeholder: "What is this about?",
                    className: `mt-1.5 block w-full rounded-lg border px-3 py-2 text-sm shadow-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 ${border("subject")}`
                  }
                ),
                errors.subject && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1.5 text-xs text-rose-600", children: errors.subject })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("label", { htmlFor: "compose-body", className: "block text-sm font-medium text-gray-700", children: "Message" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                  "textarea",
                  {
                    id: "compose-body",
                    rows: 6,
                    value: draft.body,
                    onChange: field("body"),
                    placeholder: "Write your message…",
                    className: `mt-1.5 block w-full resize-none rounded-lg border px-3 py-2 text-sm shadow-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 ${border("body")}`
                  }
                ),
                errors.body && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1.5 text-xs text-rose-600", children: errors.body })
              ] })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center justify-between rounded-b-2xl bg-gray-50 px-6 py-4", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { type: "button", className: "inline-flex items-center gap-1.5 rounded-lg px-2 py-2 text-sm text-gray-500 hover:bg-gray-100", "aria-label": "Attach a file", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Paperclip, { className: "h-4 w-4" }),
                "Attach"
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex gap-3", children: [
                /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", onClick: onClose, className: "rounded-lg px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100", children: "Discard" }),
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
                  "button",
                  {
                    type: "submit",
                    id: "compose-send",
                    className: "inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500",
                    children: [
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Send, { className: "h-4 w-4" }),
                      "Send"
                    ]
                  }
                )
              ] })
            ] })
          ]
        }
      )
    ] });
  }
  function ReadingPane({ m, position, total, onToggleStar }) {
    if (!m) {
      return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex flex-1 flex-col items-center justify-center bg-white text-center", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "flex h-12 w-12 items-center justify-center rounded-full bg-gray-100", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(MailOpen, { className: "h-6 w-6 text-gray-400" }) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-3 text-sm font-medium text-gray-900", children: "No message selected" }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-sm text-gray-500", children: "Choose a conversation from the list to read it here." })
      ] });
    }
    const toolbar = [
      { label: "Archive", icon: Archive },
      { label: "Report spam", icon: OctagonAlert },
      { label: "Delete", icon: Trash },
      { label: "Mark as unread", icon: MailOpen }
    ];
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("section", { className: "flex min-w-0 flex-1 flex-col bg-white", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex h-14 shrink-0 items-center justify-between border-b border-gray-200 px-6", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex items-center gap-1", children: toolbar.map(({ label, icon: I }) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { "aria-label": label, className: "rounded-md p-2 text-gray-500 hover:bg-gray-100 hover:text-gray-700", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(I, { className: "h-4 w-4" }) }, label)) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-1 text-sm text-gray-500", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "mr-2", children: [
            position,
            " of ",
            total
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { "aria-label": "Newer", className: "rounded-md p-2 hover:bg-gray-100", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronLeft, { className: "h-4 w-4" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { "aria-label": "Older", className: "rounded-md p-2 hover:bg-gray-100", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ChevronRight, { className: "h-4 w-4" }) })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex-1 overflow-y-auto px-8 py-6", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-start justify-between gap-4", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "text-xl font-semibold leading-snug text-gray-900", children: m.subject }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(StarButton, { m, onToggle: onToggleStar })
        ] }),
        m.label && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `mt-2 inline-flex items-center rounded-md px-2 py-0.5 text-xs font-medium ring-1 ring-inset ${labelStyles[m.label].chip}`, children: m.label }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 flex items-start gap-3", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Avatar, { m, size: "h-10 w-10 text-sm" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-baseline justify-between gap-3", children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("p", { className: "truncate text-sm font-semibold text-gray-900", children: [
                m.from,
                " ",
                /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "font-normal text-gray-500", children: [
                  "<",
                  m.email,
                  ">"
                ] })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "shrink-0 text-xs text-gray-500", children: m.date })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-0.5 text-xs text-gray-500", children: "to me" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { "aria-label": "More", className: "rounded-md p-1 text-gray-400 hover:bg-gray-100", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(EllipsisVertical, { className: "h-4 w-4" }) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "mt-6 space-y-4 text-sm leading-relaxed text-gray-700", children: m.body.map((p, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "whitespace-pre-line", children: p }, i)) }),
        m.attachment && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-6 flex w-72 items-center gap-3 rounded-lg border border-gray-200 p-3", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "flex h-10 w-10 items-center justify-center rounded-md bg-rose-50", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(FileText, { className: "h-5 w-5 text-rose-500" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-sm font-medium text-gray-900", children: m.attachment.name }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "text-xs text-gray-500", children: m.attachment.size })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-8 flex gap-2", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "inline-flex items-center gap-1.5 rounded-lg border border-gray-300 px-3.5 py-2 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Reply, { className: "h-4 w-4" }),
            "Reply"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { className: "inline-flex items-center gap-1.5 rounded-lg border border-gray-300 px-3.5 py-2 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Forward, { className: "h-4 w-4" }),
            "Forward"
          ] })
        ] })
      ] })
    ] });
  }
  function App() {
    const [messages, setMessages] = (0, import_react.useState)(initialMessages);
    const [folder, setFolder] = (0, import_react.useState)("inbox");
    const [selected, setSelected] = (0, import_react.useState)(1);
    const [query, setQuery] = (0, import_react.useState)("");
    const [composing, setComposing] = (0, import_react.useState)(false);
    const [toast, setToast] = (0, import_react.useState)(null);
    const visible = (0, import_react.useMemo)(() => {
      const q = query.trim().toLowerCase();
      return messages.filter(
        (m) => inFolder(m, folder) && (!q || m.from.toLowerCase().includes(q) || m.subject.toLowerCase().includes(q) || m.body.join(" ").toLowerCase().includes(q))
      );
    }, [messages, folder, query]);
    const unread = (f) => messages.filter((m) => inFolder(m, f) && m.unread).length;
    const current = messages.find((m) => m.id === selected && visible.some((v) => v.id === m.id));
    function open(id) {
      setSelected(id);
      setMessages((ms) => ms.map((m) => m.id === id ? { ...m, unread: false } : m));
    }
    function toggleStar(id) {
      setMessages((ms) => ms.map((m) => m.id === id ? { ...m, starred: !m.starred } : m));
    }
    function send(d) {
      setMessages((ms) => [
        {
          id: Math.max(...ms.map((m) => m.id)) + 1,
          folder: "sent",
          from: "Sam Carter",
          email: "sam@acme.dev",
          initials: "SC",
          gradient: "from-teal-500 to-emerald-400",
          subject: d.subject,
          body: d.body.split("\n\n"),
          time: "Now",
          date: "Tue, Sep 24, 9:45 AM",
          unread: false,
          starred: false
        },
        ...ms
      ]);
      setComposing(false);
      setToast(`Message sent to ${d.to}`);
    }
    const folderName = folders.find((f) => f.id === folder).name;
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex h-screen overflow-hidden bg-gray-50 font-sans text-gray-900", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("aside", { className: "flex w-60 shrink-0 flex-col border-r border-gray-200 bg-gray-50", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex h-14 items-center gap-2.5 px-5", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "flex h-8 w-8 items-center justify-center rounded-lg bg-indigo-600 text-white shadow-sm", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Inbox, { className: "h-4 w-4" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "text-base font-semibold text-gray-900", children: "Relay Mail" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "px-4 pb-4 pt-2", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
          "button",
          {
            id: "compose",
            onClick: () => setComposing(true),
            className: "flex w-full items-center justify-center gap-2 rounded-lg bg-indigo-600 px-3.5 py-2.5 text-sm font-medium text-white shadow-sm hover:bg-indigo-500",
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(PenLine, { className: "h-4 w-4" }),
              "Compose"
            ]
          }
        ) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("nav", { className: "flex-1 space-y-0.5 overflow-y-auto px-3", children: [
          folders.map(({ id, name, icon: I }) => {
            const active = id === folder;
            const n = id === "drafts" ? messages.filter((m) => m.folder === "drafts").length : unread(id);
            return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
              "button",
              {
                "data-folder": id,
                onClick: () => {
                  setFolder(id);
                  setSelected(null);
                },
                className: `flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm ${active ? "bg-indigo-50 font-semibold text-indigo-700" : "text-gray-600 hover:bg-gray-100 hover:text-gray-900"}`,
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(I, { className: `h-4 w-4 ${active ? "text-indigo-600" : "text-gray-400"}` }),
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "flex-1 text-left", children: name }),
                  n > 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
                    "span",
                    {
                      className: `rounded-full px-2 py-0.5 text-xs font-medium ${active ? "bg-indigo-600 text-white" : "bg-gray-200 text-gray-600"}`,
                      children: n
                    }
                  )
                ]
              },
              id
            );
          }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "px-3 pb-2 pt-6 text-xs font-semibold uppercase tracking-wide text-gray-400", children: "Labels" }),
          Object.keys(labelStyles).map((l) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3 rounded-md px-3 py-2 text-sm text-gray-600", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `h-2 w-2 rounded-full ${labelStyles[l].dot}` }),
            l
          ] }, l))
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-3 border-t border-gray-200 px-4 py-3", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "inline-flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-br from-teal-500 to-emerald-400 text-xs font-semibold text-white", children: "SC" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-sm font-medium text-gray-900", children: "Sam Carter" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "truncate text-xs text-gray-500", children: "sam@acme.dev" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { "aria-label": "Settings", className: "rounded-md p-1.5 text-gray-400 hover:bg-gray-100", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Settings, { className: "h-4 w-4" }) })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("section", { className: "flex w-[400px] shrink-0 flex-col border-r border-gray-200 bg-white", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "flex h-14 shrink-0 items-center gap-3 border-b border-gray-200 px-4", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "relative flex-1", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-gray-400" }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
            "input",
            {
              id: "search",
              type: "search",
              autoComplete: "off",
              value: query,
              onChange: (e) => setQuery(e.target.value),
              placeholder: "Search mail",
              className: "w-full rounded-lg border border-gray-200 bg-gray-50 py-2 pl-9 pr-3 text-sm placeholder:text-gray-400 focus:border-indigo-500 focus:bg-white focus:outline-none focus:ring-2 focus:ring-indigo-500/30"
            }
          )
        ] }) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex shrink-0 items-center justify-between px-4 pb-2 pt-3", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { className: "text-sm font-semibold text-gray-900", children: query.trim() ? `Results for “${query.trim()}”` : folderName }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "text-xs text-gray-500", children: [
            visible.length,
            " ",
            visible.length === 1 ? "conversation" : "conversations"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("ul", { className: "flex-1 divide-y divide-gray-100 overflow-y-auto", children: [
          visible.map((m) => {
            const active = m.id === current?.id;
            return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
              "div",
              {
                role: "button",
                tabIndex: 0,
                "data-message": m.id,
                onClick: () => open(m.id),
                className: `relative flex cursor-pointer gap-3 px-4 py-3 ${active ? "bg-indigo-50/70" : "hover:bg-gray-50"}`,
                children: [
                  active && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "absolute inset-y-0 left-0 w-0.5 bg-indigo-600" }),
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Avatar, { m }),
                  /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "min-w-0 flex-1", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "flex items-center gap-2", children: [
                      m.unread && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "h-2 w-2 shrink-0 rounded-full bg-indigo-600", "aria-label": "Unread" }),
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `truncate text-sm ${m.unread ? "font-semibold text-gray-900" : "font-medium text-gray-700"}`, children: m.from }),
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `ml-auto shrink-0 text-xs ${m.unread ? "font-semibold text-indigo-600" : "text-gray-400"}`, children: m.time })
                    ] }),
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-0.5 flex items-center gap-2", children: [
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: `min-w-0 flex-1 truncate text-sm ${m.unread ? "font-medium text-gray-900" : "text-gray-600"}`, children: m.subject }),
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(StarButton, { m, onToggle: toggleStar })
                    ] }),
                    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "mt-0.5 flex items-center gap-2", children: [
                      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "min-w-0 flex-1 truncate text-xs text-gray-500", children: m.body.join(" ") }),
                      m.attachment && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Paperclip, { className: "h-3.5 w-3.5 shrink-0 text-gray-400" }),
                      m.label && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: `h-2 w-2 shrink-0 rounded-full ${labelStyles[m.label].dot}`, title: m.label })
                    ] })
                  ] })
                ]
              }
            ) }, m.id);
          }),
          visible.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { className: "px-6 py-16 text-center", children: [
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Search, { className: "mx-auto h-6 w-6 text-gray-300" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-2 text-sm font-medium text-gray-900", children: "No messages found" }),
            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "mt-1 text-xs text-gray-500", children: "Try a different search or folder." })
          ] })
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ReadingPane, { m: current, position: visible.findIndex((v) => v.id === current?.id) + 1, total: visible.length, onToggleStar: toggleStar }),
      toast && /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "fixed bottom-6 left-1/2 z-40 flex -translate-x-1/2 items-center gap-3 rounded-lg bg-gray-900 px-4 py-3 text-sm text-white shadow-lg", children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Send, { className: "h-4 w-4 text-emerald-400" }),
        toast,
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { onClick: () => setToast(null), "aria-label": "Dismiss", className: "text-gray-400 hover:text-white", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(X, { className: "h-4 w-4" }) })
      ] }),
      composing && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(ComposeModal, { onClose: () => setComposing(false), onSend: send })
    ] });
  }

  // inbox/main.tsx
  var import_jsx_runtime3 = __toESM(require_jsx_runtime());
  (0, import_client.createRoot)(document.getElementById("root")).render(
    /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(import_react2.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(App, {}) })
  );
})();
