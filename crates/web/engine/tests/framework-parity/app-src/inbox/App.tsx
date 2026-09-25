import { useMemo, useState, type FormEvent, type ReactNode } from 'react';
import {
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  EllipsisVertical,
  Inbox,
  Paperclip,
  Search,
  Send,
  Settings,
  Star,
  X,
  type IconProps,
} from '../shared/icons';

// The few Lucide icons the shared set lacks, inlined the same way.
function Icon({ className, children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

const Archive = (p: IconProps) => (
  <Icon {...p}>
    <rect width="20" height="5" x="2" y="3" rx="1" />
    <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
    <path d="M10 12h4" />
  </Icon>
);

const Trash = (p: IconProps) => (
  <Icon {...p}>
    <path d="M3 6h18" />
    <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" />
    <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
  </Icon>
);

const FileText = (p: IconProps) => (
  <Icon {...p}>
    <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
    <path d="M14 2v4a2 2 0 0 0 2 2h4" />
    <path d="M10 9H8" />
    <path d="M16 13H8" />
    <path d="M16 17H8" />
  </Icon>
);

const OctagonAlert = (p: IconProps) => (
  <Icon {...p}>
    <path d="M12 16h.01" />
    <path d="M12 8v4" />
    <path d="M15.312 2a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586l-4.688-4.688A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2z" />
  </Icon>
);

const Reply = (p: IconProps) => (
  <Icon {...p}>
    <polyline points="9 17 4 12 9 7" />
    <path d="M20 18v-2a4 4 0 0 0-4-4H4" />
  </Icon>
);

const Forward = (p: IconProps) => (
  <Icon {...p}>
    <polyline points="15 17 20 12 15 7" />
    <path d="M4 18v-2a4 4 0 0 1 4-4h12" />
  </Icon>
);

const MailOpen = (p: IconProps) => (
  <Icon {...p}>
    <path d="M21.2 8.4c.5.38.8.97.8 1.6v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V10a2 2 0 0 1 .8-1.6l8-6a2 2 0 0 1 2.4 0l8 6Z" />
    <path d="m22 10-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 10" />
  </Icon>
);

const PenLine = (p: IconProps) => (
  <Icon {...p}>
    <path d="M12 20h9" />
    <path d="M16.376 3.622a1 1 0 0 1 3.002 3.002L7.368 18.635a2 2 0 0 1-.855.506l-2.872.838a.5.5 0 0 1-.62-.62l.838-2.872a2 2 0 0 1 .506-.854z" />
  </Icon>
);

type FolderId = 'inbox' | 'starred' | 'sent' | 'drafts' | 'archive' | 'spam' | 'trash';
type Label = 'Clients' | 'Finance' | 'Team';

type Message = {
  id: number;
  folder: FolderId;
  from: string;
  email: string;
  initials: string;
  gradient: string;
  subject: string;
  body: string[];
  time: string;
  date: string;
  unread: boolean;
  starred: boolean;
  label?: Label;
  attachment?: { name: string; size: string };
};

const initialMessages: Message[] = [
  {
    id: 1,
    folder: 'inbox',
    from: 'Priya Natarajan',
    email: 'priya@northwind.io',
    initials: 'PN',
    gradient: 'from-violet-500 to-fuchsia-500',
    subject: 'Kickoff notes and next steps for the Q4 redesign',
    body: [
      'Hi Sam,',
      'Thanks again for running such a focused kickoff yesterday. I pulled together the notes and the decisions we landed on so everyone is working from the same page.',
      'The short version: we keep the current navigation for launch, move the pricing experiment to November, and the design team owns the new onboarding flow end to end. Engineering will scope the account settings rework by Friday.',
      'Could you confirm the review dates on your side? I pencilled in Tuesday the 8th for the first design review and would love to lock it before the calendars fill up.',
      'Best,\nPriya',
    ],
    time: '9:41 AM',
    date: 'Tue, Sep 24, 9:41 AM',
    unread: false,
    starred: true,
    label: 'Clients',
  },
  {
    id: 2,
    folder: 'inbox',
    from: 'Stripe',
    email: 'receipts@stripe.com',
    initials: 'S',
    gradient: 'from-indigo-500 to-sky-500',
    subject: 'Your invoice #INV-20417 from Acme Cloud is ready',
    body: [
      'Hello,',
      'Your invoice #INV-20417 for September is now available. The total of $1,248.00 will be charged to the Visa ending in 4242 on October 1.',
      'You can download the PDF below or view the itemised breakdown in your billing dashboard at any time.',
      'Thanks for your business,\nThe Acme Cloud team',
    ],
    time: '8:15 AM',
    date: 'Tue, Sep 24, 8:15 AM',
    unread: true,
    starred: false,
    label: 'Finance',
    attachment: { name: 'INV-20417.pdf', size: '184 KB' },
  },
  {
    id: 3,
    folder: 'inbox',
    from: 'Marcus Webb',
    email: 'marcus.webb@hey.com',
    initials: 'MW',
    gradient: 'from-emerald-500 to-teal-400',
    subject: 'Re: Photos from the offsite',
    body: [
      'Hey Sam!',
      'Finally got through the photos from the offsite. There are some great ones of the hike and the dinner on the last night. I uploaded everything to the shared album so grab whatever you want for the recap post.',
      'Also, the team voted and we are doing the lake house again next spring. Start practising your paddleboarding.',
      'Cheers,\nMarcus',
    ],
    time: '7:02 AM',
    date: 'Tue, Sep 24, 7:02 AM',
    unread: true,
    starred: false,
    label: 'Team',
  },
  {
    id: 4,
    folder: 'inbox',
    from: 'Elena García',
    email: 'elena@studiofolk.com',
    initials: 'EG',
    gradient: 'from-pink-500 to-orange-400',
    subject: 'Contract draft for review',
    body: [
      'Hi Sam,',
      'Attached is the revised contract with the changes from our call. The main edits are in sections 4 (payment schedule) and 7 (IP ownership). Let me know if legal has any questions.',
      'Elena',
    ],
    time: 'Yesterday',
    date: 'Mon, Sep 23, 4:37 PM',
    unread: true,
    starred: true,
    label: 'Clients',
    attachment: { name: 'Studio-Folk-MSA-v3.docx', size: '92 KB' },
  },
  {
    id: 5,
    folder: 'inbox',
    from: 'GitHub',
    email: 'noreply@github.com',
    initials: 'GH',
    gradient: 'from-gray-700 to-gray-500',
    subject: '[acme/web] Release v2.14.0 published',
    body: ['A new release v2.14.0 was published by jkato. It includes 23 merged pull requests and fixes for the checkout flow on Safari.'],
    time: 'Yesterday',
    date: 'Mon, Sep 23, 2:10 PM',
    unread: false,
    starred: false,
  },
  {
    id: 6,
    folder: 'inbox',
    from: 'Harbor Accounting',
    email: 'billing@harbor.co',
    initials: 'HA',
    gradient: 'from-amber-500 to-yellow-400',
    subject: 'Overdue invoice reminder: August retainer',
    body: [
      'Hi Sam,',
      'A friendly reminder that invoice #H-0893 for the August retainer is now 14 days overdue. Please arrange payment at your earliest convenience or reply if anything looks off.',
      'Kind regards,\nHarbor Accounting',
    ],
    time: 'Sep 22',
    date: 'Sun, Sep 22, 10:03 AM',
    unread: true,
    starred: false,
    label: 'Finance',
  },
  {
    id: 7,
    folder: 'inbox',
    from: 'Jun Kato',
    email: 'jun@acme.dev',
    initials: 'JK',
    gradient: 'from-sky-500 to-indigo-500',
    subject: 'Standup moved to 10:30 tomorrow',
    body: ['Quick heads up: the design crit ran long so standup is at 10:30 tomorrow instead of 10. Same room.'],
    time: 'Sep 21',
    date: 'Sat, Sep 21, 6:48 PM',
    unread: false,
    starred: false,
    label: 'Team',
  },
  {
    id: 8,
    folder: 'inbox',
    from: 'Figma',
    email: 'no-reply@figma.com',
    initials: 'F',
    gradient: 'from-rose-500 to-pink-500',
    subject: 'Ava Reyes commented on Onboarding v4',
    body: ['Ava Reyes left a comment: "Can we try the illustration on the left and shorten the headline to one line?"'],
    time: 'Sep 20',
    date: 'Fri, Sep 20, 11:26 AM',
    unread: false,
    starred: false,
  },
  {
    id: 9,
    folder: 'inbox',
    from: 'Linear',
    email: 'notifications@linear.app',
    initials: 'L',
    gradient: 'from-indigo-600 to-violet-500',
    subject: 'Weekly summary: 18 issues closed',
    body: ['Your team closed 18 issues and opened 11 this week. Cycle 14 is 72% complete with 4 days remaining.'],
    time: 'Sep 19',
    date: 'Thu, Sep 19, 9:00 AM',
    unread: false,
    starred: false,
  },
  {
    id: 10,
    folder: 'sent',
    from: 'Sam Carter',
    email: 'sam@acme.dev',
    initials: 'SC',
    gradient: 'from-teal-500 to-emerald-400',
    subject: 'Invoice question for September',
    body: ['Hi, could you resend the September invoice with our new billing address? Thanks!'],
    time: 'Sep 18',
    date: 'Wed, Sep 18, 3:12 PM',
    unread: false,
    starred: false,
    label: 'Finance',
  },
  {
    id: 11,
    folder: 'drafts',
    from: 'Sam Carter',
    email: 'sam@acme.dev',
    initials: 'SC',
    gradient: 'from-teal-500 to-emerald-400',
    subject: 'Offsite budget proposal',
    body: ['Draft: rough numbers for spring offsite, still waiting on the venue quote.'],
    time: 'Sep 17',
    date: 'Tue, Sep 17, 5:40 PM',
    unread: false,
    starred: false,
  },
  {
    id: 12,
    folder: 'spam',
    from: 'Prize Center',
    email: 'win@prize-center.biz',
    initials: 'PC',
    gradient: 'from-lime-500 to-green-500',
    subject: 'You have been selected!!!',
    body: ['Claim your reward today.'],
    time: 'Sep 16',
    date: 'Mon, Sep 16, 1:01 AM',
    unread: true,
    starred: false,
  },
];

const folders: { id: FolderId; name: string; icon: (p: IconProps) => JSX.Element }[] = [
  { id: 'inbox', name: 'Inbox', icon: Inbox },
  { id: 'starred', name: 'Starred', icon: Star },
  { id: 'sent', name: 'Sent', icon: Send },
  { id: 'drafts', name: 'Drafts', icon: FileText },
  { id: 'archive', name: 'Archive', icon: Archive },
  { id: 'spam', name: 'Spam', icon: OctagonAlert },
  { id: 'trash', name: 'Trash', icon: Trash },
];

const labelStyles: Record<Label, { dot: string; chip: string }> = {
  Clients: { dot: 'bg-violet-500', chip: 'bg-violet-50 text-violet-700 ring-violet-600/20' },
  Finance: { dot: 'bg-emerald-500', chip: 'bg-emerald-50 text-emerald-700 ring-emerald-600/20' },
  Team: { dot: 'bg-sky-500', chip: 'bg-sky-50 text-sky-700 ring-sky-600/20' },
};

function inFolder(m: Message, folder: FolderId) {
  return folder === 'starred' ? m.starred && m.folder !== 'spam' && m.folder !== 'trash' : m.folder === folder;
}

function Avatar({ m, size = 'h-9 w-9 text-xs' }: { m: Message; size?: string }) {
  return (
    <span
      className={`inline-flex ${size} shrink-0 items-center justify-center rounded-full bg-gradient-to-br ${m.gradient} font-semibold text-white`}
    >
      {m.initials}
    </span>
  );
}

function StarButton({ m, onToggle }: { m: Message; onToggle: (id: number) => void }) {
  return (
    <button
      type="button"
      data-star={m.id}
      aria-label={m.starred ? 'Unstar' : 'Star'}
      onClick={(e) => {
        e.stopPropagation();
        onToggle(m.id);
      }}
      className="rounded p-0.5 hover:bg-gray-100"
    >
      <Star className={`h-4 w-4 ${m.starred ? 'fill-amber-400 text-amber-400' : 'text-gray-300 hover:text-gray-400'}`} />
    </button>
  );
}

type Draft = { to: string; subject: string; body: string };
type DraftErrors = Partial<Record<keyof Draft, string>>;

function validate(d: Draft): DraftErrors {
  const errors: DraftErrors = {};
  if (!d.to.trim()) errors.to = 'Add at least one recipient.';
  else if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(d.to.trim())) errors.to = `"${d.to.trim()}" is not a valid email address.`;
  if (!d.subject.trim()) errors.subject = 'Subject is required.';
  if (d.body.trim().length < 5) errors.body = 'Write a message before sending.';
  return errors;
}

function ComposeModal({ onClose, onSend }: { onClose: () => void; onSend: (d: Draft) => void }) {
  const [draft, setDraft] = useState<Draft>({ to: '', subject: '', body: '' });
  const [submitted, setSubmitted] = useState(false);
  const errors = submitted ? validate(draft) : {};
  const count = Object.keys(errors).length;

  function submit(e: FormEvent) {
    e.preventDefault();
    setSubmitted(true);
    if (Object.keys(validate(draft)).length === 0) onSend(draft);
  }

  const field = (key: keyof Draft) => (e: { target: { value: string } }) => setDraft((d) => ({ ...d, [key]: e.target.value }));
  const border = (key: keyof Draft) =>
    errors[key] ? 'border-rose-300 focus:ring-rose-500/30' : 'border-gray-300 focus:border-indigo-500 focus:ring-indigo-500/30';

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-gray-900/40 backdrop-blur-sm" onClick={onClose} />
      <form
        onSubmit={submit}
        noValidate
        role="dialog"
        aria-modal="true"
        aria-labelledby="compose-title"
        className="relative w-full max-w-xl rounded-2xl bg-white shadow-2xl ring-1 ring-gray-900/5"
      >
        <div className="flex items-center justify-between border-b border-gray-100 px-6 py-4">
          <h2 id="compose-title" className="text-base font-semibold text-gray-900">
            New message
          </h2>
          <button type="button" onClick={onClose} className="rounded-md p-1 text-gray-400 hover:bg-gray-100" aria-label="Close">
            <X className="h-5 w-5" />
          </button>
        </div>
        {count > 0 && (
          <div className="mx-6 mt-5 flex items-start gap-2.5 rounded-lg bg-rose-50 px-3.5 py-3 text-sm text-rose-800 ring-1 ring-inset ring-rose-200">
            <CircleAlert className="mt-0.5 h-4 w-4 shrink-0 text-rose-500" />
            <span>
              {count === 1 ? 'There is 1 problem' : `There are ${count} problems`} with this message. Fix {count === 1 ? 'it' : 'them'} and try again.
            </span>
          </div>
        )}
        <div className="space-y-4 px-6 py-5">
          <div>
            <label htmlFor="compose-to" className="block text-sm font-medium text-gray-700">
              To
            </label>
            <input
              id="compose-to"
              type="email"
              autoComplete="off"
              value={draft.to}
              onChange={field('to')}
              placeholder="name@company.com"
              className={`mt-1.5 block w-full rounded-lg border px-3 py-2 text-sm shadow-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 ${border('to')}`}
            />
            {errors.to && <p className="mt-1.5 text-xs text-rose-600">{errors.to}</p>}
          </div>
          <div>
            <label htmlFor="compose-subject" className="block text-sm font-medium text-gray-700">
              Subject
            </label>
            <input
              id="compose-subject"
              autoComplete="off"
              value={draft.subject}
              onChange={field('subject')}
              placeholder="What is this about?"
              className={`mt-1.5 block w-full rounded-lg border px-3 py-2 text-sm shadow-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 ${border('subject')}`}
            />
            {errors.subject && <p className="mt-1.5 text-xs text-rose-600">{errors.subject}</p>}
          </div>
          <div>
            <label htmlFor="compose-body" className="block text-sm font-medium text-gray-700">
              Message
            </label>
            <textarea
              id="compose-body"
              rows={6}
              value={draft.body}
              onChange={field('body')}
              placeholder="Write your message…"
              className={`mt-1.5 block w-full resize-none rounded-lg border px-3 py-2 text-sm shadow-sm placeholder:text-gray-400 focus:outline-none focus:ring-2 ${border('body')}`}
            />
            {errors.body && <p className="mt-1.5 text-xs text-rose-600">{errors.body}</p>}
          </div>
        </div>
        <div className="flex items-center justify-between rounded-b-2xl bg-gray-50 px-6 py-4">
          <button type="button" className="inline-flex items-center gap-1.5 rounded-lg px-2 py-2 text-sm text-gray-500 hover:bg-gray-100" aria-label="Attach a file">
            <Paperclip className="h-4 w-4" />
            Attach
          </button>
          <div className="flex gap-3">
            <button type="button" onClick={onClose} className="rounded-lg px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100">
              Discard
            </button>
            <button
              type="submit"
              id="compose-send"
              className="inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500"
            >
              <Send className="h-4 w-4" />
              Send
            </button>
          </div>
        </div>
      </form>
    </div>
  );
}

function ReadingPane({ m, position, total, onToggleStar }: { m: Message | undefined; position: number; total: number; onToggleStar: (id: number) => void }) {
  if (!m) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center bg-white text-center">
        <span className="flex h-12 w-12 items-center justify-center rounded-full bg-gray-100">
          <MailOpen className="h-6 w-6 text-gray-400" />
        </span>
        <p className="mt-3 text-sm font-medium text-gray-900">No message selected</p>
        <p className="mt-1 text-sm text-gray-500">Choose a conversation from the list to read it here.</p>
      </div>
    );
  }
  const toolbar = [
    { label: 'Archive', icon: Archive },
    { label: 'Report spam', icon: OctagonAlert },
    { label: 'Delete', icon: Trash },
    { label: 'Mark as unread', icon: MailOpen },
  ];
  return (
    <section className="flex min-w-0 flex-1 flex-col bg-white">
      <div className="flex h-14 shrink-0 items-center justify-between border-b border-gray-200 px-6">
        <div className="flex items-center gap-1">
          {toolbar.map(({ label, icon: I }) => (
            <button key={label} aria-label={label} className="rounded-md p-2 text-gray-500 hover:bg-gray-100 hover:text-gray-700">
              <I className="h-4 w-4" />
            </button>
          ))}
        </div>
        <div className="flex items-center gap-1 text-sm text-gray-500">
          <span className="mr-2">{position} of {total}</span>
          <button aria-label="Newer" className="rounded-md p-2 hover:bg-gray-100">
            <ChevronLeft className="h-4 w-4" />
          </button>
          <button aria-label="Older" className="rounded-md p-2 hover:bg-gray-100">
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-8 py-6">
        <div className="flex items-start justify-between gap-4">
          <h2 className="text-xl font-semibold leading-snug text-gray-900">{m.subject}</h2>
          <StarButton m={m} onToggle={onToggleStar} />
        </div>
        {m.label && (
          <span className={`mt-2 inline-flex items-center rounded-md px-2 py-0.5 text-xs font-medium ring-1 ring-inset ${labelStyles[m.label].chip}`}>
            {m.label}
          </span>
        )}
        <div className="mt-6 flex items-start gap-3">
          <Avatar m={m} size="h-10 w-10 text-sm" />
          <div className="min-w-0 flex-1">
            <div className="flex items-baseline justify-between gap-3">
              <p className="truncate text-sm font-semibold text-gray-900">
                {m.from} <span className="font-normal text-gray-500">&lt;{m.email}&gt;</span>
              </p>
              <span className="shrink-0 text-xs text-gray-500">{m.date}</span>
            </div>
            <p className="mt-0.5 text-xs text-gray-500">to me</p>
          </div>
          <button aria-label="More" className="rounded-md p-1 text-gray-400 hover:bg-gray-100">
            <EllipsisVertical className="h-4 w-4" />
          </button>
        </div>
        <div className="mt-6 space-y-4 text-sm leading-relaxed text-gray-700">
          {m.body.map((p, i) => (
            <p key={i} className="whitespace-pre-line">
              {p}
            </p>
          ))}
        </div>
        {m.attachment && (
          <div className="mt-6 flex w-72 items-center gap-3 rounded-lg border border-gray-200 p-3">
            <span className="flex h-10 w-10 items-center justify-center rounded-md bg-rose-50">
              <FileText className="h-5 w-5 text-rose-500" />
            </span>
            <div className="min-w-0">
              <p className="truncate text-sm font-medium text-gray-900">{m.attachment.name}</p>
              <p className="text-xs text-gray-500">{m.attachment.size}</p>
            </div>
          </div>
        )}
        <div className="mt-8 flex gap-2">
          <button className="inline-flex items-center gap-1.5 rounded-lg border border-gray-300 px-3.5 py-2 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50">
            <Reply className="h-4 w-4" />
            Reply
          </button>
          <button className="inline-flex items-center gap-1.5 rounded-lg border border-gray-300 px-3.5 py-2 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50">
            <Forward className="h-4 w-4" />
            Forward
          </button>
        </div>
      </div>
    </section>
  );
}

export default function App() {
  const [messages, setMessages] = useState<Message[]>(initialMessages);
  const [folder, setFolder] = useState<FolderId>('inbox');
  const [selected, setSelected] = useState<number | null>(1);
  const [query, setQuery] = useState('');
  const [composing, setComposing] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return messages.filter(
      (m) =>
        inFolder(m, folder) &&
        (!q || m.from.toLowerCase().includes(q) || m.subject.toLowerCase().includes(q) || m.body.join(' ').toLowerCase().includes(q)),
    );
  }, [messages, folder, query]);

  const unread = (f: FolderId) => messages.filter((m) => inFolder(m, f) && m.unread).length;
  const current = messages.find((m) => m.id === selected && visible.some((v) => v.id === m.id));

  function open(id: number) {
    setSelected(id);
    setMessages((ms) => ms.map((m) => (m.id === id ? { ...m, unread: false } : m)));
  }

  function toggleStar(id: number) {
    setMessages((ms) => ms.map((m) => (m.id === id ? { ...m, starred: !m.starred } : m)));
  }

  function send(d: Draft) {
    setMessages((ms) => [
      {
        id: Math.max(...ms.map((m) => m.id)) + 1,
        folder: 'sent',
        from: 'Sam Carter',
        email: 'sam@acme.dev',
        initials: 'SC',
        gradient: 'from-teal-500 to-emerald-400',
        subject: d.subject,
        body: d.body.split('\n\n'),
        time: 'Now',
        date: 'Tue, Sep 24, 9:45 AM',
        unread: false,
        starred: false,
      },
      ...ms,
    ]);
    setComposing(false);
    setToast(`Message sent to ${d.to}`);
  }

  const folderName = folders.find((f) => f.id === folder)!.name;

  return (
    <div className="flex h-screen overflow-hidden bg-gray-50 font-sans text-gray-900">
      <aside className="flex w-60 shrink-0 flex-col border-r border-gray-200 bg-gray-50">
        <div className="flex h-14 items-center gap-2.5 px-5">
          <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-indigo-600 text-white shadow-sm">
            <Inbox className="h-4 w-4" />
          </span>
          <span className="text-base font-semibold text-gray-900">Relay Mail</span>
        </div>
        <div className="px-4 pb-4 pt-2">
          <button
            id="compose"
            onClick={() => setComposing(true)}
            className="flex w-full items-center justify-center gap-2 rounded-lg bg-indigo-600 px-3.5 py-2.5 text-sm font-medium text-white shadow-sm hover:bg-indigo-500"
          >
            <PenLine className="h-4 w-4" />
            Compose
          </button>
        </div>
        <nav className="flex-1 space-y-0.5 overflow-y-auto px-3">
          {folders.map(({ id, name, icon: I }) => {
            const active = id === folder;
            const n = id === 'drafts' ? messages.filter((m) => m.folder === 'drafts').length : unread(id);
            return (
              <button
                key={id}
                data-folder={id}
                onClick={() => {
                  setFolder(id);
                  setSelected(null);
                }}
                className={`flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm ${
                  active ? 'bg-indigo-50 font-semibold text-indigo-700' : 'text-gray-600 hover:bg-gray-100 hover:text-gray-900'
                }`}
              >
                <I className={`h-4 w-4 ${active ? 'text-indigo-600' : 'text-gray-400'}`} />
                <span className="flex-1 text-left">{name}</span>
                {n > 0 && (
                  <span
                    className={`rounded-full px-2 py-0.5 text-xs font-medium ${
                      active ? 'bg-indigo-600 text-white' : 'bg-gray-200 text-gray-600'
                    }`}
                  >
                    {n}
                  </span>
                )}
              </button>
            );
          })}
          <p className="px-3 pb-2 pt-6 text-xs font-semibold uppercase tracking-wide text-gray-400">Labels</p>
          {(Object.keys(labelStyles) as Label[]).map((l) => (
            <div key={l} className="flex items-center gap-3 rounded-md px-3 py-2 text-sm text-gray-600">
              <span className={`h-2 w-2 rounded-full ${labelStyles[l].dot}`} />
              {l}
            </div>
          ))}
        </nav>
        <div className="flex items-center gap-3 border-t border-gray-200 px-4 py-3">
          <span className="inline-flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-br from-teal-500 to-emerald-400 text-xs font-semibold text-white">
            SC
          </span>
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium text-gray-900">Sam Carter</p>
            <p className="truncate text-xs text-gray-500">sam@acme.dev</p>
          </div>
          <button aria-label="Settings" className="rounded-md p-1.5 text-gray-400 hover:bg-gray-100">
            <Settings className="h-4 w-4" />
          </button>
        </div>
      </aside>

      <section className="flex w-[400px] shrink-0 flex-col border-r border-gray-200 bg-white">
        <div className="flex h-14 shrink-0 items-center gap-3 border-b border-gray-200 px-4">
          <div className="relative flex-1">
            <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-gray-400" />
            <input
              id="search"
              type="search"
              autoComplete="off"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search mail"
              className="w-full rounded-lg border border-gray-200 bg-gray-50 py-2 pl-9 pr-3 text-sm placeholder:text-gray-400 focus:border-indigo-500 focus:bg-white focus:outline-none focus:ring-2 focus:ring-indigo-500/30"
            />
          </div>
        </div>
        <div className="flex shrink-0 items-center justify-between px-4 pb-2 pt-3">
          <h1 className="text-sm font-semibold text-gray-900">{query.trim() ? `Results for “${query.trim()}”` : folderName}</h1>
          <span className="text-xs text-gray-500">
            {visible.length} {visible.length === 1 ? 'conversation' : 'conversations'}
          </span>
        </div>
        <ul className="flex-1 divide-y divide-gray-100 overflow-y-auto">
          {visible.map((m) => {
            const active = m.id === current?.id;
            return (
              <li key={m.id}>
                <div
                  role="button"
                  tabIndex={0}
                  data-message={m.id}
                  onClick={() => open(m.id)}
                  className={`relative flex cursor-pointer gap-3 px-4 py-3 ${active ? 'bg-indigo-50/70' : 'hover:bg-gray-50'}`}
                >
                  {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-indigo-600" />}
                  <Avatar m={m} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      {m.unread && <span className="h-2 w-2 shrink-0 rounded-full bg-indigo-600" aria-label="Unread" />}
                      <span className={`truncate text-sm ${m.unread ? 'font-semibold text-gray-900' : 'font-medium text-gray-700'}`}>{m.from}</span>
                      <span className={`ml-auto shrink-0 text-xs ${m.unread ? 'font-semibold text-indigo-600' : 'text-gray-400'}`}>{m.time}</span>
                    </div>
                    <div className="mt-0.5 flex items-center gap-2">
                      <p className={`min-w-0 flex-1 truncate text-sm ${m.unread ? 'font-medium text-gray-900' : 'text-gray-600'}`}>{m.subject}</p>
                      <StarButton m={m} onToggle={toggleStar} />
                    </div>
                    <div className="mt-0.5 flex items-center gap-2">
                      <p className="min-w-0 flex-1 truncate text-xs text-gray-500">{m.body.join(' ')}</p>
                      {m.attachment && <Paperclip className="h-3.5 w-3.5 shrink-0 text-gray-400" />}
                      {m.label && <span className={`h-2 w-2 shrink-0 rounded-full ${labelStyles[m.label].dot}`} title={m.label} />}
                    </div>
                  </div>
                </div>
              </li>
            );
          })}
          {visible.length === 0 && (
            <li className="px-6 py-16 text-center">
              <Search className="mx-auto h-6 w-6 text-gray-300" />
              <p className="mt-2 text-sm font-medium text-gray-900">No messages found</p>
              <p className="mt-1 text-xs text-gray-500">Try a different search or folder.</p>
            </li>
          )}
        </ul>
      </section>

      <ReadingPane m={current} position={visible.findIndex((v) => v.id === current?.id) + 1} total={visible.length} onToggleStar={toggleStar} />

      {toast && (
        <div className="fixed bottom-6 left-1/2 z-40 flex -translate-x-1/2 items-center gap-3 rounded-lg bg-gray-900 px-4 py-3 text-sm text-white shadow-lg">
          <Send className="h-4 w-4 text-emerald-400" />
          {toast}
          <button onClick={() => setToast(null)} aria-label="Dismiss" className="text-gray-400 hover:text-white">
            <X className="h-4 w-4" />
          </button>
        </div>
      )}

      {composing && <ComposeModal onClose={() => setComposing(false)} onSend={send} />}
    </div>
  );
}

