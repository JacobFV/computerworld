// Built by crates/applications/web/build.mjs from notes/Notes.tsx. Do not edit.
"use strict";
(() => {
  // sdk/react.ts
  var R = globalThis.React;
  var react_default = R;
  var {
    Fragment,
    createElement,
    useCallback,
    useEffect,
    useLayoutEffect,
    useMemo,
    useReducer,
    useRef,
    useState,
    useSyncExternalStore
  } = R;

  // sdk/react-dom-client.ts
  var D = globalThis.ReactDOM;
  var createRoot = D.createRoot;

  // sdk/cw.ts
  function declaredStore(init) {
    const saved = cw.state.get();
    let current = saved ?? init();
    if (saved === null) cw.state.set(current);
    const listeners = /* @__PURE__ */ new Set();
    const store2 = {
      restored: saved !== null,
      get: () => current,
      set(next) {
        if (Object.is(next, current)) return;
        current = next;
        cw.state.set(next);
        for (const listener of Array.from(listeners)) listener();
      },
      update(change) {
        store2.set(change(current));
      },
      subscribe(listener) {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      }
    };
    return store2;
  }
  function useStore(store2) {
    return useSyncExternalStore(store2.subscribe, store2.get);
  }
  function useEnv() {
    const [env, setEnv] = useState(cw.env);
    useEffect(() => cw.onEnv(setEnv), []);
    return env;
  }

  // notes/Notes.tsx
  var TEXT_LIMIT = 64 * 1024;
  var store = declaredStore(() => ({
    folder: cw.argument ? cw.argument.replace(/\/+$/, "") : "Notes",
    entries: [],
    open: null,
    text: "",
    dirty: false,
    problem: null,
    editing: false
  }));
  function byCodePoint(a, b) {
    const x = Array.from(a);
    const y = Array.from(b);
    for (let i = 0; i < Math.min(x.length, y.length); i++) {
      const d = (x[i].codePointAt(0) ?? 0) - (y[i].codePointAt(0) ?? 0);
      if (d !== 0) return d;
    }
    return x.length - y.length;
  }
  function describe(error) {
    return error instanceof Error ? error.message : String(error);
  }
  function list() {
    const { folder } = store.get();
    cw.fs.list(folder).then(
      (names) => store.update((s) => ({
        ...s,
        entries: names.filter((n) => !n.endsWith("/")),
        problem: null
      })),
      (error) => store.update((s) => ({ ...s, problem: describe(error) }))
    );
  }
  function save() {
    const s = store.get();
    if (s.open === null) {
      cw.refuse("no note is open");
      return false;
    }
    const path = `${s.folder}/${s.open}`;
    store.set({ ...s, dirty: false });
    cw.fs.mkdir(s.folder);
    cw.fs.writeFile(path, s.text).then(list);
    return true;
  }
  function newNote() {
    const name = `note-${Math.floor(cw.now() / 1e6)}.txt`;
    store.update((s) => ({
      ...s,
      open: name,
      editing: true,
      text: "",
      dirty: true,
      entries: s.entries.includes(name) ? s.entries : [...s.entries, name].sort(byCodePoint)
    }));
  }
  function open(name) {
    const s = store.get();
    if (!s.entries.includes(name)) {
      cw.refuse("note not found");
      return;
    }
    store.set({ ...s, open: name, editing: false, text: "", dirty: false });
    cw.fs.readFile(`${s.folder}/${name}`).then(
      (content) => store.update((now) => now.open === name ? { ...now, text: content, dirty: false } : now),
      // A note that cannot be read fails the click that opened it.
      (error) => cw.refuse(describe(error))
    );
  }
  function close() {
    if (store.get().dirty) save();
    store.update((s) => ({ ...s, open: null, editing: false, text: "", dirty: false }));
  }
  function edit(text) {
    store.update((s) => ({ ...s, text: text.slice(0, TEXT_LIMIT), dirty: true }));
  }
  var EDITING_KEYS = /* @__PURE__ */ new Set([
    "Backspace",
    "Delete",
    "Enter",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Home",
    "End",
    "PageUp",
    "PageDown"
  ]);
  function onKey(event) {
    const s = store.get();
    const command = event.ctrlKey || event.metaKey;
    if (command && event.key.toLowerCase() === "s") {
      event.preventDefault();
      save();
      return;
    }
    const printable = Array.from(event.key).length === 1 && !command && !event.altKey;
    if (s.open === null) {
      event.preventDefault();
      cw.refuse(
        event.key === "Backspace" || event.key === "Enter" ? "no note is open" : `unsupported notes key ${event.key}`
      );
      return;
    }
    const inBody = event.target?.id === "notes:body";
    if (!inBody) {
      if (event.key === "Backspace") {
        event.preventDefault();
        edit(Array.from(s.text).slice(0, -1).join(""));
      } else if (event.key === "Enter") {
        event.preventDefault();
        edit(s.text + "\n");
      } else if (printable) {
        event.preventDefault();
        edit(s.text + event.key);
      } else {
        event.preventDefault();
        cw.refuse(`unsupported notes key ${event.key}`);
      }
      return;
    }
    if (!printable && !EDITING_KEYS.has(event.key)) {
      event.preventDefault();
      cw.refuse(`unsupported notes key ${event.key}`);
    } else if (event.key === "Backspace" && s.text === "") {
      store.set({ ...s, dirty: true });
    }
  }
  function title(platform) {
    return platform === "windows" ? "Sticky Notes" : platform === "android" ? "Keep" : "Notes";
  }
  function Action(props) {
    return /* @__PURE__ */ react_default.createElement(
      "button",
      {
        id: props.id,
        className: `action ${props.place}${props.primary ? " primary" : ""}`,
        onClick: props.onClick
      },
      props.label
    );
  }
  function List(props) {
    const { s } = props;
    return /* @__PURE__ */ react_default.createElement(react_default.Fragment, null, s.entries.map((name) => /* @__PURE__ */ react_default.createElement(
      "button",
      {
        key: name,
        id: `notes:open:${name}`,
        "aria-label": name,
        className: s.open === name ? "row on" : "row",
        onClick: () => open(name)
      },
      name.replace(/\.txt$/, "")
    )));
  }
  function Notice(props) {
    const { s } = props;
    if (s.problem !== null) {
      return /* @__PURE__ */ react_default.createElement("p", { id: "notes-problem", role: "alert", className: "notice listed" }, s.problem);
    }
    return s.entries.length === 0 ? /* @__PURE__ */ react_default.createElement("p", { className: "notice listed" }, "No notes yet") : null;
  }
  function Buttons() {
    return /* @__PURE__ */ react_default.createElement("div", { className: "buttons" }, /* @__PURE__ */ react_default.createElement(Action, { id: "notes:new", place: "new", label: "New note", primary: true, onClick: newNote }), /* @__PURE__ */ react_default.createElement(Action, { id: "notes:reload", place: "reload", label: "Reload", primary: false, onClick: list }));
  }
  function Note(props) {
    const { s, phone, heading } = props;
    const name = (s.open ?? "").replace(/\.txt$/, "");
    return /* @__PURE__ */ react_default.createElement("section", { className: phone ? "note phone" : "note" }, phone && /* @__PURE__ */ react_default.createElement("button", { id: "notes:close", "data-cw-back": "", className: "back", "aria-label": "Back to notes", onClick: close }, /* @__PURE__ */ react_default.createElement("span", { className: "chevron" }, "‹"), /* @__PURE__ */ react_default.createElement("span", { className: "back-title" }, heading)), /* @__PURE__ */ react_default.createElement("strong", { className: "name" }, name), /* @__PURE__ */ react_default.createElement(
      "textarea",
      {
        id: "notes:body",
        "data-page-id": "notes-body",
        "data-cw-pane": "note",
        "aria-label": "Note",
        className: "body",
        value: s.text,
        maxLength: TEXT_LIMIT,
        spellCheck: false,
        onChange: (e) => edit(e.currentTarget.value),
        onClick: () => store.update((now) => now.editing ? now : { ...now, editing: true })
      }
    ), s.text === "" && /* @__PURE__ */ react_default.createElement("span", { className: "empty", "aria-hidden": "true" }, "Empty note"), /* @__PURE__ */ react_default.createElement("button", { id: "notes:save", className: s.dirty ? "action primary save" : "action save", onClick: save }, s.dirty ? "Save •" : "Save"));
  }
  function Notes() {
    const s = useStore(store);
    const env = useEnv();
    const narrow = env.mobile || env.width < 480;
    const heading = title(env.platform);
    const body = useRef(null);
    const wantsFocus = s.open !== null && (s.editing || !env.mobile);
    useLayoutEffect(() => {
      const field = document.getElementById("notes:body");
      body.current = field;
      if (wantsFocus && field && document.activeElement !== field) {
        field.focus();
        const end = field.value.length;
        field.setSelectionRange(end, end);
      }
    });
    useEffect(() => {
      const refocus = () => {
        const field = body.current;
        if (wantsFocus && field && document.activeElement !== field) field.focus();
      };
      document.addEventListener("focusin", refocus);
      return () => document.removeEventListener("focusin", refocus);
    }, [wantsFocus]);
    useEffect(() => {
      document.addEventListener("keydown", onKey);
      return () => document.removeEventListener("keydown", onKey);
    }, []);
    useEffect(() => {
      cw.window.set({
        document: s.open === null ? "" : `${s.folder}/${s.open}`,
        caption: s.open ?? "",
        modified: s.dirty
      });
    }, [s.folder, s.open, s.dirty]);
    const folder = /* @__PURE__ */ react_default.createElement("h2", { id: "notes-folder", hidden: true }, s.folder);
    if (narrow && s.open !== null) {
      return /* @__PURE__ */ react_default.createElement("div", { className: "app narrow" }, folder, /* @__PURE__ */ react_default.createElement(Note, { s, phone: true, heading }));
    }
    if (env.mobile) {
      return /* @__PURE__ */ react_default.createElement("div", { className: `app narrow ${env.platform}` }, folder, env.platform === "android" && /* @__PURE__ */ react_default.createElement("header", { className: "appbar" }, /* @__PURE__ */ react_default.createElement("strong", null, heading)), /* @__PURE__ */ react_default.createElement(
        "div",
        {
          id: "main",
          className: "main",
          "data-cw-large-title": env.platform === "ios" ? heading : void 0,
          "data-cw-large-title-height": env.platform === "ios" ? "52" : void 0
        },
        env.platform === "ios" && /* @__PURE__ */ react_default.createElement("strong", { className: "large-title" }, heading),
        /* @__PURE__ */ react_default.createElement("div", { className: "sheet" }, /* @__PURE__ */ react_default.createElement(Notice, { s }), /* @__PURE__ */ react_default.createElement(Buttons, null), /* @__PURE__ */ react_default.createElement("div", { className: "rows" }, /* @__PURE__ */ react_default.createElement(List, { s })))
      ));
    }
    return /* @__PURE__ */ react_default.createElement("div", { className: narrow ? "app desktop narrow" : "app desktop" }, folder, /* @__PURE__ */ react_default.createElement("header", { className: "toolbar" }, /* @__PURE__ */ react_default.createElement("strong", null, heading)), /* @__PURE__ */ react_default.createElement("aside", { className: "sidebar" }, /* @__PURE__ */ react_default.createElement(Notice, { s }), /* @__PURE__ */ react_default.createElement(Buttons, null), /* @__PURE__ */ react_default.createElement("div", { id: "list", className: "list" }, /* @__PURE__ */ react_default.createElement(List, { s }))), !narrow && /* @__PURE__ */ react_default.createElement("main", { className: "pane" }, s.open !== null ? /* @__PURE__ */ react_default.createElement(Note, { s, phone: false, heading }) : /* @__PURE__ */ react_default.createElement("p", { className: "notice" }, "Select a note")));
  }
  if (!store.restored) list();
  createRoot(document.getElementById("root")).render(/* @__PURE__ */ react_default.createElement(Notes, null));
})();
