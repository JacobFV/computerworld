import { useState, type FormEvent, type ReactNode } from 'react';
import { Bell, Camera, CircleAlert, CircleCheck, Globe, Lock, Mail, User, X } from '../shared/icons';

type Profile = {
  name: string;
  username: string;
  email: string;
  website: string;
  bio: string;
  timezone: string;
};

type Errors = Partial<Record<keyof Profile, string>>;

const initialProfile: Profile = {
  name: 'Maya Chen',
  username: 'mayachen',
  email: 'maya@studio.design',
  website: '',
  bio: 'Product designer. Previously at Figma and Linear. I like type, trains and tidy spreadsheets.',
  timezone: 'Europe/Lisbon',
};

const BIO_LIMIT = 160;

function validate(p: Profile): Errors {
  const errors: Errors = {};
  if (!p.name.trim()) errors.name = 'Your name is required.';
  if (!/^[a-z0-9_]+$/i.test(p.username)) errors.username = 'Usernames can only contain letters, numbers and underscores.';
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(p.email)) errors.email = 'Enter a valid email address.';
  if (p.website && !/^https:\/\/\S+\.\S+$/.test(p.website)) errors.website = 'Enter a URL that starts with https://';
  if (p.bio.length > BIO_LIMIT) errors.bio = `Keep your bio under ${BIO_LIMIT} characters.`;
  return errors;
}

const tabs = [
  { id: 'profile', label: 'Profile', icon: User },
  { id: 'account', label: 'Account', icon: Lock },
  { id: 'notifications', label: 'Notifications', icon: Bell },
] as const;

type TabId = (typeof tabs)[number]['id'];

function Field({ label, htmlFor, hint, error, children }: { label: string; htmlFor: string; hint?: string; error?: string; children: ReactNode }) {
  return (
    <div>
      <label htmlFor={htmlFor} className="block text-sm font-medium text-gray-900">
        {label}
      </label>
      <div className="relative mt-2">{children}</div>
      {error ? (
        <p className="mt-2 flex items-center gap-1.5 text-sm text-red-600" id={`${htmlFor}-error`}>
          <CircleAlert className="h-4 w-4 shrink-0" />
          {error}
        </p>
      ) : (
        hint && <p className="mt-2 text-sm text-gray-500">{hint}</p>
      )}
    </div>
  );
}

function inputClass(error?: string) {
  return `block w-full rounded-md border-0 px-3 py-2 text-sm text-gray-900 shadow-sm ring-1 ring-inset placeholder:text-gray-400 focus:ring-2 focus:ring-inset ${
    error ? 'pr-10 text-red-900 ring-red-300 focus:ring-red-500' : 'ring-gray-300 focus:ring-indigo-600'
  }`;
}

function ProfileForm({ onSaved }: { onSaved: () => void }) {
  const [profile, setProfile] = useState(initialProfile);
  const [errors, setErrors] = useState<Errors>({});
  const [submitted, setSubmitted] = useState(false);

  function update<K extends keyof Profile>(key: K, value: Profile[K]) {
    const next = { ...profile, [key]: value };
    setProfile(next);
    if (submitted) setErrors(validate(next));
  }

  function submit(e: FormEvent) {
    e.preventDefault();
    const found = validate(profile);
    setErrors(found);
    setSubmitted(true);
    if (Object.keys(found).length === 0) onSaved();
  }

  const errorCount = Object.keys(errors).length;

  return (
    <form onSubmit={submit} noValidate className="divide-y divide-gray-200 rounded-xl bg-white shadow-sm ring-1 ring-gray-900/5">
      <div className="px-8 py-6">
        <h2 className="text-base font-semibold text-gray-900">Public profile</h2>
        <p className="mt-1 text-sm text-gray-500">This information will be shown on your profile and in comments.</p>

        {errorCount > 0 && (
          <div className="mt-6 rounded-lg border border-red-200 bg-red-50 p-4" role="alert">
            <div className="flex gap-3">
              <CircleAlert className="h-5 w-5 shrink-0 text-red-500" />
              <div>
                <h3 className="text-sm font-medium text-red-800">
                  There {errorCount === 1 ? 'is 1 problem' : `are ${errorCount} problems`} with your profile
                </h3>
                <ul className="mt-2 list-disc space-y-1 pl-5 text-sm text-red-700">
                  {Object.values(errors).map((m) => (
                    <li key={m}>{m}</li>
                  ))}
                </ul>
              </div>
            </div>
          </div>
        )}

        <div className="mt-6 flex items-center gap-5">
          <div className="relative">
            <div className="flex h-16 w-16 items-center justify-center rounded-full bg-gradient-to-br from-fuchsia-500 via-purple-500 to-indigo-500 text-xl font-semibold text-white">
              MC
            </div>
            <span className="absolute -bottom-0.5 -right-0.5 flex h-6 w-6 items-center justify-center rounded-full bg-white text-gray-600 shadow ring-1 ring-gray-200">
              <Camera className="h-3.5 w-3.5" />
            </span>
          </div>
          <div>
            <div className="flex gap-3">
              <button type="button" className="rounded-md bg-white px-3 py-1.5 text-sm font-medium text-gray-900 shadow-sm ring-1 ring-inset ring-gray-300 hover:bg-gray-50">
                Change photo
              </button>
              <button type="button" className="rounded-md px-3 py-1.5 text-sm font-medium text-gray-600 hover:text-gray-900">
                Remove
              </button>
            </div>
            <p className="mt-2 text-xs text-gray-500">JPG, PNG or GIF. 1 MB max.</p>
          </div>
        </div>

        <div className="mt-8 grid grid-cols-6 gap-x-6 gap-y-6">
          <div className="col-span-3">
            <Field label="Full name" htmlFor="name" error={errors.name}>
              <input id="name" value={profile.name} onChange={(e) => update('name', e.target.value)} className={inputClass(errors.name)} />
            </Field>
          </div>
          <div className="col-span-3">
            <Field label="Username" htmlFor="username" error={errors.username}>
              <div className="flex rounded-md shadow-sm">
                <span className="inline-flex items-center rounded-l-md border border-r-0 border-gray-300 bg-gray-50 px-3 text-sm text-gray-500">studio.design/</span>
                <input
                  id="username"
                  value={profile.username}
                  onChange={(e) => update('username', e.target.value)}
                  className={`${inputClass(errors.username)} rounded-l-none`}
                />
              </div>
            </Field>
          </div>
          <div className="col-span-4">
            <Field label="Email address" htmlFor="email" error={errors.email} hint="We'll only use this to send you receipts.">
              <Mail className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-gray-400" />
              <input id="email" type="email" value={profile.email} onChange={(e) => update('email', e.target.value)} className={`${inputClass(errors.email)} pl-9`} />
            </Field>
          </div>
          <div className="col-span-2">
            <Field label="Time zone" htmlFor="timezone">
              <select id="timezone" value={profile.timezone} onChange={(e) => update('timezone', e.target.value)} className={inputClass()}>
                <option>Europe/Lisbon</option>
                <option>America/New_York</option>
                <option>Asia/Tokyo</option>
              </select>
            </Field>
          </div>
          <div className="col-span-6">
            <Field label="Website" htmlFor="website" error={errors.website}>
              <Globe className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-gray-400" />
              <input
                id="website"
                value={profile.website}
                placeholder="https://example.com"
                onChange={(e) => update('website', e.target.value)}
                className={`${inputClass(errors.website)} pl-9`}
              />
              {errors.website && <CircleAlert className="pointer-events-none absolute right-3 top-2.5 h-4 w-4 text-red-500" />}
            </Field>
          </div>
          <div className="col-span-6">
            <Field label="About" htmlFor="bio" error={errors.bio}>
              <textarea id="bio" rows={3} value={profile.bio} onChange={(e) => update('bio', e.target.value)} className={inputClass(errors.bio)} />
            </Field>
            <p className={`mt-1 text-right text-xs ${profile.bio.length > BIO_LIMIT ? 'text-red-600' : 'text-gray-400'}`}>
              {profile.bio.length}/{BIO_LIMIT}
            </p>
          </div>
        </div>
      </div>
      <div className="sticky bottom-0 flex items-center justify-end gap-3 rounded-b-xl bg-white/90 px-8 py-4 backdrop-blur">
        <button type="button" className="rounded-md px-3 py-2 text-sm font-semibold text-gray-700 hover:bg-gray-50">
          Cancel
        </button>
        <button type="submit" id="save" className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-semibold text-white shadow-sm hover:bg-indigo-500">
          Save changes
        </button>
      </div>
    </form>
  );
}

function Toggle({ id, checked, onChange }: { id: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ${
        checked ? 'bg-indigo-600' : 'bg-gray-200'
      }`}
    >
      <span
        className={`pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ${
          checked ? 'translate-x-5' : 'translate-x-0'
        }`}
      />
    </button>
  );
}

const notificationOptions = [
  { id: 'comments', title: 'Comments', body: 'When someone comments on a file you own.' },
  { id: 'mentions', title: 'Mentions', body: 'When someone @mentions you anywhere.' },
  { id: 'digest', title: 'Weekly digest', body: 'A summary of activity across your projects, every Monday.' },
  { id: 'product', title: 'Product updates', body: 'Occasional news about new features.' },
];

function Notifications() {
  const [on, setOn] = useState<Record<string, boolean>>({ comments: true, mentions: true, digest: false, product: false });
  return (
    <div className="rounded-xl bg-white shadow-sm ring-1 ring-gray-900/5">
      <div className="px-8 py-6">
        <h2 className="text-base font-semibold text-gray-900">Email notifications</h2>
        <p className="mt-1 text-sm text-gray-500">Choose what we email you about. You can unsubscribe at any time.</p>
      </div>
      <ul className="divide-y divide-gray-100 border-t border-gray-100">
        {notificationOptions.map((o) => (
          <li key={o.id} className="flex items-center justify-between gap-6 px-8 py-4">
            <div>
              <p className="text-sm font-medium text-gray-900">{o.title}</p>
              <p className="text-sm text-gray-500">{o.body}</p>
            </div>
            <Toggle id={`toggle-${o.id}`} checked={on[o.id]} onChange={(v) => setOn({ ...on, [o.id]: v })} />
          </li>
        ))}
      </ul>
    </div>
  );
}

function Account() {
  return (
    <div className="space-y-6">
      <div className="rounded-xl bg-white px-8 py-6 shadow-sm ring-1 ring-gray-900/5">
        <h2 className="text-base font-semibold text-gray-900">Password</h2>
        <p className="mt-1 text-sm text-gray-500">Last changed 3 months ago.</p>
        <button type="button" className="mt-4 rounded-md bg-white px-3 py-2 text-sm font-semibold text-gray-900 shadow-sm ring-1 ring-inset ring-gray-300">
          Change password
        </button>
      </div>
      <div className="rounded-xl border border-red-200 bg-white px-8 py-6">
        <h2 className="text-base font-semibold text-red-700">Delete account</h2>
        <p className="mt-1 text-sm text-gray-500">Permanently remove your account and all of its content. This cannot be undone.</p>
        <button type="button" className="mt-4 rounded-md bg-red-600 px-3 py-2 text-sm font-semibold text-white shadow-sm hover:bg-red-500">
          Delete my account
        </button>
      </div>
    </div>
  );
}

export default function App() {
  const [tab, setTab] = useState<TabId>('profile');
  const [toast, setToast] = useState(false);

  return (
    <div className="min-h-screen bg-gray-50 font-sans">
      <div className="mx-auto max-w-5xl px-8 py-10">
        <h1 className="text-2xl font-bold tracking-tight text-gray-900">Settings</h1>
        <p className="mt-1 text-sm text-gray-500">Manage your profile, account and notifications.</p>
        <div className="mt-8 flex gap-10">
          <nav className="w-52 shrink-0 space-y-1">
            {tabs.map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                id={`tab-${id}`}
                onClick={() => setTab(id)}
                className={`flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium ${
                  tab === id ? 'bg-white text-indigo-600 shadow-sm ring-1 ring-gray-900/5' : 'text-gray-600 hover:bg-gray-100 hover:text-gray-900'
                }`}
              >
                <Icon className={`h-4 w-4 ${tab === id ? 'text-indigo-600' : 'text-gray-400'}`} />
                {label}
              </button>
            ))}
          </nav>
          <div className="min-w-0 flex-1">
            {tab === 'profile' && <ProfileForm onSaved={() => setToast(true)} />}
            {tab === 'account' && <Account />}
            {tab === 'notifications' && <Notifications />}
          </div>
        </div>
      </div>

      {toast && (
        <div className="fixed bottom-6 right-6 z-50 flex w-80 items-start gap-3 rounded-xl bg-white p-4 shadow-lg ring-1 ring-black/5" role="status">
          <CircleCheck className="h-5 w-5 shrink-0 text-emerald-500" />
          <div className="flex-1">
            <p className="text-sm font-medium text-gray-900">Profile saved</p>
            <p className="mt-1 text-sm text-gray-500">Your changes are live.</p>
          </div>
          <button onClick={() => setToast(false)} className="rounded-md text-gray-400 hover:text-gray-500" aria-label="Dismiss">
            <X className="h-4 w-4" />
          </button>
        </div>
      )}
    </div>
  );
}
