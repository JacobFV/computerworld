#!/usr/bin/env node
// Writes the RealWorld API's seed database (worlds/oss-web/services/realworld-api/
// service.json) in the format the JSON-file Prisma Client keeps
// (crates/services/oss-web/shims/prisma-store.js): the people, articles, tags,
// comments, follows and favourites the Conduit sites open with.
//
//     node scripts/oss-web/seed-realworld.mjs [path/to/bcryptjs]
//
// Passwords are real bcrypt hashes (cost 4, as the packaged API hashes them), with
// salts derived from the username so the file is the same on every run. bcryptjs
// comes from the API's own checkout (scripts/oss-web/build.sh caches it).
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '../..');
const cache = process.env.OSS_WEB_CACHE || join(homedir(), '.cache/computerworld/oss-web');
const bcryptPath = process.argv[2] || join(cache, 'realworld-api/node_modules/bcryptjs');
const bcrypt = createRequire(import.meta.url)(bcryptPath);
// The API's own slugs: `slugify(title)` with the package's defaults, then the author id.
const slugifyPackage = createRequire(import.meta.url)(join(dirname(bcryptPath), 'slugify'));

const B64 = './ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
function salt(username) {
  const h = createHash('sha256').update(`conduit:${username}`).digest();
  let s = '';
  for (let i = 0; i < 22; i++) s += B64[h[i] % 64];
  // The last character of a bcrypt salt carries only two bits.
  s = s.slice(0, 21) + '.Oeu'[h[21] % 4];
  return `$2a$04$${s}`;
}

const IMAGE = 'http://api.realworld.show/images/smiley-cyrus.jpeg';
const people = [
  { username: 'ada', email: 'ada@okonkwo.dev', password: 'conduit-ada-2026', bio: 'Platform engineer. Writes about builds, caches and the small tools that keep a team fast.' },
  { username: 'wren', email: 'wren@castillo.io', password: 'wren-writes', bio: 'Payments infrastructure. Retries are a decision to not find out what went wrong.' },
  { username: 'tomas.reyes', email: 'tomas@reyes.dev', password: 'tomas-reyes', bio: 'Frontend lead. Forms, focus rings and the web platform.' },
  { username: 'priya.natarajan', email: 'priya@natarajan.dev', password: 'priya-conduit', bio: 'Data engineer. Postgres, queues and the occasional incident review.' },
  { username: 'jules.moreau', email: 'jules@moreau.fr', password: 'jules-moreau', bio: 'Designer who ships. Type, spacing and design systems.' },
  { username: 'kenji.watanabe', email: 'kenji@watanabe.jp', password: 'kenji-watanabe', bio: 'SRE. On-call rotations that people do not dread.' },
  { username: 'fatima.haddad', email: 'fatima@haddad.dev', password: 'fatima-haddad', bio: 'Security engineer. Threat models and boring authentication.' },
];

// [author, title, description, tags, body paragraphs, day of September 2026 (or August when negative)]
const articles = [
  ['wren', 'I deleted our retry logic and the outages stopped', 'Every retry was a small lie about why the request failed the first time.', ['reliability', 'payments'],
    ['For three years our payment service retried every failed request up to five times. For three years we had an outage every six weeks that nobody could explain.',
     'The retries hid a race in our connection pool that failed one request in four hundred. Under load the retries saturated the pool and everything fell over at once.',
     'The fix to the pool was nine lines. Removing the retries was one. A retry is a decision to not find out what went wrong; make it on purpose.'], 3],
  ['ada', 'Our CI got four times faster when we stopped caching node_modules', 'The cache took longer to restore than the install it replaced.', ['ci', 'javascript', 'performance'],
    ['We cached node_modules between CI runs for two years because everyone does. Last month I timed it: restoring the 1.4 GB cache took 3 minutes 10 seconds; a clean install from the lockfile with a warm package store took 41 seconds.',
     'The store is content-addressed and small; node_modules is neither. Cache the store, not the tree it produces.',
     'We also split the test job by package, which is where the rest of the speedup came from.'], 9],
  ['tomas.reyes', 'Your form does not need a library', 'Constraint validation, FormData and one submit handler go a long way.', ['frontend', 'forms', 'javascript'],
    ['Most of the form libraries in our bundle re-implemented what the browser already does: required fields, patterns, lengths, and the messages that go with them.',
     'We moved three forms back to native validation, read values with FormData, and kept one small hook for server errors. The bundle lost 38 KB and the forms got more accessible, because the browser announces its own errors.'], 5],
  ['priya.natarajan', 'The queue that ate our Tuesdays', 'A post-incident review of a backlog that only grew on one day of the week.', ['postgres', 'incidents', 'reliability'],
    ['Every Tuesday our export queue backed up for six hours. The job that fed it ran on Tuesdays, which was the obvious suspect and the wrong one.',
     'The real cause was a vacuum on the jobs table that the weekly job made necessary: the queue scanned millions of dead tuples until autovacuum caught up.',
     'We partitioned the table by week and dropped old partitions instead of deleting rows. The Tuesday backlog is gone.'], 12],
  ['jules.moreau', 'Spacing is a system, not a set of numbers', 'Why our design tokens stopped at eight values and got better for it.', ['design', 'css'],
    ['We used to have twenty-three spacing tokens. Designers picked whichever looked right, engineers picked whichever was closest, and nothing lined up.',
     'We cut to eight, named them by role (inset, stack, inline) rather than size, and wrote down when each applies. Reviews got shorter because there was less to argue about.'], -28],
  ['kenji.watanabe', 'An on-call rotation people volunteer for', 'Pages that are actionable, handoffs that are written down, and a real day off after.', ['sre', 'on-call', 'culture'],
    ['We deleted 60 percent of our alerts. Every remaining page has a runbook link and an owner, and any page without one is a bug filed against the team that owns the alert.',
     'Handoffs are a written note, not a meeting. And the day after a bad night is a day off, taken, not offered.'], 15],
  ['fatima.haddad', 'Passkeys for the rest of us', 'What changed when we let people sign in without a password.', ['security', 'authentication'],
    ['We shipped passkeys as an option in March. By August, 41 percent of weekly sign-ins used one, and support tickets about locked accounts fell by half.',
     'The hard part was not the cryptography. It was account recovery, and making sure every recovery path was as strong as the passkey it replaced.'], 1],
  ['ada', 'Reproducible builds are a team sport', 'Pin everything, then find out what you forgot to pin.', ['ci', 'builds'],
    ['We pinned our compiler, our dependencies and our base image, and our builds were still not reproducible. The culprits were timestamps in archives, file ordering in a zip step, and a locale-dependent sort.',
     'The fix for each was small. Finding them took a diff of two builds, byte by byte, and patience.'], 14],
  ['wren', 'Idempotency keys, explained with a coffee order', 'Charge the card once, however many times the network makes you ask.', ['payments', 'api-design'],
    ['If you order a coffee and the barista does not hear you, you ask again. You do not want two coffees. An idempotency key is how the second ask says "the same order as before".',
     'The server stores the key with the result of the first request and returns that result to every retry. Keys expire after a day, which covers every client we have.'], -20],
  ['priya.natarajan', 'Stop writing ETL, start writing contracts', 'Schemas between teams catch breakage before the dashboard does.', ['data', 'api-design'],
    ['Most of our data incidents were a producer changing a column that a consumer depended on. We now publish a schema for every table another team reads, and CI fails when a change breaks it.'], 16],
];

const comments = [
  [1, 'ada', 'We found the same thing with our webhook sender. The retries were masking a DNS timeout.', 4],
  [1, 'kenji.watanabe', 'Logging the one retry you kept is the part people skip. Good call.', 5],
  [2, 'tomas.reyes', 'Which package manager? We saw the same with pnpm but not with npm.', 10],
  [2, 'ada', 'pnpm. The store is the thing to cache; with npm the equivalent is ~/.npm.', 10],
  [3, 'jules.moreau', 'The accessibility point is underrated. Native errors are announced for free.', 6],
  [4, 'kenji.watanabe', 'Partitioning by week is exactly what fixed ours too.', 13],
  [6, 'priya.natarajan', 'The day off after a bad night should be policy everywhere.', 15],
  [7, 'wren', 'Recovery is always the weakest link. Thanks for writing this up.', 2],
  [8, 'fatima.haddad', 'Locale-dependent sort bit us in a release script last year.', 14],
];

// [follower, followed]
const follows = [['ada', 'wren'], ['ada', 'priya.natarajan'], ['tomas.reyes', 'ada'], ['kenji.watanabe', 'wren'], ['wren', 'ada'], ['jules.moreau', 'tomas.reyes'], ['priya.natarajan', 'kenji.watanabe']];
// [username, article number]
const favorites = [['ada', 1], ['kenji.watanabe', 1], ['tomas.reyes', 2], ['wren', 2], ['priya.natarajan', 2], ['jules.moreau', 3], ['ada', 4], ['kenji.watanabe', 4], ['fatima.haddad', 6], ['ada', 7], ['wren', 7], ['priya.natarajan', 8]];

function slugify(title, id) {
  return `${slugifyPackage(title)}-${id}`;
}
function at(day, hour) {
  const d = day < 0 ? new Date(Date.UTC(2026, 7, 31 + day, hour)) : new Date(Date.UTC(2026, 8, day, hour));
  return d.toISOString();
}

const db = { User: [], Article: [], Comment: [], Tag: [], _links: {}, _seq: {} };
const userId = {};
for (const [i, p] of people.entries()) {
  const id = i + 1;
  userId[p.username] = id;
  db.User.push({ id, email: p.email, username: p.username, password: bcrypt.hashSync(p.password, salt(p.username)), image: IMAGE, bio: p.bio, demo: true });
}
const tagId = {};
const pairs = { ArticleToTag: [], UserFavorites: [], UserFollows: [] };
for (const [i, [author, title, description, tags, body, day]] of articles.entries()) {
  const id = i + 1;
  const created = at(day, 9 + (i % 8));
  db.Article.push({ id, slug: slugify(title, userId[author]), title, description, body: body.join('\n\n'), createdAt: created, updatedAt: created, authorId: userId[author] });
  for (const t of tags) {
    if (!tagId[t]) {
      tagId[t] = db.Tag.length + 1;
      db.Tag.push({ id: tagId[t], name: t });
    }
    pairs.ArticleToTag.push([id, tagId[t]]);
  }
}
for (const [i, [article, author, body, day]] of comments.entries()) {
  const created = at(day, 12 + (i % 6));
  db.Comment.push({ id: i + 1, createdAt: created, updatedAt: created, body, articleId: article, authorId: userId[author] });
}
// Article.favoritedBy (a) <-> User.favorites (b); User.followedBy (a) <-> User.following (b).
for (const [u, a] of favorites) pairs.UserFavorites.push([a, userId[u]]);
for (const [follower, followed] of follows) pairs.UserFollows.push([userId[followed], userId[follower]]);
// The implicit Article<->Tag relation is unnamed; the store keys it by its two fields.
db._links['Article.tagList|Tag.articles'] = pairs.ArticleToTag;
db._links.UserFavorites = pairs.UserFavorites;
db._links.UserFollows = pairs.UserFollows;
db._seq = { User: db.User.length, Article: db.Article.length, Comment: db.Comment.length, Tag: db.Tag.length };

const path = join(root, 'worlds/oss-web/services/realworld-api/service.json');
const service = JSON.parse(readFileSync(path, 'utf8'));
service.initial_state.files['/data/db.json'] = db;
writeFileSync(path, JSON.stringify(service, null, 2) + '\n');
console.log(`wrote ${path}: ${db.User.length} users, ${db.Article.length} articles, ${db.Comment.length} comments, ${db.Tag.length} tags`);
