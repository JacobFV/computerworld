import { useEffect, useRef, useState, type FormEvent } from 'react';
import { CheckCheck, EllipsisVertical, Paperclip, Phone, Search, Send, Smile, Video } from '../shared/icons';
import { conversations as seed, me, type Conversation, type Message } from './data';

function Avatar({ c, size = 'h-10 w-10', dot = false }: { c: Pick<Conversation, 'initials' | 'color' | 'online'>; size?: string; dot?: boolean }) {
  return (
    <div className="relative shrink-0">
      <div className={`flex ${size} items-center justify-center rounded-full ${c.color} text-sm font-semibold text-white`}>{c.initials}</div>
      {dot && c.online && <span className="absolute bottom-0 right-0 block h-2.5 w-2.5 rounded-full bg-emerald-500 ring-2 ring-white" />}
    </div>
  );
}

function ConversationList({ items, activeId, onSelect }: { items: Conversation[]; activeId: string; onSelect: (id: string) => void }) {
  return (
    <aside className="flex w-80 shrink-0 flex-col border-r border-slate-200 bg-white">
      <div className="border-b border-slate-200 p-4">
        <div className="flex items-center justify-between">
          <h1 className="text-lg font-semibold text-slate-900">Messages</h1>
          <span className="rounded-full bg-indigo-100 px-2 py-0.5 text-xs font-semibold text-indigo-700">
            {items.reduce((n, c) => n + c.unread, 0)} new
          </span>
        </div>
        <div className="relative mt-3">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
          <input placeholder="Search" className="w-full rounded-full bg-slate-100 py-2 pl-9 pr-4 text-sm text-slate-900 placeholder:text-slate-400 focus:outline-none focus:ring-2 focus:ring-indigo-500" />
        </div>
      </div>
      <ul className="flex-1 overflow-y-auto">
        {items.map((c) => {
          const last = c.messages[c.messages.length - 1];
          const active = c.id === activeId;
          return (
            <li key={c.id}>
              <button
                data-conversation={c.id}
                onClick={() => onSelect(c.id)}
                className={`flex w-full items-center gap-3 border-l-2 px-4 py-3 text-left ${
                  active ? 'border-indigo-500 bg-indigo-50/60' : 'border-transparent hover:bg-slate-50'
                }`}
              >
                <Avatar c={c} dot />
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline justify-between gap-2">
                    <p className="truncate text-sm font-medium text-slate-900">{c.name}</p>
                    <span className="shrink-0 text-xs text-slate-400">{last.time}</span>
                  </div>
                  <div className="mt-0.5 flex items-center justify-between gap-2">
                    <p className={`truncate text-sm ${c.unread ? 'font-medium text-slate-700' : 'text-slate-500'}`}>
                      {last.from === me ? 'You: ' : ''}
                      {last.text}
                    </p>
                    {c.unread > 0 && (
                      <span className="flex h-5 min-w-[1.25rem] items-center justify-center rounded-full bg-indigo-600 px-1.5 text-[11px] font-semibold text-white">
                        {c.unread}
                      </span>
                    )}
                  </div>
                </div>
              </button>
            </li>
          );
        })}
      </ul>
    </aside>
  );
}

function Bubble({ m, mine }: { m: Message; mine: boolean }) {
  return (
    <div className={`flex ${mine ? 'justify-end' : 'justify-start'}`}>
      <div className={`max-w-md ${mine ? 'items-end' : 'items-start'} flex flex-col`}>
        <div
          className={`rounded-2xl px-4 py-2 text-sm leading-relaxed shadow-sm ${
            mine ? 'rounded-br-md bg-indigo-600 text-white' : 'rounded-bl-md bg-white text-slate-800 ring-1 ring-slate-200'
          }`}
        >
          {m.text}
        </div>
        <span className="mt-1 flex items-center gap-1 text-[11px] text-slate-400">
          {m.time}
          {mine && <CheckCheck className="h-3.5 w-3.5 text-indigo-500" />}
        </span>
      </div>
    </div>
  );
}

export default function App() {
  const [items, setItems] = useState(seed);
  const [activeId, setActiveId] = useState(seed[0].id);
  const [draft, setDraft] = useState('');
  const scroller = useRef<HTMLDivElement>(null);
  const active = items.find((c) => c.id === activeId)!;

  useEffect(() => {
    const el = scroller.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [activeId, active.messages.length]);

  function select(id: string) {
    setActiveId(id);
    setItems((cs) => cs.map((c) => (c.id === id ? { ...c, unread: 0 } : c)));
  }

  function send(e: FormEvent) {
    e.preventDefault();
    const text = draft.trim();
    if (!text) return;
    setItems((cs) => cs.map((c) => (c.id === activeId ? { ...c, messages: [...c.messages, { from: me, text, time: '10:42' }] } : c)));
    setDraft('');
  }

  return (
    <div className="flex h-screen bg-slate-50 font-sans text-slate-900">
      <ConversationList items={items} activeId={activeId} onSelect={select} />
      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-16 shrink-0 items-center gap-3 border-b border-slate-200 bg-white px-6">
          <Avatar c={active} size="h-9 w-9" />
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-sm font-semibold text-slate-900">{active.name}</h2>
            <p className={`text-xs ${active.online ? 'text-emerald-600' : 'text-slate-400'}`}>{active.online ? 'Online' : 'Last seen yesterday'}</p>
          </div>
          <div className="flex items-center gap-1 text-slate-500">
            {[Phone, Video, EllipsisVertical].map((Icon, i) => (
              <button key={i} className="rounded-lg p-2 hover:bg-slate-100">
                <Icon className="h-5 w-5" />
              </button>
            ))}
          </div>
        </header>

        <div ref={scroller} id="thread" className="flex-1 space-y-4 overflow-y-auto px-6 py-6">
          <div className="flex items-center gap-4">
            <div className="h-px flex-1 bg-slate-200" />
            <span className="text-xs font-medium text-slate-400">Today</span>
            <div className="h-px flex-1 bg-slate-200" />
          </div>
          {active.messages.map((m, i) => (
            <Bubble key={i} m={m} mine={m.from === me} />
          ))}
        </div>

        <form onSubmit={send} className="shrink-0 border-t border-slate-200 bg-white p-4">
          <div className="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 focus-within:border-indigo-400 focus-within:ring-2 focus-within:ring-indigo-100">
            <button type="button" className="text-slate-400 hover:text-slate-600" aria-label="Attach">
              <Paperclip className="h-5 w-5" />
            </button>
            <input
              id="composer"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={`Message ${active.name.split(' ')[0]}…`}
              className="flex-1 bg-transparent py-1 text-sm placeholder:text-slate-400 focus:outline-none"
            />
            <button type="button" className="text-slate-400 hover:text-slate-600" aria-label="Emoji">
              <Smile className="h-5 w-5" />
            </button>
            <button
              type="submit"
              id="send"
              disabled={!draft.trim()}
              className="flex h-8 w-8 items-center justify-center rounded-lg bg-indigo-600 text-white disabled:bg-slate-300"
              aria-label="Send"
            >
              <Send className="h-4 w-4" />
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}
