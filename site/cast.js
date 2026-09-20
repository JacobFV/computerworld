// The machines in the slideshow, in the order they are shown.
//
// WRITING A SCENE (site/scenes/<id>.js, default-exporting one object)
//
//   export default {
//     id: 'swe-team',                       // the URL fragment: `#swe-team` opens on it
//     title: 'Shipping a fix together',     // the tab's label, and a tile's alt text
//     layout: 'auto',                       // optional, see below
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
// HOW A SLIDE IS LAID OUT (site/app.js, `plan` and `fit`)
//
// One machine leads and fills the slide's height; the rest are stacked beside it in rows
// that together come to the same height, so the group reads as one picture and every
// slide sits in the same box. The lead is the first machine listed, or the first desktop
// when the slide mixes desktops and phones — so list the machine the scene is about
// first. The number of rows is chosen from the tiles' shapes alone: as few as fit the
// width. Nothing is stretched; a phone stays 390x844 and a desktop 1280x800.
//
// `layout` overrides that when a scene knows better:
//   'auto'  (the default)  the lead, and the rest in as few rows as fit
//   'row'   every machine in one row, in the order listed, at one height
//   'stack' the lead, then the other desktops in one row and the phones in another
//   [2, 3]  explicit rows beside the lead: two machines, then three, in the order listed
//
// STILLS
//
// Each machine's still is site/media/scenes/<machine id>.jpg, rendered from the real
// thing by `node scripts/render-site-stills.mjs [scene-id ...]`. A machine whose still
// has not been rendered yet shows an empty screen rather than a broken image, so a new
// scene can land before its pictures do.
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
