// Tailwind for the app-* fixtures, run by build.sh once per app with that app's sources
// as `--content`, so each sheet holds exactly the utilities its app uses, as a
// production build would. Inter is the sans face, loaded by input.css's @font-face
// from the same file the engine bundles (tests/vendor/inter-*.ttf).
/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [],
  theme: {
    extend: {
      fontFamily: {
        // Tailwind's default sans stack with Inter in front.
        sans: ['Inter', 'ui-sans-serif', 'system-ui', 'sans-serif', '"Apple Color Emoji"', '"Segoe UI Emoji"', '"Segoe UI Symbol"', '"Noto Color Emoji"'],
      },
    },
  },
  plugins: [],
};
