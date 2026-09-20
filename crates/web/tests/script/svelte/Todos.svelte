<script>
  import { onMount, onDestroy, beforeUpdate, afterUpdate } from 'svelte';
  import { count, doubled } from './stores.js';
  import Counter from './Counter.svelte';

  // Props with defaults.
  export let title = 'Todos';
  export let items = [
    { id: 1, text: 'Write spec', done: false },
    { id: 2, text: 'Review PR', done: true },
    { id: 3, text: 'Ship it', done: false },
  ];

  if (!window.trace) window.trace = [];

  let draft = '';
  let filter = 'all'; // 'all' | 'active' | 'done'
  let compact = false;
  let nextId = items.reduce((max, i) => Math.max(max, i.id), 0) + 1;
  let lastMilestone = null;

  // Reactive statements.
  $: remaining = items.filter((i) => !i.done).length;
  $: visible = items.filter((i) => (filter === 'active' ? !i.done : filter === 'done' ? i.done : true));
  // A reactive block with a side effect: keep window.trace up to date whenever
  // the remaining count changes.
  $: {
    window.trace.push('remaining:' + remaining);
  }

  function addItem() {
    const text = draft.trim();
    if (!text) return;
    items = [...items, { id: nextId++, text, done: false }];
    draft = '';
    window.trace.push('add:' + text);
  }

  function removeItem(id) {
    items = items.filter((i) => i.id !== id);
    window.trace.push('remove:' + id);
  }

  function toggleItem(id) {
    items = items.map((i) => (i.id === id ? { ...i, done: !i.done } : i));
  }

  function reverseItems() {
    items = [...items].reverse();
    window.trace.push('reverse');
  }

  function setFilter(f) {
    filter = f;
  }

  function handleKeydown(event) {
    if (event.key === 'Escape') {
      draft = '';
      window.trace.push('escape');
    }
  }

  function handleMilestone(event) {
    lastMilestone = event.detail.count;
    window.trace.push('todos:milestone:' + event.detail.count);
  }

  onMount(() => window.trace.push('todos:mount'));
  onDestroy(() => window.trace.push('todos:destroy'));
  beforeUpdate(() => window.trace.push('todos:before-update'));
  afterUpdate(() => window.trace.push('todos:after-update'));
</script>

<div id="sv-todos">
  <h2 id="sv-title">{title} ({remaining} left)</h2>

  <form id="sv-add-form" on:submit|preventDefault={addItem}>
    <input
      id="sv-new-input"
      type="text"
      bind:value={draft}
      on:keydown={handleKeydown}
      placeholder="What needs doing?"
    />
    <button id="sv-add" type="submit" disabled={!draft.trim()}>Add</button>
  </form>

  <div id="sv-filters">
    <button id="sv-filter-all" class:active={filter === 'all'} on:click={() => setFilter('all')}>All</button>
    <button id="sv-filter-active" class:active={filter === 'active'} on:click={() => setFilter('active')}>Active</button>
    <button id="sv-filter-done" class:active={filter === 'done'} on:click={() => setFilter('done')}>Done</button>
  </div>

  <label><input id="sv-compact" type="checkbox" bind:checked={compact} /> compact</label>
  <button id="sv-reverse" on:click={reverseItems}>Reverse</button>

  <ul id="sv-list" class:compact>
    {#each visible as item (item.id)}
      <li id={`sv-item-${item.id}`} class:done={item.done}>
        <input
          type="checkbox"
          id={`sv-item-${item.id}-check`}
          checked={item.done}
          on:change={() => toggleItem(item.id)}
        />
        <span id={`sv-item-${item.id}-text`}>{item.text}</span>
        <button id={`sv-item-${item.id}-remove`} on:click={() => removeItem(item.id)}>x</button>
      </li>
    {/each}
  </ul>

  {#if remaining === 0}
    <p id="sv-empty">Nothing to do</p>
  {:else if remaining < 3}
    <p id="sv-some">A few things left</p>
  {:else}
    <p id="sv-many">Lots to do</p>
  {/if}

  <p id="sv-shared-count">shared count: {$count} (doubled {$doubled})</p>

  <section id="sv-counter-host">
    <Counter step={2} label="Widgets" on:milestone={handleMilestone}>
      <span id="sv-counter-slot-content">extra widget info</span>
    </Counter>
  </section>
  <p id="sv-last-milestone">last milestone: {lastMilestone === null ? 'none' : lastMilestone}</p>
</div>
