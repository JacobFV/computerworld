// Types for the `cw` global a web application reaches its machine through.
// The host installs it before the application's script runs; see
// docs/custom-application.md ("Web applications") for the contract.

/** A platform the window is painted as. */
type CwPlatform = "macos" | "windows" | "ubuntu" | "ios" | "android";

/** Where the application runs. It changes when the window is resized or painted for another platform. */
interface CwEnv {
  readonly platform: CwPlatform;
  /** A phone: one screen at a time, and text fields take the keyboard only when tapped. */
  readonly mobile: boolean;
  /** The window's content size in CSS pixels. */
  readonly width: number;
  readonly height: number;
  /**
   * The platform's palette and UI typeface as custom properties on `:root`
   * (`--cw-accent`, `--cw-surface`, `--cw-chrome`, `--cw-selection`, `--cw-ink`,
   * `--cw-muted`, `--cw-faint`, `--cw-line`, `--cw-radius`, `--cw-row`, `--cw-title`,
   * `--cw-font`). Already applied; `<html data-platform=… data-mobile>` is set too.
   */
  readonly css: string;
}

/** A service's answer to `cw.fetch`. */
interface CwResponse {
  readonly status: number;
  readonly ok: boolean;
  readonly body: string;
  text(): Promise<string>;
  json<T = unknown>(): Promise<T>;
}

interface CwFetchInit {
  method?: "GET" | "POST" | "PATCH" | "PUT" | "DELETE";
  body?: string;
}

/** Facts the window frame shows. */
interface CwWindowFacts {
  /** The path or address of what is open (the frame may show it in its title). */
  document?: string;
  /** A short name for what is open. */
  caption?: string;
  /** What is open has changes that are not saved. */
  modified?: boolean;
}

interface Cw {
  /** The application's kind, as it was registered. */
  readonly kind: string;
  /** What it was launched on (a folder, a file, a service URL); empty on a restore. */
  readonly argument: string;
  readonly env: CwEnv;
  /** Calls `listener` whenever `env` changes. Must not declare state or request anything. */
  onEnv(listener: (env: CwEnv) => void): () => void;
  /** The world clock, in microseconds, as of the input being handled. `Date.now()` may run ahead of it. */
  now(): number;
  /**
   * The application's declared state: all a snapshot keeps. A restored window boots
   * the same script with `get()` returning what was last `set`, so the document must be
   * a function of it; `get()` is `null` on a first launch.
   */
  readonly state: {
    get<T = unknown>(): T | null;
    set<T>(value: T): void;
  };
  /** The machine's filesystem, with the user's permissions. Relative paths are the user's. */
  readonly fs: {
    readFile(path: string): Promise<string>;
    writeFile(path: string, content: string): Promise<void>;
    /** Names in a folder, folders ending in `/`. */
    list(path: string): Promise<string[]>;
    /** Creates the folder and missing parents. Settles at once: later requests run after it. */
    mkdir(path: string): Promise<void>;
  };
  /** A request to a world service through the machine's network. Rejects on a transport failure. */
  fetch(url: string, init?: CwFetchInit): Promise<CwResponse>;
  /** Opens another application in a window of its own. */
  launch(kind: string, argument?: string): Promise<void>;
  /** Records named data in the world's event log. */
  emit(name: string, data?: unknown): Promise<void>;
  /**
   * Refuses the input being handled: the action that delivered it fails with
   * `message`, as a native application's refusal does.
   */
  refuse(message: string): void;
  readonly window: {
    set(facts: CwWindowFacts): void;
  };
}

declare const cw: Cw;
