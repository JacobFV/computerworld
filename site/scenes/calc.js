export default {
  id: 'calc', title: "Four-quarter forecasting in LibreOffice",
  summary: "A planner builds Atlas FY 2027 in LibreOffice Calc on Ubuntu: 1,240 customers growing 12 % a quarter, a margin row under them, and a line chart over the result.",
  machines: [{ id: 'ubuntu-calc', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    const sheet = command => pc.step('application.v1', 'shell', { target: `window:0:content:sheet:${command}` });
    const quarters = formula => ['B', 'C', 'D', 'E'].map(formula);
    const before = { C: 'B', D: 'C', E: 'D' };
    const rows = [
      ['Atlas forecast', 'Q1', 'Q2', 'Q3', 'Q4', 'FY 2027'],
      ['Revenue', ...quarters(q => `=${q}8*${q}9*3`), '=SUM(B2:E2)'],
      ['Costs', ...quarters(q => `=${q}10+${q}11`), '=SUM(B3:E3)'],
      ['Operating income', ...quarters(q => `=${q}2-${q}3`), '=SUM(B4:E4)'],
      ['Operating margin', ...quarters(q => `=${q}4/${q}2`), '=F4/F2'],
      [],
      ['Assumptions'],
      ['Customers', '1240', ...quarters(q => `=ROUND(${before[q]}8*1.12,0)`).slice(1)],
      ['Price per month', '49', '49', '54', '54'],
      ['Hosting and support', ...quarters(q => `=${q}8*31`)],
      ['Payroll', '118000', '118000', '131000', '131000'],
    ];
    pc.launch('spreadsheet');
    pc.click('window:0:maximize');
    sheet('new:calc');
    rows.forEach((row, i) => {
      sheet(`select:A${i + 1}`);
      row.forEach(cell => { pc.type(cell); pc.key('Tab'); });
    });
    sheet('colwidth:A:150');
    sheet('select:A1:F1'); sheet('bold');
    sheet('select:A7'); sheet('bold');
    sheet('select:A4:F4'); sheet('bold');
    sheet('select:B2:F4'); sheet('fmt:currency'); sheet('dec:less'); sheet('dec:less');
    sheet('select:B9:E11'); sheet('fmt:currency'); sheet('dec:less'); sheet('dec:less');
    sheet('select:B5:F5'); sheet('fmt:percent');
    sheet('select:A1:E4'); sheet('chart:line');
    // Drag the chart from beside the model to under it.
    const drag = (op, x, y) => pc.step('pointer.v1', op, { x, y, width: 1280, height: 800 });
    drag('down', 1000, 300); drag('move', 800, 400); drag('move', 520, 516); drag('up', 520, 516);
    sheet('rename'); for (const _ of 'Sheet1') pc.key('Backspace');
    pc.type('Forecast'); pc.key('Enter');
    sheet('select:F4');
  },
};
