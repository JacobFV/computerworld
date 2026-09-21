// Homepage diagram frames: one checkpoint, three trajectories forked from it.
//
// The figure on the home page is meant to show what a bounded search actually does —
// fork a checkpoint, drive each branch a step or two, and end with a verdict — so the
// frames are a real fork per branch rather than one world driven and rewound. Two of the
// three paths are the checker's own counterexamples, copied from its report so the
// picture cannot drift from the run: P12 (the agent closes a pull request somebody else
// opened) and P15 (the agent stars the repository). The third is a read-only pass over
// the same pull request, which breaks nothing. The report's shortest counterexamples are
// the ones taken, so every column is one or two frames tall and the figure stays short.
//
//   node scripts/render-branch-frames.mjs
//
// Writes site/media/branches/{checkpoint,a-N,b-N,c-N}.png and prints the step labels the
// figure's captions must match.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createRequire} from 'node:module';
import {encodePNG} from './dom-to-site/png.mjs';
const require = createRequire(import.meta.url);
const {World} = require('../pkg/node/computerworld.js');

const report = JSON.parse(readFileSync(new URL('../site/data/policies.json', import.meta.url)));
const out = new URL('../site/media/branches/', import.meta.url);
mkdirSync(out, {recursive: true});

// The checker records a counterexample as controls with roles; a click is a click and a
// textbox is a fill. Reading the paths out of the report keeps the two branches that are
// counterexamples honest: if the run changes, so does the picture.
const counterexample = (id) => report.policies.find((p) => p.id === id).counterexample.actions.map((a) => ({
  id: a.control,
  role: a.role,
  value: a.role === 'textbox' || a.role === 'textarea'
    ? JSON.parse(/^type (".*") into /.exec(a.description)[1]) : null,
  label: a.label,
  says: a.description,
}));
const clicks = (...steps) => steps.map(([id, label, says]) => ({id, role: 'link', value: null, label, says}));

const TRAJECTORIES = [
  {key: 'a', verdict: 'holds', policy: null, steps: clicks(
    ['thread-15', 'Sort refs before iterating in BFS', 'click link "Sort refs before iterating in BFS"'],
    ['ptab-files-link', 'Files changed 2', 'click link "Files changed 2"'],
  )},
  {key: 'b', verdict: 'breaks', policy: 'P12', steps: counterexample('P12')},
  {key: 'c', verdict: 'breaks', policy: 'P15', steps: counterexample('P15')},
];

const world = new World(JSON.parse(readFileSync(new URL('../worlds/company-2026/world.json', import.meta.url))), 7n);
const grant = {actor: 'alice', machines: ['alice-mac'], actions: ['application.v1', 'pointer.v1', 'browser.v1', 'keyboard.v1'], observations: ['semantic.v1']};
const drive = (env) => (action) => {
  const result = env.step([{...action, machine: 'alice-mac'}]).outcomes[0];
  if (!result.success) throw new Error(JSON.stringify(result.error));
};
const shoot = (env, name) => {
  const frame = env.render(1440, 900);
  writeFileSync(new URL(`${name}.png`, out), encodePNG(frame));
  frame.free();
};

// The checkpoint every branch starts from: the repository's open pull requests.
const env = world.environment(grant);
const start = drive(env);
start({family: 'application.v1', op: 'launch', payload: {kind: 'browser', argument: 'http://github.com/northstar/atlas/pulls'}});
start({family: 'application.v1', op: 'maximize', payload: {window: 0}});
shoot(env, 'checkpoint');
const checkpoint = world.snapshot();

for (const {key, steps, verdict, policy} of TRAJECTORIES) {
  // A real fork, not a rewind: each branch is its own world from the same checkpoint.
  const branch = world.fork(checkpoint);
  const session = branch.session(env.id);
  const step = drive(session);
  steps.forEach((s, i) => {
    step(s.value === null
      ? {family: 'browser.v1', op: 'click', payload: {id: s.id}}
      : {family: 'browser.v1', op: 'fill', payload: {id: s.id, value: s.value}});
    shoot(session, `${key}-${i + 1}`);
  });
  const says = verdict === 'holds' ? 'no policy broken' : `${policy} broken`;
  console.log(`${key}: ${steps.length} steps, ${says}`);
  for (const [i, s] of steps.entries()) console.log(`   ${i + 1}. ${s.says}`);
}

// The policy-checks study shows one frame of its own: P2's counterexample, a merge while
// a reviewer's request for changes still stands. It is rendered here so that frame comes
// from the same run as the rest and is regenerated with them.
{
  const branch = world.fork(checkpoint);
  const session = branch.session(env.id);
  const step = drive(session);
  const steps = counterexample('P2');
  for (const s of steps) step({family: 'browser.v1', op: 'click', payload: {id: s.id}});
  shoot(session, 'merged');
  console.log(`merged.png: ${steps.length} steps, P2 broken`);
  for (const [i, s] of steps.entries()) console.log(`   ${i + 1}. ${s.says}`);
}

console.log(`\nRendered the checkpoint, ${TRAJECTORIES.reduce((n, t) => n + t.steps.length, 0)} step frames and the study's frame.`);
