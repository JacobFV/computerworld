// The machines in the slideshow, in the order they are shown.
//
// WRITING A SCENE (site/scenes/<id>.js, default-exporting one object)
//
//   export default {
//     id: 'swe-team',                       // the URL fragment: `#swe-team` opens on it
//     title: 'Shipping a fix together',     // the caption's name for it, and a tile's alt text
//     summary: 'atlas#14 → pull/15 on sort-refs: src/bfs.rs takes a BTreeMap',
//     machines: [
//       { id: 'swe-mac', like: 'alice-mac', size: [1280, 800], label: "Alice's Mac" },
//       { id: 'swe-phone', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
//     ],
//     open(mac, phone) { ... },             // one argument per machine, in this order
//     sync(others) { ... },                 // optional: catch the others up after a gesture
//   };
//
// `like` names the reference machine each is a copy of: alice-mac, bob-windows,
// carol-ubuntu, alice-phone or bob-android. `size` is that machine's screen; landscape
// makes a desktop tile, portrait a phone tile. Up to seven machines read well on one
// slide; past that they are thumbnails.
//
// `summary` is the line under the stage, beside the title: one dense technical sentence
// naming what the machines are actually doing — the project, the part designator, the
// file, the ticket, the branch — written from `open()` rather than from the title. Keep
// it to about a hundred characters so it stays on one line in a 1280-wide window; it
// wraps on a phone.
//
// HOW A SLIDE IS LAID OUT — EQUAL SHARE (site/app.js, `choose` and `fit`)
//
// No machine on a slide leads, none is anybody's thumbnail, and the picture they make
// together has no holes in it. A slide is one rectangle cut in two — side by side or one
// above the other, never reordered — each half cut in two again, and so on down to the
// machines, so every cut fills what it was given exactly. A phone never gets a row of its
// own with margin either side, because a "row" here is only ever the whole of its box.
//
// Which cut to take is decided on the machines that come off worst. Two machines of the
// same shape always come out the same size — every desktop on a slide is one tile and
// every phone is another — and among the cuts that hold to that, the one that makes the
// SMALLEST screen on the slide as LARGE as it can be wins. Raising the floor is what an
// equal share means once the shapes are mixed: a phone and a desktop cannot be given the
// same area and still tile a rectangle, so the fair thing is to make the worst-off machine
// as big as possible. Where the shapes are not mixed it is equal area exactly, since then
// every tile is the floor.
//
// What that comes to, for the slides in this cast, in a wide window:
//   four desktops and a phone  a 2 x 2 block, the phone beside it at the block's height
//   three desktops, four phones  a row of each, both rows the same width
//   two desktops, two phones   the desktops in a column, the phones beside them
//   a Mac and an iPhone        side by side, both floor to ceiling
//   three desktops             three equal tiles in a row
//   two phones                 side by side, as before
// Nothing is stretched: a phone stays 390x844 and a desktop 1280x800. It is all re-cut
// when the window changes, so a team that reads as one wide picture on a laptop reads as
// a block on a phone.
//
// So: list the machines in the order they should be read, left to right and top to bottom.
// That order is the only say a scene has in it; the sizes are not a scene's business.
//
// STILLS
//
// Each machine's still is site/media/scenes/<machine id>.jpg, rendered from the real
// thing by `node scripts/site/render-site-stills.mjs [scene-id ...]`. A machine whose still
// has not been rendered yet shows an empty screen rather than a broken image, so a new
// scene can land before its pictures do. The stills are all asked for within a second or
// two of the page opening — the slide in the middle and the ones a keypress away first,
// decoded before they are needed — and each tile keeps its still, undimmed, until its own
// machine is genuinely running behind it. Nothing in the slideshow waits for a boot: it
// turns on the frame the key is pressed.
//
// Each scene (site/scenes/) has computers of its own and an `open()`: the actions that
// take them from a fresh boot to something mid-work. Openings run in the visitor's tab,
// each scene in a world of its own.
import xEverywhere from './scenes/x-everywhere.js';
import artStudio from './scenes/art-studio.js';
import hardwareTeam from './scenes/hardware-team.js';
import sweTeam from './scenes/swe-team.js';
import officeTeam from './scenes/office-team.js';
import excel from './scenes/excel.js';
import numbers from './scenes/numbers.js';
import calc from './scenes/calc.js';
import docs from './scenes/docs.js';
import gdocs from './scenes/gdocs.js';
import notes from './scenes/notes.js';
import calendar from './scenes/calendar.js';
import bank from './scenes/bank.js';
import mail from './scenes/mail.js';
import slack from './scenes/slack.js';
import discord from './scenes/discord.js';
import linear from './scenes/linear.js';
import assistant from './scenes/assistant.js';
import github from './scenes/github.js';
import terminalGit from './scenes/terminal-git.js';
import codeDebug from './scenes/code-debug.js';
import codeJs from './scenes/code-js.js';
import sqlite from './scenes/sqlite.js';
import tableplus from './scenes/tableplus.js';
import freecadPart from './scenes/freecad-part.js';
import freecadSketch from './scenes/freecad-sketch.js';
import kicadSchematic from './scenes/kicad-schematic.js';
import kicadBoard from './scenes/kicad-board.js';
import kicadSim from './scenes/kicad-sim.js';
import gimp from './scenes/gimp.js';
import pixelmator from './scenes/pixelmator.js';
import paint from './scenes/paint.js';
import kdenlive from './scenes/kdenlive.js';
import imovie from './scenes/imovie.js';
import phonesVideo from './scenes/phones-video.js';
import phonesMusic from './scenes/phones-music.js';
import phonesPhotos from './scenes/phones-photos.js';
import phonesCalendarNotes from './scenes/phones-calendar-notes.js';
import phonesMapsWeather from './scenes/phones-maps-weather.js';
import phonesWork from './scenes/phones-work.js';
import texting from './scenes/texting.js';
import wikipedia from './scenes/wikipedia.js';
import phonesWeb from './scenes/phones-web.js';
import x from './scenes/x.js';

// The slideshow is a ring, so this reads outward from the machine it opens on rather
// than front to back: the teams first, then a walk round the work they do — the office,
// the people they talk to, the code, the data under it, the boards and the parts, the
// pictures, the films and music, the phones that carry all of it, and the web, which
// comes back round to one post on X and the machines it was read on.
export const cast = [
  xEverywhere, artStudio, hardwareTeam, sweTeam, officeTeam,
  excel, numbers, calc, docs, gdocs, notes, calendar, bank, mail,
  slack, discord, linear, assistant,
  github, terminalGit, codeDebug, codeJs,
  sqlite, tableplus,
  freecadPart, freecadSketch, kicadSchematic, kicadBoard, kicadSim,
  gimp, pixelmator, paint,
  kdenlive, imovie, phonesVideo, phonesMusic,
  phonesPhotos, phonesCalendarNotes, phonesMapsWeather, phonesWork, texting,
  wikipedia, phonesWeb, x,
];

// Where the slideshow opens when the address bar names no scene. Its neighbours are what
// a visitor sees first at the edges, so this is the middle of the five team scenes.
export const opening = 'hardware-team';
