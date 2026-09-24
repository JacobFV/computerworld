// `react-dom/client`, from the global `ReactDOM` the host's React build defines.
import type * as ClientTypes from "react-dom/client";

const D = (globalThis as unknown as { ReactDOM: typeof ClientTypes }).ReactDOM;
export const createRoot = D.createRoot;
