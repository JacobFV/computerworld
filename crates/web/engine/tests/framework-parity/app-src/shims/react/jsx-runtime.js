// `react/jsx-runtime` over the UMD build, which ships only createElement: the
// automatic runtime's jsx(type, props, key) is createElement with the key folded back
// into the props (children already travel in props.children).
const React = window.React;
function jsx(type, props, key) {
  return React.createElement(type, key === undefined ? props : { ...props, key });
}
exports.jsx = jsx;
exports.jsxs = jsx;
exports.Fragment = React.Fragment;
