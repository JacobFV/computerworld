import { useState, type FormEvent } from 'react';
import { ArrowRight, Calendar, Ellipsis, Filter, Flag, MessageSquare, Paperclip, Plus, Search, X } from '../shared/icons';
import { initialCards, people, type Card, type ColumnId, type Priority } from './data';

const columns: { id: ColumnId; title: string; dot: string }[] = [
  { id: 'todo', title: 'To do', dot: 'bg-gray-400' },
  { id: 'progress', title: 'In progress', dot: 'bg-sky-500' },
  { id: 'review', title: 'In review', dot: 'bg-amber-500' },
  { id: 'done', title: 'Done', dot: 'bg-emerald-500' },
];

const nextColumn: Record<ColumnId, ColumnId | null> = {
  todo: 'progress',
  progress: 'review',
  review: 'done',
  done: null,
};

const priorityStyles: Record<Priority, string> = {
  Low: 'text-gray-500',
  Medium: 'text-amber-500',
  High: 'text-rose-500',
};

const labelStyles: Record<string, string> = {
  Design: 'bg-pink-50 text-pink-700',
  Frontend: 'bg-indigo-50 text-indigo-700',
  Backend: 'bg-emerald-50 text-emerald-700',
  Research: 'bg-amber-50 text-amber-700',
  Bug: 'bg-rose-50 text-rose-700',
};

function Avatar({ id, size = 'h-6 w-6' }: { id: string; size?: string }) {
  const person = people[id];
  return (
    <span
      title={person.name}
      className={`inline-flex ${size} items-center justify-center rounded-full bg-gradient-to-br ${person.gradient} text-[10px] font-semibold text-white ring-2 ring-white`}
    >
      {person.initials}
    </span>
  );
}

function TaskCard({ card, onAdvance }: { card: Card; onAdvance: (id: number) => void }) {
  const next = nextColumn[card.column];
  return (
    <article className="group rounded-lg border border-gray-200 bg-white p-3 shadow-sm transition-shadow hover:shadow-md">
      <div className="flex items-start justify-between gap-2">
        <div className="flex flex-wrap gap-1">
          {card.labels.map((l) => (
            <span key={l} className={`rounded px-1.5 py-0.5 text-[11px] font-medium ${labelStyles[l]}`}>
              {l}
            </span>
          ))}
        </div>
        <Flag className={`h-3.5 w-3.5 shrink-0 ${priorityStyles[card.priority]}`} />
      </div>
      <h3 className="mt-2 text-sm font-medium leading-snug text-gray-900">{card.title}</h3>
      {card.progress !== undefined && (
        <div className="mt-3">
          <div className="flex justify-between text-[11px] text-gray-500">
            <span>Progress</span>
            <span>{card.progress}%</span>
          </div>
          <div className="mt-1 h-1.5 rounded-full bg-gray-100">
            <div className="h-1.5 rounded-full bg-sky-500" style={{ width: `${card.progress}%` }} />
          </div>
        </div>
      )}
      <div className="mt-3 flex items-center justify-between">
        <div className="flex -space-x-1.5">
          {card.assignees.map((a) => (
            <Avatar key={a} id={a} />
          ))}
        </div>
        <div className="flex items-center gap-3 text-xs text-gray-400">
          {card.due && (
            <span className="flex items-center gap-1">
              <Calendar className="h-3.5 w-3.5" />
              {card.due}
            </span>
          )}
          {card.comments > 0 && (
            <span className="flex items-center gap-1">
              <MessageSquare className="h-3.5 w-3.5" />
              {card.comments}
            </span>
          )}
          {card.files > 0 && (
            <span className="flex items-center gap-1">
              <Paperclip className="h-3.5 w-3.5" />
              {card.files}
            </span>
          )}
          {next && (
            <button
              onClick={() => onAdvance(card.id)}
              aria-label={`Move ${card.title}`}
              data-card={card.id}
              className="rounded p-0.5 text-gray-400 hover:bg-gray-100 hover:text-gray-700"
            >
              <ArrowRight className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </div>
    </article>
  );
}

function NewTaskModal({ onClose, onCreate }: { onClose: () => void; onCreate: (title: string, priority: Priority) => void }) {
  const [title, setTitle] = useState('');
  const [priority, setPriority] = useState<Priority>('Medium');
  const [touched, setTouched] = useState(false);
  const error = touched && title.trim().length < 3 ? 'Give the task a title of at least 3 characters.' : null;

  function submit(e: FormEvent) {
    e.preventDefault();
    setTouched(true);
    if (title.trim().length < 3) return;
    onCreate(title.trim(), priority);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-gray-900/40 backdrop-blur-sm" onClick={onClose} />
      <form
        onSubmit={submit}
        role="dialog"
        aria-modal="true"
        className="relative w-full max-w-md rounded-2xl bg-white shadow-2xl ring-1 ring-gray-900/5"
      >
        <div className="flex items-center justify-between border-b border-gray-100 px-6 py-4">
          <h2 className="text-base font-semibold text-gray-900">New task</h2>
          <button type="button" onClick={onClose} className="rounded-md p-1 text-gray-400 hover:bg-gray-100" aria-label="Close">
            <X className="h-5 w-5" />
          </button>
        </div>
        <div className="space-y-4 px-6 py-5">
          <div>
            <label htmlFor="task-title" className="block text-sm font-medium text-gray-700">
              Title
            </label>
            <input
              id="task-title"
              autoComplete="off"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="e.g. Draft the onboarding email"
              className={`mt-1.5 block w-full rounded-lg border px-3 py-2 text-sm shadow-sm focus:outline-none focus:ring-2 ${
                error ? 'border-rose-300 focus:ring-rose-500/30' : 'border-gray-300 focus:border-indigo-500 focus:ring-indigo-500/30'
              }`}
            />
            {error && <p className="mt-1.5 text-xs text-rose-600">{error}</p>}
          </div>
          <div>
            <span className="block text-sm font-medium text-gray-700">Priority</span>
            <div className="mt-1.5 grid grid-cols-3 gap-2">
              {(['Low', 'Medium', 'High'] as Priority[]).map((p) => (
                <button
                  key={p}
                  type="button"
                  data-priority={p}
                  onClick={() => setPriority(p)}
                  className={`flex items-center justify-center gap-1.5 rounded-lg border px-3 py-2 text-sm font-medium ${
                    priority === p ? 'border-indigo-500 bg-indigo-50 text-indigo-700' : 'border-gray-200 text-gray-600 hover:bg-gray-50'
                  }`}
                >
                  <Flag className={`h-3.5 w-3.5 ${priorityStyles[p]}`} />
                  {p}
                </button>
              ))}
            </div>
          </div>
        </div>
        <div className="flex justify-end gap-3 rounded-b-2xl bg-gray-50 px-6 py-4">
          <button type="button" onClick={onClose} className="rounded-lg px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100">
            Cancel
          </button>
          <button type="submit" id="create-task" className="rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500">
            Create task
          </button>
        </div>
      </form>
    </div>
  );
}

export default function App() {
  const [cards, setCards] = useState<Card[]>(initialCards);
  const [showModal, setShowModal] = useState(false);

  function advance(id: number) {
    setCards((cs) => cs.map((c) => (c.id === id && nextColumn[c.column] ? { ...c, column: nextColumn[c.column]! } : c)));
  }

  function create(title: string, priority: Priority) {
    setCards((cs) => [
      ...cs,
      { id: Math.max(...cs.map((c) => c.id)) + 1, title, priority, column: 'todo', labels: ['Research'], assignees: ['ar'], comments: 0, files: 0 },
    ]);
    setShowModal(false);
  }

  return (
    <div className="flex h-screen flex-col bg-gray-50 font-sans text-gray-900">
      <header className="border-b border-gray-200 bg-white">
        <div className="flex items-center justify-between px-6 py-4">
          <div>
            <nav className="text-xs text-gray-500">
              Projects <span className="mx-1 text-gray-300">/</span> Website relaunch
            </nav>
            <h1 className="mt-1 text-xl font-semibold text-gray-900">Sprint 14</h1>
          </div>
          <div className="flex items-center gap-3">
            <div className="flex -space-x-2">
              {Object.keys(people).map((id) => (
                <Avatar key={id} id={id} size="h-8 w-8" />
              ))}
            </div>
            <button
              id="new-task"
              onClick={() => setShowModal(true)}
              className="inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3.5 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500"
            >
              <Plus className="h-4 w-4" />
              New task
            </button>
          </div>
        </div>
        <div className="flex items-center gap-3 px-6 pb-3">
          <div className="relative">
            <Search className="absolute left-2.5 top-2 h-4 w-4 text-gray-400" />
            <input placeholder="Filter cards" className="w-56 rounded-md border border-gray-200 py-1.5 pl-8 pr-3 text-sm placeholder:text-gray-400" />
          </div>
          <button className="inline-flex items-center gap-1.5 rounded-md border border-gray-200 px-2.5 py-1.5 text-sm text-gray-600 hover:bg-gray-50">
            <Filter className="h-4 w-4" />
            Filters
          </button>
          <span className="ml-auto text-sm text-gray-500">
            {cards.filter((c) => c.column === 'done').length} of {cards.length} done
          </span>
        </div>
      </header>

      <main className="flex flex-1 gap-4 overflow-x-auto p-6">
        {columns.map((col) => {
          const list = cards.filter((c) => c.column === col.id);
          return (
            <section key={col.id} className="flex w-72 shrink-0 flex-col rounded-xl bg-gray-100/80">
              <div className="flex items-center justify-between px-3 py-3">
                <div className="flex items-center gap-2">
                  <span className={`h-2 w-2 rounded-full ${col.dot}`} />
                  <h2 className="text-sm font-semibold text-gray-700">{col.title}</h2>
                  <span className="rounded-full bg-white px-2 text-xs font-medium text-gray-500 shadow-sm">{list.length}</span>
                </div>
                <button className="rounded p-1 text-gray-400 hover:bg-white" aria-label={`${col.title} options`}>
                  <Ellipsis className="h-4 w-4" />
                </button>
              </div>
              <div className="flex-1 space-y-2 overflow-y-auto px-2 pb-2">
                {list.map((card) => (
                  <TaskCard key={card.id} card={card} onAdvance={advance} />
                ))}
                {list.length === 0 && (
                  <div className="rounded-lg border-2 border-dashed border-gray-200 p-6 text-center text-xs text-gray-400">Drop cards here</div>
                )}
              </div>
            </section>
          );
        })}
      </main>

      {showModal && <NewTaskModal onClose={() => setShowModal(false)} onCreate={create} />}
    </div>
  );
}
