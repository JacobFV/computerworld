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
  const first = saved === null;
  const holder = { current: saved === null ? init() : saved };
  if (first) cw.state.set(holder.current);
  const listeners = new Set<() => void>();
  const set = (next: T): void => {
    if (Object.is(next, holder.current)) return;
    holder.current = next;
    cw.state.set(next);
    for (const listener of Array.from(listeners)) listener();
  };
  return {
    restored: !first,
    get: () => holder.current,
    set,
    update: (change: (current: T) => T) => set(change(holder.current)),
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
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
