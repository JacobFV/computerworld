// The measured interactions, shared by driver.mjs (Node) and
// examples/dom_bench.rs (cw-jsvm): a click on a task's check button that
// re-renders the list, and one keystroke into the controlled "new task" input.
(function (g) {
  'use strict';
  g.benchClick = function (id) {
    const el = document.getElementById(id || 'check-2');
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, detail: 1, view: g }));
  };
  g.benchKey = function () {
    const input = document.getElementById('new-task');
    input.focus();
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    setter.call(input, input.value + 'x');
    input.dispatchEvent(new InputEvent('input', { bubbles: true, data: 'x', inputType: 'insertText' }));
  };
  // What the phases should have produced: each task row's class, and the input.
  g.benchSummary = function () {
    const rows = [];
    for (let i = 1; i <= 6; i++) {
      const r = document.getElementById('task-' + i);
      rows.push(r ? r.className : '-');
    }
    const input = document.getElementById('new-task');
    return rows.join('|') + ' / ' + (input ? input.value : '-');
  };
})(globalThis);
