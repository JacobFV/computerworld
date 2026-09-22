<script>
  import { onMount, onDestroy, tick, createEventDispatcher } from 'svelte';
  import { fade } from 'svelte/transition';
  import { count, doubled } from './stores.js';

  // Props with defaults.
  export let step = 1;
  export let label = 'Count';

  if (!window.trace) window.trace = [];

  const dispatch = createEventDispatcher();

  // A plain reactive statement...
  $: milestoneHit = $count > 0 && $count % 5 === 0;
  // ...and a reactive block with a side effect: dispatch a component event and
  // record it, whenever the plain reactive statement above turns true.
  $: {
    if (milestoneHit) {
      dispatch('milestone', { count: $count });
      window.trace.push('counter:milestone:' + $count);
    }
  }

  let noteVisible = false;

  async function inc() {
    count.update((n) => n + step);
    // tick(): the store subscription updates `$count` synchronously, but the
    // DOM text is only patched once the scheduler flushes. Record both sides
    // of that gap to prove tick() actually waits for it.
    const before = document.getElementById('sv-counter-value').textContent;
    await tick();
    const after = document.getElementById('sv-counter-value').textContent;
    window.trace.push('tick:' + before + '->' + after);
  }

  function dec() {
    count.update((n) => n - step);
  }

  function toggleNote() {
    noteVisible = !noteVisible;
  }

  onMount(() => window.trace.push('counter:mount'));
  onDestroy(() => window.trace.push('counter:destroy'));
</script>

<div id="sv-counter">
  <span id="sv-counter-label">{label}</span>:
  <span id="sv-counter-value">{$count}</span>
  <span id="sv-counter-double">(x2 = {$doubled})</span>
  <button id="sv-inc" on:click={inc}>+{step}</button>
  <button id="sv-dec" on:click={dec}>-{step}</button>
  <button id="sv-note-toggle" on:click={toggleNote}>toggle note</button>
  {#if noteVisible}
    <p id="sv-note" transition:fade={{ duration: 200 }}>Faded note (count={$count})</p>
  {/if}
  <div id="sv-counter-slot">
    <slot>default slot text</slot>
  </div>
</div>
