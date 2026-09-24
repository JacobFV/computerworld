'use strict';
const PEOPLE = { ada: ['AL', 'rgb(200, 90, 60)'], bo: ['BK', 'rgb(60, 120, 190)'], cy: ['CN', 'rgb(120, 90, 170)'] };
const INITIAL = [
  { id: 1, title: 'Checkout fails when the basket holds a gift card', tag: 'bug', who: 'ada', due: 'Sep 24', status: 'doing' },
  { id: 2, title: 'Export invoices as CSV', tag: 'feature', who: 'bo', due: 'Sep 26', status: 'todo' },
  { id: 3, title: 'Rotate the staging certificates', tag: 'chore', who: 'cy', due: 'Sep 22', status: 'done' },
  { id: 4, title: 'Search results lose their filters after paging back', tag: 'bug', who: 'bo', due: 'Sep 25', status: 'todo' },
  { id: 5, title: 'Dark mode for the settings pages', tag: 'feature', who: 'ada', due: 'Oct 02', status: 'todo' },
  { id: 6, title: 'Upgrade the build image', tag: 'chore', who: 'cy', due: 'Sep 20', status: 'done' },
];
const COLUMNS = [['todo', 'To do'], ['doing', 'In progress'], ['done', 'Done']];
const { createApp, ref, computed } = Vue;

const Avatar = {
  props: ['who'],
  template: `<span class="avatar" :style="{ backgroundColor: PEOPLE[who][1] }">{{ PEOPLE[who][0] }}</span>`,
  setup() { return { PEOPLE }; },
};

const Tag = {
  props: ['tag'],
  template: `<span :class="'tag ' + tag">{{ tag }}</span>`,
};

createApp({
  components: { Avatar, Tag },
  template: `
<div class="shell">
  <header class="top">
    <span class="brand">Tracker</span>
    <nav class="tabs">
      <button :class="['tab', { active: view === 'list' }]" id="tab-list" @click="view = 'list'">List</button>
      <button :class="['tab', { active: view === 'board' }]" id="tab-board" @click="view = 'board'">Board</button>
    </nav>
    <span class="spacer"></span>
    <span class="who"><span>Cy Nakamura</span><Avatar who="cy" /></span>
  </header>
  <div class="body">
    <aside class="side">
      <h2>Filters</h2>
      <button v-for="[name, label] in FILTERS" :key="name" :class="['filter', { active: filter === name }]"
        :id="'filter-' + name" @click="filter = name"><span>{{ label }}</span><span class="count">{{ counts[name] }}</span></button>
    </aside>
    <main class="main">
      <section class="stats">
        <div class="stat" id="stat-total"><div class="label">Total</div><div class="value">{{ tasks.length }}</div></div>
        <div class="stat" id="stat-open"><div class="label">Open</div><div class="value">{{ counts.open }}</div></div>
        <div class="stat" id="stat-done"><div class="label">Done</div><div class="value">{{ pct }}%</div>
          <div class="bar"><div :style="{ width: pct + '%' }"></div></div></div>
      </section>
      <template v-if="view === 'list'">
        <form class="add" @submit.prevent="add">
          <input id="new-task" placeholder="Add a task" v-model="draft">
          <button type="submit" id="add-task">Add</button>
        </form>
        <ul v-if="visible.length" class="list" id="list">
          <li v-for="t in visible" :key="t.id" :class="['task', { done: t.status === 'done' }]" :id="'task-' + t.id">
            <button class="check" :id="'check-' + t.id" aria-label="toggle" @click="toggle(t.id)"></button>
            <span class="title">{{ t.title }}</span>
            <Tag :tag="t.tag" />
            <Avatar :who="t.who" />
            <span class="due">{{ t.due }}</span>
          </li>
        </ul>
        <div v-else class="list empty">Nothing here</div>
      </template>
      <section v-else class="board">
        <div v-for="[status, name] in COLUMNS" :key="status" class="column" :id="'column-' + status">
          <h3><span>{{ name }}</span><span class="count">{{ tasks.filter((t) => t.status === status).length }}</span></h3>
          <div v-for="t in tasks.filter((t) => t.status === status)" :key="t.id" class="card">
            <div>{{ t.title }}</div>
            <div class="meta"><Tag :tag="t.tag" /><Avatar :who="t.who" /></div>
          </div>
        </div>
      </section>
    </main>
  </div>
</div>`,
  setup() {
    const tasks = ref(INITIAL.map((t) => ({ ...t })));
    const view = ref('list');
    const filter = ref('all');
    const draft = ref('');
    const counts = computed(() => ({
      all: tasks.value.length,
      open: tasks.value.filter((t) => t.status !== 'done').length,
      done: tasks.value.filter((t) => t.status === 'done').length,
    }));
    const pct = computed(() => (tasks.value.length ? Math.round((counts.value.done * 100) / tasks.value.length) : 0));
    const visible = computed(() => tasks.value.filter((t) =>
      filter.value === 'all' || (filter.value === 'open' ? t.status !== 'done' : t.status === 'done')));
    const toggle = (id) => {
      const t = tasks.value.find((x) => x.id === id);
      t.status = t.status === 'done' ? 'todo' : 'done';
    };
    const add = () => {
      const title = draft.value.trim();
      if (!title) return;
      tasks.value.push({ id: tasks.value.length + 1, title, tag: 'feature', who: 'cy', due: 'Oct 09', status: 'todo' });
      draft.value = '';
    };
    const FILTERS = [['all', 'All tasks'], ['open', 'Open'], ['done', 'Done']];
    return { tasks, view, filter, draft, counts, pct, visible, toggle, add, FILTERS, COLUMNS };
  },
}).mount('#app');
