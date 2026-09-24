// Notes, kept as real files on the machine's own filesystem under the user's Notes
// folder. Nothing is cached that the filesystem does not actually hold.
//
// Control ids are the agent-facing contract and match the native application this
// replaces: `notes:new`, `notes:reload`, `notes:open:<file>`, `notes:save`,
// `notes:body` and, on a phone, `notes:close`; the semantic page names the folder
// `notes-folder`, a problem `notes-problem` and the note's text `notes-body`.
/// <reference path="../types/cw.d.ts" />
import React, { useEffect, useLayoutEffect, useRef } from "react";
import { createRoot } from "react-dom/client";
import { declaredStore, useEnv, useStore } from "../sdk/cw";

/** The declared state: what a snapshot keeps. Field names and order are the window's saved form. */
interface NotesState {
  folder: string;
  entries: string[];
  open: string | null;
  text: string;
  dirty: boolean;
  problem: string | null;
  /** The body was tapped (or the note was just created): on a phone it has the keyboard. */
  editing: boolean;
}

/** A note is bounded like every field the desktop keeps. */
const TEXT_LIMIT = 64 * 1024;

const store = declaredStore<NotesState>(() => ({
  folder: cw.argument ? cw.argument.replace(/\/+$/, "") : "Notes",
  entries: [],
  open: null,
  text: "",
  dirty: false,
  problem: null,
  editing: false,
}));

/** Code point order, which is the order the machine sorts names in. */
function byCodePoint(a: string, b: string): number {
  const x = Array.from(a);
  const y = Array.from(b);
  for (let i = 0; i < Math.min(x.length, y.length); i++) {
    const d = (x[i].codePointAt(0) ?? 0) - (y[i].codePointAt(0) ?? 0);
    if (d !== 0) return d;
  }
  return x.length - y.length;
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function list(): void {
  const { folder } = store.get();
  cw.fs.list(folder).then(
    (names) =>
      store.update((s) => ({
        ...s,
        entries: names.filter((n) => !n.endsWith("/")),
        problem: null,
      })),
    (error) => store.update((s) => ({ ...s, problem: describe(error) })),
  );
}

function save(): boolean {
  const s = store.get();
  if (s.open === null) {
    cw.refuse("no note is open");
    return false;
  }
  const path = `${s.folder}/${s.open}`;
  store.set({ ...s, dirty: false });
  // The folder may not exist yet on a machine that has never taken a note.
  cw.fs.mkdir(s.folder);
  cw.fs.writeFile(path, s.text).then(list);
  return true;
}

function newNote(): void {
  // A new note is named from the world clock, so two machines agree.
  const name = `note-${Math.floor(cw.now() / 1_000_000)}.txt`;
  store.update((s) => ({
    ...s,
    open: name,
    editing: true,
    text: "",
    dirty: true,
    entries: s.entries.includes(name) ? s.entries : [...s.entries, name].sort(byCodePoint),
  }));
}

function open(name: string): void {
  const s = store.get();
  if (!s.entries.includes(name)) {
    cw.refuse("note not found");
    return;
  }
  store.set({ ...s, open: name, editing: false, text: "", dirty: false });
  cw.fs.readFile(`${s.folder}/${name}`).then(
    (content) =>
      store.update((now) => (now.open === name ? { ...now, text: content, dirty: false } : now)),
    // A note that cannot be read fails the click that opened it.
    (error) => cw.refuse(describe(error)),
  );
}

/** A phone's back button: the list again, and the keyboard goes down. Unsaved text is written first. */
function close(): void {
  if (store.get().dirty) save();
  store.update((s) => ({ ...s, open: null, editing: false, text: "", dirty: false }));
}

function edit(text: string): void {
  store.update((s) => ({ ...s, text: text.slice(0, TEXT_LIMIT), dirty: true }));
}

/** Keys a note's text takes; any other key is refused, as the native Notes refused it. */
const EDITING_KEYS = new Set([
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
  "PageDown",
]);

function onKey(event: KeyboardEvent): void {
  const s = store.get();
  const command = event.ctrlKey || event.metaKey;
  if (command && event.key.toLowerCase() === "s") {
    event.preventDefault();
    save();
    return;
  }
  // One character, which may be outside the BMP (an emoji is two UTF-16 units).
  const printable = Array.from(event.key).length === 1 && !command && !event.altKey;
  if (s.open === null) {
    event.preventDefault();
    cw.refuse(
      event.key === "Backspace" || event.key === "Enter"
        ? "no note is open"
        : `unsupported notes key ${event.key}`,
    );
    return;
  }
  const inBody = (event.target as Element | null)?.id === "notes:body";
  if (!inBody) {
    // The note is open but its body does not have the focus (a phone before the
    // body is tapped): the keys that edit still edit it.
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
    // An erase in an empty note still counts as an edit, as it always has.
    store.set({ ...s, dirty: true });
  }
}

function title(platform: CwPlatform): string {
  return platform === "windows" ? "Sticky Notes" : platform === "android" ? "Keep" : "Notes";
}

function Action(props: { id: string; label: string; primary: boolean; place: string; onClick: () => void }) {
  return (
    <button
      id={props.id}
      className={`action ${props.place}${props.primary ? " primary" : ""}`}
      onClick={props.onClick}
    >
      {props.label}
    </button>
  );
}

function List(props: { s: NotesState }) {
  const { s } = props;
  return (
    <>
      {s.entries.map((name) => (
        <button
          key={name}
          id={`notes:open:${name}`}
          aria-label={name}
          className={s.open === name ? "row on" : "row"}
          onClick={() => open(name)}
        >
          {name.replace(/\.txt$/, "")}
        </button>
      ))}
    </>
  );
}

/** The problem, or why the list is empty; it sits under the buttons. */
function Notice(props: { s: NotesState }) {
  const { s } = props;
  if (s.problem !== null) {
    return (
      <p id="notes-problem" role="alert" className="notice listed">
        {s.problem}
      </p>
    );
  }
  return s.entries.length === 0 ? <p className="notice listed">No notes yet</p> : null;
}

function Buttons() {
  return (
    <div className="buttons">
      <Action id="notes:new" place="new" label="New note" primary onClick={newNote} />
      <Action id="notes:reload" place="reload" label="Reload" primary={false} onClick={list} />
    </div>
  );
}

/** The open note: its name, Save, and the body, which scrolls when it is longer than the window. */
function Note(props: { s: NotesState; phone: boolean; heading: string }) {
  const { s, phone, heading } = props;
  const name = (s.open ?? "").replace(/\.txt$/, "");
  return (
    <section className={phone ? "note phone" : "note"}>
      {phone && (
        <button id="notes:close" data-cw-back="" className="back" aria-label="Back to notes" onClick={close}>
          <svg className="chevron" width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">
            <path
              d="M12.5 4.5 L7 10 L12.5 15.5"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          <span className="back-title">{heading}</span>
        </button>
      )}
      <strong className="name">{name}</strong>
      <textarea
        id="notes:body"
        data-page-id="notes-body"
        data-cw-pane="note"
        aria-label="Note"
        className="body"
        value={s.text}
        maxLength={TEXT_LIMIT}
        spellCheck={false}
        onChange={(e) => edit(e.currentTarget.value)}
        onClick={() => store.update((now) => (now.editing ? now : { ...now, editing: true }))}
      />
      {s.text === "" && (
        <span className="empty" aria-hidden="true">
          Empty note
        </span>
      )}
      <button id="notes:save" className={s.dirty ? "action primary save" : "action save"} onClick={save}>
        {s.dirty ? "Save •" : "Save"}
      </button>
    </section>
  );
}

function Notes() {
  const s = useStore(store);
  const env = useEnv();
  const narrow = env.mobile || env.width < 480;
  const heading = title(env.platform);
  const body = useRef<HTMLTextAreaElement | null>(null);

  // The body has the keyboard whenever a note is open on a desktop, and on a phone
  // once it was tapped: after every render, and whenever something else took it.
  const wantsFocus = s.open !== null && (s.editing || !env.mobile);
  useLayoutEffect(() => {
    const field = document.getElementById("notes:body") as HTMLTextAreaElement | null;
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
      modified: s.dirty,
    });
  }, [s.folder, s.open, s.dirty]);

  const folder = (
    <h2 id="notes-folder" hidden>
      {s.folder}
    </h2>
  );
  // A phone shows one thing at a time: the note that is open, or the list.
  if (narrow && s.open !== null) {
    return (
      <div className="app narrow">
        {folder}
        <Note s={s} phone heading={heading} />
      </div>
    );
  }
  if (env.mobile) {
    return (
      <div className={`app narrow ${env.platform}`}>
        {folder}
        {env.platform === "android" && (
          <header className="appbar">
            <strong>{heading}</strong>
          </header>
        )}
        <div
          id="main"
          className="main"
          data-cw-large-title={env.platform === "ios" ? heading : undefined}
          data-cw-large-title-height={env.platform === "ios" ? "52" : undefined}
        >
          {env.platform === "ios" && <strong className="large-title">{heading}</strong>}
          <div className="sheet">
            <Notice s={s} />
            <Buttons />
            <div className="rows">
              <List s={s} />
            </div>
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className={narrow ? "app desktop narrow" : "app desktop"}>
      {folder}
      <header className="toolbar">
        <strong>{heading}</strong>
      </header>
      <aside className="sidebar">
        <Notice s={s} />
        <Buttons />
        <div id="list" className="list">
          <List s={s} />
        </div>
      </aside>
      {!narrow && (
        <main className="pane">
          {s.open !== null ? (
            <Note s={s} phone={false} heading={heading} />
          ) : (
            <p className="notice">Select a note</p>
          )}
        </main>
      )}
    </div>
  );
}

if (!store.restored) list();
createRoot(document.getElementById("root")!).render(<Notes />);
