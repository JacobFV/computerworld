// `react`, for applications bundled against the React build the host loads first:
// the global `React` the production build defines, re-exported as the module.
/// <reference path="../types/cw.d.ts" />
import type * as ReactTypes from "react";

const R = (globalThis as unknown as { React: typeof ReactTypes }).React;
export default R;
export const {
  Fragment,
  createElement,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} = R;
