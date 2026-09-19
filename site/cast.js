// The machines in the slideshow, in the order they are shown. Each scene (site/scenes/) has
// computers of its own (`like` names the reference machine each is a copy of) and an
// `open()`: the actions that take them from a fresh boot to something mid-work. Openings
// run in the visitor's tab, each scene in a world of its own.
import codeDebug from './scenes/code-debug.js';
import texting from './scenes/texting.js';
import freecadPart from './scenes/freecad-part.js';
import slack from './scenes/slack.js';
import kicadBoard from './scenes/kicad-board.js';
import excel from './scenes/excel.js';
import phonesMusic from './scenes/phones-music.js';
import gimp from './scenes/gimp.js';
import github from './scenes/github.js';
import kdenlive from './scenes/kdenlive.js';
import sqlite from './scenes/sqlite.js';
import phonesMapsWeather from './scenes/phones-maps-weather.js';
import kicadSim from './scenes/kicad-sim.js';
import docs from './scenes/docs.js';
import discord from './scenes/discord.js';
import codeJs from './scenes/code-js.js';
import phonesPhotos from './scenes/phones-photos.js';
import numbers from './scenes/numbers.js';
import linear from './scenes/linear.js';
import pixelmator from './scenes/pixelmator.js';
import terminalGit from './scenes/terminal-git.js';
import phonesWork from './scenes/phones-work.js';
import freecadSketch from './scenes/freecad-sketch.js';
import assistant from './scenes/assistant.js';
import calendar from './scenes/calendar.js';
import imovie from './scenes/imovie.js';
import phonesWeb from './scenes/phones-web.js';
import kicadSchematic from './scenes/kicad-schematic.js';
import calc from './scenes/calc.js';
import x from './scenes/x.js';
import paint from './scenes/paint.js';
import phonesCalendarNotes from './scenes/phones-calendar-notes.js';
import tableplus from './scenes/tableplus.js';
import wikipedia from './scenes/wikipedia.js';
import mail from './scenes/mail.js';
import phonesVideo from './scenes/phones-video.js';
import notes from './scenes/notes.js';
import gdocs from './scenes/gdocs.js';
import bank from './scenes/bank.js';

export const cast = [
  codeDebug, texting, freecadPart, slack, kicadBoard, excel, phonesMusic, gimp, github,
  kdenlive, sqlite, phonesMapsWeather, kicadSim, docs, discord, codeJs, phonesPhotos,
  numbers, linear, pixelmator, terminalGit, phonesWork, freecadSketch, assistant,
  calendar, imovie, phonesWeb, kicadSchematic, calc, x, paint, phonesCalendarNotes,
  tableplus, wikipedia, mail, phonesVideo, notes, gdocs, bank,
];
