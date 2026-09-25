import { useMemo, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react';
import { ChevronLeft, ChevronRight, Clock, Ellipsis, Plus, Search, X } from '../shared/icons';

// ---------------------------------------------------------------------------
// Icons (Lucide 0.460.0, ISC) the player needs beyond the shared set.
// ---------------------------------------------------------------------------

type IconProps = { className?: string; filled?: boolean };

function Icon({ className, filled, children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill={filled ? 'currentColor' : 'none'}
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

const Play = (p: IconProps) => (
  <Icon {...p} filled>
    <polygon points="6 3 20 12 6 21 6 3" />
  </Icon>
);
const Pause = (p: IconProps) => (
  <Icon {...p} filled>
    <rect x="14" y="4" width="4" height="16" rx="1" />
    <rect x="6" y="4" width="4" height="16" rx="1" />
  </Icon>
);
const SkipBack = (p: IconProps) => (
  <Icon {...p} filled>
    <polygon points="19 20 9 12 19 4 19 20" />
    <line x1="5" x2="5" y1="19" y2="5" />
  </Icon>
);
const SkipForward = (p: IconProps) => (
  <Icon {...p} filled>
    <polygon points="5 4 15 12 5 20 5 4" />
    <line x1="19" x2="19" y1="5" y2="19" />
  </Icon>
);
const Shuffle = (p: IconProps) => (
  <Icon {...p}>
    <path d="m18 14 4 4-4 4" />
    <path d="m18 2 4 4-4 4" />
    <path d="M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-7.6a4 4 0 0 1 3.3-1.7H22" />
    <path d="M2 6h1.972a4 4 0 0 1 3.6 2.2" />
    <path d="M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45" />
  </Icon>
);
const Repeat = (p: IconProps) => (
  <Icon {...p}>
    <path d="m17 2 4 4-4 4" />
    <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
    <path d="m7 22-4-4 4-4" />
    <path d="M21 13v1a4 4 0 0 1-4 4H3" />
  </Icon>
);
const VolumeIcon = ({ level, className }: { level: number; className?: string }) => (
  <Icon className={className}>
    <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
    {level === 0 ? (
      <>
        <line x1="22" x2="16" y1="9" y2="15" />
        <line x1="16" x2="22" y1="9" y2="15" />
      </>
    ) : (
      <>
        <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
        {level > 50 && <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />}
      </>
    )}
  </Icon>
);
const ListMusic = (p: IconProps) => (
  <Icon {...p}>
    <path d="M21 15V6" />
    <path d="M18.5 18a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z" />
    <path d="M12 12H3" />
    <path d="M16 6H3" />
    <path d="M12 18H3" />
  </Icon>
);
const Library = (p: IconProps) => (
  <Icon {...p}>
    <path d="m16 6 4 14" />
    <path d="M12 6v14" />
    <path d="M8 8v12" />
    <path d="M4 4v16" />
  </Icon>
);
const House = (p: IconProps) => (
  <Icon {...p}>
    <path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8" />
    <path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
  </Icon>
);
const MonitorSpeaker = (p: IconProps) => (
  <Icon {...p}>
    <path d="M5.5 20H8" />
    <path d="M17 9h.01" />
    <rect width="10" height="16" x="12" y="4" rx="2" />
    <path d="M8 6H4a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h4" />
    <circle cx="17" cy="15" r="1" />
  </Icon>
);
const Mic = (p: IconProps) => (
  <Icon {...p}>
    <path d="m12 8-9.04 9.06a2.82 2.82 0 1 0 3.98 3.98L16 12" />
    <circle cx="17" cy="7" r="5" />
  </Icon>
);
const Music = (p: IconProps) => (
  <Icon {...p}>
    <path d="M9 18V5l12-2v13" />
    <circle cx="6" cy="18" r="3" />
    <circle cx="18" cy="16" r="3" />
  </Icon>
);
const Heart = (p: IconProps) => (
  <Icon {...p}>
    <path d="M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z" />
  </Icon>
);

// ---------------------------------------------------------------------------
// Library data. Everything is fixed so every render of a state is identical.
// ---------------------------------------------------------------------------

type Track = { id: number; title: string; artist: string; album: string; seconds: number };

type Playlist = {
  id: string;
  name: string;
  description: string;
  owner: string;
  likes: string;
  cover: string; // gradient stops for the cover art box
  wash: string; // the header's tinted background
  tracks: Track[];
};

const playlists: Playlist[] = [
  {
    id: 'late-night',
    name: 'Late Night Drive',
    description: 'Synthwave, city pop and slow-burning grooves for empty highways after midnight.',
    owner: 'Maya Chen',
    likes: '2,418',
    cover: 'from-indigo-500 via-purple-600 to-pink-500',
    wash: 'from-indigo-800',
    tracks: [
      { id: 101, title: 'Neon Harbor', artist: 'Kavya Lights', album: 'Afterglow Avenue', seconds: 228 },
      { id: 102, title: 'Glass Skyline', artist: 'The Midnight Parade', album: 'Skyline Tapes', seconds: 254 },
      { id: 103, title: 'Velvet Overpass', artist: 'Sora Kimura', album: 'Plastic Moon', seconds: 197 },
      { id: 104, title: 'Chrome Hearts Club', artist: 'Delta Fontaine', album: 'Night Market', seconds: 241 },
      { id: 105, title: 'Tail Lights in Rain', artist: 'Kavya Lights', album: 'Afterglow Avenue', seconds: 276 },
      { id: 106, title: 'Mirror Tunnel', artist: 'Halcyon Drive', album: 'Signal Loss', seconds: 213 },
      { id: 107, title: 'Palm Static', artist: 'Juno & The Coast', album: 'Low Tide FM', seconds: 188 },
      { id: 108, title: 'Last Exit to Osaka', artist: 'Sora Kimura', album: 'Plastic Moon', seconds: 302 },
      { id: 109, title: 'Satellite Hotel', artist: 'The Midnight Parade', album: 'Skyline Tapes', seconds: 234 },
      { id: 110, title: 'Four A.M. Diner', artist: 'Delta Fontaine', album: 'Night Market', seconds: 219 },
    ],
  },
  {
    id: 'focus',
    name: 'Deep Focus',
    description: 'Ambient textures and soft piano to keep you in the zone for hours.',
    owner: 'Soundwave',
    likes: '48,902',
    cover: 'from-emerald-400 via-teal-500 to-cyan-700',
    wash: 'from-teal-800',
    tracks: [
      { id: 201, title: 'Rain on Cedar', artist: 'Ólafur Brenna', album: 'Quiet Rooms', seconds: 245 },
      { id: 202, title: 'Paper Lanterns', artist: 'Mira Ostrowski', album: 'Drift', seconds: 312 },
      { id: 203, title: 'Slow Orbit', artist: 'Field Notes', album: 'Weightless', seconds: 287 },
      { id: 204, title: 'Morning Fog', artist: 'Ólafur Brenna', album: 'Quiet Rooms', seconds: 198 },
      { id: 205, title: 'Rainfall Study No. 2', artist: 'Aiko Mori', album: 'Etudes for Weather', seconds: 264 },
      { id: 206, title: 'Soft Machinery', artist: 'Field Notes', album: 'Weightless', seconds: 331 },
      { id: 207, title: 'Low Light', artist: 'Mira Ostrowski', album: 'Drift', seconds: 223 },
      { id: 208, title: 'Tidal Memory', artist: 'Aiko Mori', album: 'Etudes for Weather', seconds: 276 },
    ],
  },
  {
    id: 'sunday',
    name: 'Sunday Morning',
    description: 'Warm acoustic songs for slow breakfasts and open windows.',
    owner: 'Maya Chen',
    likes: '613',
    cover: 'from-amber-300 via-orange-400 to-rose-500',
    wash: 'from-orange-800',
    tracks: [
      { id: 301, title: 'Honey & Toast', artist: 'The Linden Trees', album: 'Porch Songs', seconds: 186 },
      { id: 302, title: 'Open Window', artist: 'Clara Vale', album: 'Wildflower', seconds: 214 },
      { id: 303, title: 'Coffee for Two', artist: 'Ben Arlo', album: 'Kitchen Radio', seconds: 172 },
      { id: 304, title: 'Sunlit Room', artist: 'Clara Vale', album: 'Wildflower', seconds: 238 },
      { id: 305, title: 'Garden Path', artist: 'The Linden Trees', album: 'Porch Songs', seconds: 205 },
      { id: 306, title: 'Lazy River', artist: 'Ben Arlo', album: 'Kitchen Radio', seconds: 227 },
    ],
  },
  {
    id: 'run',
    name: 'Run Club 170 BPM',
    description: 'High-tempo tracks locked to your stride. No skips needed.',
    owner: 'Soundwave',
    likes: '12,077',
    cover: 'from-rose-500 via-red-500 to-orange-500',
    wash: 'from-red-900',
    tracks: [
      { id: 401, title: 'Redline', artist: 'Volt Theory', album: 'Pulse', seconds: 201 },
      { id: 402, title: 'Second Wind', artist: 'Nia Blaze', album: 'Stride', seconds: 189 },
      { id: 403, title: 'Pacesetter', artist: 'Volt Theory', album: 'Pulse', seconds: 214 },
      { id: 404, title: 'Uphill', artist: 'Kilo Echo', album: 'Tempo Run', seconds: 176 },
      { id: 405, title: 'Finish Line', artist: 'Nia Blaze', album: 'Stride', seconds: 233 },
    ],
  },
  {
    id: 'discover',
    name: 'Discover Weekly',
    description: 'Your weekly mixtape of fresh music, picked just for you.',
    owner: 'Soundwave',
    likes: '—',
    cover: 'from-sky-400 via-blue-500 to-violet-600',
    wash: 'from-blue-900',
    tracks: [
      { id: 501, title: 'Paper Planes Home', artist: 'Lumen Kid', album: 'Fold', seconds: 207 },
      { id: 502, title: 'Stillwater', artist: 'Harper Quinn', album: 'Currents', seconds: 243 },
      { id: 503, title: 'Kite String', artist: 'Lumen Kid', album: 'Fold', seconds: 192 },
      { id: 504, title: 'Blue Hour', artist: 'Otis Rowe', album: 'Dusk Sessions', seconds: 259 },
    ],
  },
];

const allTracks = new Map<number, { track: Track; playlist: Playlist }>();
for (const p of playlists) for (const t of p.tracks) allTracks.set(t.id, { track: t, playlist: p });

const formatTime = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;

function formatTotal(seconds: number) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return h > 0 ? `${h} hr ${m} min` : `${m} min ${seconds % 60} sec`;
}

// ---------------------------------------------------------------------------
// Pieces
// ---------------------------------------------------------------------------

function Cover({ playlist, size }: { playlist: Playlist; size: string }) {
  return (
    <div
      className={`flex shrink-0 items-center justify-center rounded-md bg-gradient-to-br ${playlist.cover} ${size}`}
    >
      <Music className="h-1/2 w-1/2 text-white/80" />
    </div>
  );
}

function Equalizer() {
  return (
    <span className="inline-flex h-3.5 items-end gap-0.5" aria-label="Now playing">
      <span className="h-2 w-[3px] rounded-sm bg-emerald-400" />
      <span className="h-3.5 w-[3px] rounded-sm bg-emerald-400" />
      <span className="h-1.5 w-[3px] rounded-sm bg-emerald-400" />
      <span className="h-3 w-[3px] rounded-sm bg-emerald-400" />
    </span>
  );
}

function Sidebar({ activeId, onSelect }: { activeId: string; onSelect: (id: string) => void }) {
  return (
    <aside className="flex w-72 shrink-0 flex-col gap-2">
      <nav className="rounded-lg bg-neutral-900 px-3 py-2">
        <a href="#" className="flex items-center gap-4 rounded-md px-3 py-2.5 text-sm font-semibold text-white">
          <House className="h-6 w-6" />
          Home
        </a>
        <a href="#" className="flex items-center gap-4 rounded-md px-3 py-2.5 text-sm font-semibold text-neutral-400">
          <Search className="h-6 w-6" />
          Search
        </a>
      </nav>
      <section className="flex min-h-0 flex-1 flex-col rounded-lg bg-neutral-900">
        <header className="flex items-center justify-between px-6 pb-2 pt-4">
          <h2 className="flex items-center gap-3 text-sm font-semibold text-neutral-300">
            <Library className="h-6 w-6" />
            Your Library
          </h2>
          <button
            type="button"
            aria-label="Create playlist"
            className="flex h-8 w-8 items-center justify-center rounded-full text-neutral-400"
          >
            <Plus className="h-5 w-5" />
          </button>
        </header>
        <div className="flex gap-2 px-4 py-2">
          <span className="rounded-full bg-white px-3 py-1 text-xs font-medium text-black">Playlists</span>
          <span className="rounded-full bg-neutral-800 px-3 py-1 text-xs font-medium text-white">Artists</span>
          <span className="rounded-full bg-neutral-800 px-3 py-1 text-xs font-medium text-white">Albums</span>
        </div>
        <ul className="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-2 pb-2">
          <li>
            <div className="flex items-center gap-3 rounded-md p-2">
              <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-md bg-gradient-to-br from-violet-700 to-sky-300">
                <Heart className="h-5 w-5 fill-white text-white" />
              </div>
              <div className="min-w-0">
                <p className="truncate text-sm font-medium text-white">Liked Songs</p>
                <p className="truncate text-xs text-neutral-400">Playlist · 214 songs</p>
              </div>
            </div>
          </li>
          {playlists.map((p) => {
            const active = p.id === activeId;
            return (
              <li key={p.id}>
                <button
                  type="button"
                  data-playlist={p.id}
                  onClick={() => onSelect(p.id)}
                  className={`flex w-full items-center gap-3 rounded-md p-2 text-left ${active ? 'bg-white/10' : ''}`}
                >
                  <Cover playlist={p} size="h-12 w-12" />
                  <div className="min-w-0">
                    <p className={`truncate text-sm font-medium ${active ? 'text-emerald-400' : 'text-white'}`}>
                      {p.name}
                    </p>
                    <p className="truncate text-xs text-neutral-400">Playlist · {p.owner}</p>
                  </div>
                </button>
              </li>
            );
          })}
        </ul>
      </section>
    </aside>
  );
}

function VolumeSlider({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const fraction = (e.clientX - rect.left) / rect.width;
    onChange(Math.max(0, Math.min(100, Math.round(fraction * 20) * 5)));
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'ArrowRight' || e.key === 'ArrowUp') onChange(Math.min(100, value + 10));
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') onChange(Math.max(0, value - 10));
    else return;
    e.preventDefault();
  };
  return (
    <div
      id="volume"
      role="slider"
      tabIndex={0}
      aria-label="Volume"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value}
      onClick={onClick}
      onKeyDown={onKeyDown}
      className="group flex h-4 w-28 cursor-pointer items-center outline-none"
    >
      <div className="relative h-1 w-full rounded-full bg-neutral-600">
        <div className="absolute inset-y-0 left-0 rounded-full bg-emerald-400" style={{ width: `${value}%` }} />
        <div
          className="absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow"
          style={{ left: `${value}%` }}
        />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

export default function App() {
  const [playlistId, setPlaylistId] = useState('late-night');
  const [currentId, setCurrentId] = useState(101);
  const [playing, setPlaying] = useState(false);
  // Playback position of the current track in seconds; nothing advances it, a fresh
  // track starts at 0:00 and the resumed session opens part-way through.
  const [position, setPosition] = useState(83);
  const [liked, setLiked] = useState<Set<number>>(() => new Set([102, 105, 203]));
  const [queueOpen, setQueueOpen] = useState(false);
  const [volume, setVolume] = useState(70);
  const [filter, setFilter] = useState('');

  const playlist = playlists.find((p) => p.id === playlistId)!;
  const current = allTracks.get(currentId)!;
  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return playlist.tracks;
    return playlist.tracks.filter((t) =>
      [t.title, t.artist, t.album].some((field) => field.toLowerCase().includes(q)),
    );
  }, [playlist, filter]);
  const totalSeconds = playlist.tracks.reduce((sum, t) => sum + t.seconds, 0);
  const playlistIsPlaying = playing && current.playlist.id === playlist.id;

  const play = (id: number) => {
    if (id === currentId) {
      setPlaying((p) => !p);
      return;
    }
    setCurrentId(id);
    setPosition(0);
    setPlaying(true);
  };

  const toggleLike = (id: number) =>
    setLiked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const selectPlaylist = (id: string) => {
    setPlaylistId(id);
    setFilter('');
  };

  const playAlbum = () => {
    if (current.playlist.id === playlist.id) setPlaying((p) => !p);
    else play(playlist.tracks[0].id);
  };

  const step = (delta: number) => {
    const list = current.playlist.tracks;
    const i = list.findIndex((t) => t.id === currentId);
    const next = list[(i + delta + list.length) % list.length];
    setCurrentId(next.id);
    setPosition(0);
    setPlaying(true);
  };

  const upNext = (() => {
    const list = current.playlist.tracks;
    const i = list.findIndex((t) => t.id === currentId);
    return list.slice(i + 1).concat(list.slice(0, i));
  })();

  const progress = (position / current.track.seconds) * 100;

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-black font-sans text-white antialiased">
      <div className="flex min-h-0 flex-1 gap-2 p-2 pb-0">
        <Sidebar activeId={playlistId} onSelect={selectPlaylist} />

        <main className="relative min-w-0 flex-1 overflow-y-auto rounded-lg bg-neutral-900">
          {/* Album header */}
          <div className={`bg-gradient-to-b ${playlist.wash} to-neutral-900 px-6 pb-6 pt-4`}>
            <div className="flex items-center justify-between">
              <div className="flex gap-2">
                <button type="button" aria-label="Back" className="flex h-8 w-8 items-center justify-center rounded-full bg-black/40">
                  <ChevronLeft className="h-5 w-5" />
                </button>
                <button type="button" aria-label="Forward" className="flex h-8 w-8 items-center justify-center rounded-full bg-black/40 text-neutral-400">
                  <ChevronRight className="h-5 w-5" />
                </button>
              </div>
              <div className="flex items-center gap-2 rounded-full bg-black/40 p-0.5 pr-3">
                <span className="flex h-7 w-7 items-center justify-center rounded-full bg-gradient-to-br from-pink-500 to-orange-400 text-xs font-bold">
                  MC
                </span>
                <span className="text-sm font-semibold">Maya Chen</span>
              </div>
            </div>
            <div className="mt-6 flex items-end gap-6">
              <div className={`flex h-48 w-48 shrink-0 items-center justify-center rounded-md bg-gradient-to-br ${playlist.cover} shadow-2xl shadow-black/50`}>
                <Music className="h-20 w-20 text-white/80" />
              </div>
              <div className="min-w-0 pb-1">
                <p className="text-xs font-semibold uppercase tracking-wider">Playlist</p>
                <h1 id="playlist-title" className="mt-2 truncate text-6xl font-bold tracking-tight">
                  {playlist.name}
                </h1>
                <p className="mt-4 max-w-xl text-sm text-white/70">{playlist.description}</p>
                <p className="mt-2 text-sm">
                  <span className="font-semibold">{playlist.owner}</span>
                  <span className="text-white/70">
                    {' '}
                    · {playlist.likes} saves · {playlist.tracks.length} songs, {formatTotal(totalSeconds)}
                  </span>
                </p>
              </div>
            </div>
          </div>

          {/* Actions */}
          <div className="flex items-center gap-6 px-6 py-4">
            <button
              id="play-album"
              type="button"
              aria-label={playlistIsPlaying ? 'Pause' : 'Play'}
              onClick={playAlbum}
              className="flex h-14 w-14 items-center justify-center rounded-full bg-emerald-400 text-black shadow-lg"
            >
              {playlistIsPlaying ? <Pause className="h-6 w-6" /> : <Play className="ml-0.5 h-6 w-6" />}
            </button>
            <Shuffle className="h-7 w-7 text-neutral-400" />
            <Heart className="h-7 w-7 text-neutral-400" />
            <Ellipsis className="h-7 w-7 text-neutral-400" />
            <label className="ml-auto flex w-56 items-center gap-2 rounded-md bg-white/10 px-3 py-1.5 text-sm text-neutral-400 focus-within:ring-2 focus-within:ring-white/30">
              <Search className="h-4 w-4 shrink-0" />
              <input
                id="filter-tracks"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && visible.length > 0) play(visible[0].id);
                }}
                placeholder="Search in playlist"
                className="w-full bg-transparent text-white placeholder-neutral-400 outline-none"
              />
            </label>
          </div>

          {/* Track table */}
          <div className="px-6 pb-8">
            <table className="w-full table-fixed text-left text-sm">
              <thead>
                <tr className="border-b border-white/10 text-xs uppercase tracking-wider text-neutral-400">
                  <th className="w-12 py-2 pl-4 font-normal">#</th>
                  <th className="py-2 font-normal">Title</th>
                  <th className="w-[22%] py-2 font-normal">Artist</th>
                  <th className="w-[22%] py-2 font-normal">Album</th>
                  <th className="w-12 py-2 font-normal">
                    <span className="sr-only">Liked</span>
                  </th>
                  <th className="w-20 py-2 pr-4 font-normal">
                    <Clock className="ml-auto h-4 w-4" />
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr aria-hidden="true">
                  <td colSpan={6} className="h-2" />
                </tr>
                {visible.map((t) => {
                  const index = playlist.tracks.indexOf(t) + 1;
                  const isCurrent = t.id === currentId;
                  const isLiked = liked.has(t.id);
                  return (
                    <tr
                      key={t.id}
                      data-track={t.id}
                      onClick={() => play(t.id)}
                      className={`cursor-pointer ${isCurrent ? 'bg-white/10' : ''}`}
                    >
                      <td className="rounded-l-md py-2 pl-4 tabular-nums text-neutral-400">
                        {isCurrent && playing ? (
                          <Equalizer />
                        ) : (
                          <span className={isCurrent ? 'text-emerald-400' : ''}>{index}</span>
                        )}
                      </td>
                      <td className="py-2 pr-4">
                        <div className="flex items-center gap-3">
                          <Cover playlist={playlist} size="h-10 w-10" />
                          <div className="min-w-0">
                            <p className={`truncate font-medium ${isCurrent ? 'text-emerald-400' : 'text-white'}`}>
                              {t.title}
                            </p>
                            <p className="truncate text-xs text-neutral-400">{t.artist}</p>
                          </div>
                        </div>
                      </td>
                      <td className="truncate py-2 pr-4 text-neutral-400">{t.artist}</td>
                      <td className="truncate py-2 pr-4 text-neutral-400">{t.album}</td>
                      <td className="py-2">
                        <button
                          type="button"
                          data-like={t.id}
                          aria-pressed={isLiked}
                          aria-label={isLiked ? `Remove ${t.title} from Liked Songs` : `Save ${t.title} to Liked Songs`}
                          onClick={(e) => {
                            e.stopPropagation();
                            toggleLike(t.id);
                          }}
                          className={`flex h-8 w-8 items-center justify-center ${isLiked ? 'text-emerald-400' : 'text-neutral-600'}`}
                        >
                          <Heart className={`h-4 w-4 ${isLiked ? 'fill-emerald-400' : ''}`} />
                        </button>
                      </td>
                      <td className="rounded-r-md py-2 pr-4 text-right tabular-nums text-neutral-400">
                        {formatTime(t.seconds)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            {filter && (
              <p className="mt-4 text-xs text-neutral-400">
                {visible.length} of {playlist.tracks.length} songs match “{filter}”
              </p>
            )}
          </div>
        </main>
      </div>

      {/* Now-playing bar */}
      <footer className="grid h-[88px] shrink-0 grid-cols-[1fr_minmax(0,2fr)_1fr] items-center gap-4 px-4">
        <div className="flex min-w-0 items-center gap-3">
          <Cover playlist={current.playlist} size="h-14 w-14" />
          <div className="min-w-0">
            <p id="now-playing-title" className="truncate text-sm font-medium text-white">
              {current.track.title}
            </p>
            <p className="truncate text-xs text-neutral-400">{current.track.artist}</p>
          </div>
          <button
            type="button"
            data-like={`bar-${current.track.id}`}
            aria-label="Save to Liked Songs"
            onClick={() => toggleLike(current.track.id)}
            className={`ml-2 shrink-0 ${liked.has(current.track.id) ? 'text-emerald-400' : 'text-neutral-400'}`}
          >
            <Heart className={`h-4 w-4 ${liked.has(current.track.id) ? 'fill-emerald-400' : ''}`} />
          </button>
        </div>

        <div className="flex flex-col items-center gap-2">
          <div className="flex items-center gap-6">
            <button type="button" aria-label="Shuffle" className="text-neutral-400">
              <Shuffle className="h-4 w-4" />
            </button>
            <button id="prev" type="button" aria-label="Previous" onClick={() => step(-1)} className="text-neutral-300">
              <SkipBack className="h-4 w-4" />
            </button>
            <button
              id="play-pause"
              type="button"
              aria-label={playing ? 'Pause' : 'Play'}
              onClick={() => setPlaying((p) => !p)}
              className="flex h-8 w-8 items-center justify-center rounded-full bg-white text-black"
            >
              {playing ? <Pause className="h-4 w-4" /> : <Play className="ml-0.5 h-4 w-4" />}
            </button>
            <button id="next" type="button" aria-label="Next" onClick={() => step(1)} className="text-neutral-300">
              <SkipForward className="h-4 w-4" />
            </button>
            <button type="button" aria-label="Repeat" className="text-emerald-400">
              <Repeat className="h-4 w-4" />
            </button>
          </div>
          <div className="flex w-full max-w-xl items-center gap-2 text-[11px] tabular-nums text-neutral-400">
            <span className="w-10 text-right">{formatTime(position)}</span>
            <div className="relative h-1 flex-1 rounded-full bg-neutral-600">
              <div className="absolute inset-y-0 left-0 rounded-full bg-white" style={{ width: `${progress}%` }} />
            </div>
            <span className="w-10">{formatTime(current.track.seconds)}</span>
          </div>
        </div>

        <div className="flex items-center justify-end gap-4 text-neutral-400">
          <Mic className="h-4 w-4" />
          <button
            id="queue-toggle"
            type="button"
            aria-label="Queue"
            aria-pressed={queueOpen}
            onClick={() => setQueueOpen((o) => !o)}
            className={`relative ${queueOpen ? 'text-emerald-400' : ''}`}
          >
            <ListMusic className="h-4 w-4" />
            {queueOpen && <span className="absolute -bottom-2 left-1/2 h-1 w-1 -translate-x-1/2 rounded-full bg-emerald-400" />}
          </button>
          <MonitorSpeaker className="h-4 w-4" />
          <div className="flex items-center gap-2">
            <VolumeIcon level={volume} className="h-4 w-4" />
            <VolumeSlider value={volume} onChange={setVolume} />
          </div>
        </div>
      </footer>

      {/* Queue drawer */}
      <aside
        id="queue"
        aria-hidden={!queueOpen}
        className={`fixed bottom-[88px] right-2 top-2 flex w-80 flex-col rounded-lg border border-white/10 bg-neutral-900 shadow-2xl shadow-black transition-transform duration-300 ease-out ${
          queueOpen ? 'translate-x-0' : 'translate-x-[110%]'
        }`}
      >
        <header className="flex items-center justify-between px-4 pb-2 pt-4">
          <h2 className="text-base font-bold">Queue</h2>
          <button
            id="queue-close"
            type="button"
            aria-label="Close queue"
            onClick={() => setQueueOpen(false)}
            className="flex h-8 w-8 items-center justify-center rounded-full text-neutral-400"
          >
            <X className="h-4 w-4" />
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
          <h3 className="px-2 pb-2 pt-2 text-sm font-bold">Now playing</h3>
          <div className="flex items-center gap-3 rounded-md bg-white/5 p-2">
            <Cover playlist={current.playlist} size="h-10 w-10" />
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium text-emerald-400">{current.track.title}</p>
              <p className="truncate text-xs text-neutral-400">{current.track.artist}</p>
            </div>
            {playing && <Equalizer />}
          </div>
          <h3 className="px-2 pb-2 pt-5 text-sm font-bold">
            Next from: <span className="text-neutral-400">{current.playlist.name}</span>
          </h3>
          <ul className="space-y-0.5">
            {upNext.map((t) => (
              <li key={t.id}>
                <button
                  type="button"
                  data-queue={t.id}
                  onClick={() => play(t.id)}
                  className="flex w-full items-center gap-3 rounded-md p-2 text-left"
                >
                  <Cover playlist={current.playlist} size="h-10 w-10" />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-white">{t.title}</p>
                    <p className="truncate text-xs text-neutral-400">{t.artist}</p>
                  </div>
                  <span className="text-xs tabular-nums text-neutral-500">{formatTime(t.seconds)}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      </aside>
    </div>
  );
}
