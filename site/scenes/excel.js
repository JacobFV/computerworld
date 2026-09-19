export default {
  id: 'excel', title: 'A budget in Excel on Windows 11',
  machines: [{ id: 'windows-excel', like: 'bob-windows', size: [1280, 800] }],
  open(pc) {
    const sheet = command => pc.step('application.v1', 'shell', { target: `window:0:content:sheet:${command}` });
    pc.launch('spreadsheet', 'C:/Users/bob/Documents/Budget.xlsx');
    pc.click('window:0:maximize');
    // A "share of total" column beside the table, with data bars.
    // Copying the header beside it brings its formatting along.
    sheet('select:F1'); sheet('copy'); sheet('select:G1'); sheet('paste');
    pc.type('Share'); pc.key('Enter');
    pc.type('=E2/$E$8'); pc.key('Enter');
    sheet('select:G2:G8'); sheet('filldown'); sheet('fmt:percent');
    sheet('select:G8'); sheet('bold');
    sheet('select:G2:G7'); sheet('cf:bar:63be7b');
    sheet('select:E2:E7'); pc.key('Escape');
  },
};
