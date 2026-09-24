// Helpers for React applications on the `cw` global.
/// <reference path="../types/cw.d.ts" />
import { useEffect, useState, useSyncExternalStore } from "react";

/**
 * The application's declared state as a store: `get` is always the latest value (an
 * event handler can decide on it synchronously), `set` declares it to the host and
 * re-renders. On a restore it starts from what was declared; otherwise from `init()`,
 * which is declared at once.
 */
export interface Store<T> {
  get(): T;
  set(next: T): void;
  update(change: (current: T) => T): void;
  subscribe(listener: () => void): () => void;
  /** Whether this boot restored declared state rather than starting fresh. */
  readonly restored: boolean;
}

export function declaredStore<T>(init: () => T): Store<T> {
  const saved = cw.state.get<T>();
  let current: T = saved ?? init();
  if (saved === null) cw.state.set(current);
  const listeners = new Set<() => void>();
  const store: Store<T> = {
    restored: saved !== null,
    get: () => current,
    set(next) {
      if (Object.is(next, current)) return;
      current = next;
      cw.state.set(next);
      for (const listener of Array.from(listeners)) listener();
    },
    update(change) {
      store.set(change(current));
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
  return store;
}

/** The store's current value, re-rendering when it changes. */
export function useStore<T>(store: Store<T>): T {
  return useSyncExternalStore(store.subscribe, store.get);
}

/** `cw.env`, re-rendering when it changes. */
export function useEnv(): CwEnv {
  const [env, setEnv] = useState(cw.env);
  useEffect(() => cw.onEnv(setEnv), []);
  return env;
}
