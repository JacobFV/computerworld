// A writable store shared between Todos.svelte and Counter.svelte (both
// auto-subscribe with `$count`), plus a store derived from it. Plain
// `svelte/store` usage - no compiler magic needed for this file itself.
import { writable, derived } from 'svelte/store';

export const count = writable(0);
export const doubled = derived(count, ($count) => $count * 2);
