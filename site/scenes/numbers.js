export default {
  id: 'numbers', title: 'Totalling sales by region in Numbers on macOS',
  machines: [{ id: 'mac-numbers', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    const sheet = command => pc.step('application.v1', 'shell', { target: `window:0:content:sheet:${command}` });
    // Type a block of cells: rows go down from `at`, Tab moves along a row.
    const fill = (at, rows) => rows.forEach((row, i) => {
      sheet(`select:${at[0]}${+at.slice(1) + i}`);
      row.forEach(cell => { pc.type(cell); pc.key('Tab'); });
    });
    pc.launch('spreadsheet', '/Users/alice/Documents/Sales.csv');
    pc.click('window:0:maximize');
    sheet('colwidth:A:96'); sheet('colwidth:C:88'); sheet('colwidth:G:24');
    fill('F1', [['Revenue'], ['=D2*E2']]);
    sheet('select:F2:F49'); sheet('filldown'); sheet('fmt:currency');
    fill('H1', [
      ['Region', 'Revenue', 'Units'],
      ...['North', 'East', 'South', 'West'].map((region, i) =>
        [region, `=SUMIF($B$2:$B$49,H${i + 2},$F$2:$F$49)`, `=SUMIF($B$2:$B$49,H${i + 2},$D$2:$D$49)`]),
      ['Total', '=SUM(I2:I5)', '=SUM(J2:J5)'],
    ]);
    sheet('select:I2:I6'); sheet('fmt:currency');
    sheet('select:H1:J1'); sheet('bold');
    sheet('select:H6:J6'); sheet('bold');
    sheet('select:H1:I5'); sheet('chart:column');
    sheet('colwidth:F:96'); sheet('colwidth:I:104');
    sheet('select:A1');
    // Drag the chart from beside the data to under the summary.
    const drag = (op, x, y) => pc.step('pointer.v1', op, { x, y, width: 1280, height: 800 });
    drag('down', 1140, 400); drag('move', 1000, 450); drag('move', 770, 534); drag('up', 770, 534);
    sheet('select:I6');
  },
};
