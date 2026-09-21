// One change, seven machines. Atlas issue 14 is a BFS path test that only fails on the
// Windows runner; the fix is pull request 15 on the sort-refs branch, tracked as ATL-1.
// Every machine here is on that one change: the same branch, the same two files, the
// same pull request number.
export default {
  id: 'swe-team', title: "A bug report carried through to a merge",
  summary: "Seven machines move atlas#14 to pull/15: src/bfs.rs takes a BTreeMap on macOS, the diff is reviewed on Windows 11, ATL-1 advances in Linear on Ubuntu, and #eng, the issue and the CI mail are read on an iPhone and a Pixel.",
  machines: [
    { id: 'swe-mac-code', like: 'alice-mac', size: [1280, 800], label: "Alice's Mac · src/bfs.rs on sort-refs" },
    { id: 'swe-windows-pr', like: 'bob-windows', size: [1280, 800], label: "Bob's PC · the pull request 15 diff" },
    { id: 'swe-ubuntu-linear', like: 'carol-ubuntu', size: [1280, 800], label: "Carol's ThinkPad · ATL-1 in Linear" },
    { id: 'swe-iphone-slack', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone · #eng in Slack" },
    { id: 'swe-pixel-pr', like: 'bob-android', size: [412, 892], label: "Bob's Pixel · pull request 15" },
    { id: 'swe-iphone-issue', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone · issue 14 in Safari" },
    { id: 'swe-pixel-mail', like: 'bob-android', size: [412, 892], label: "Bob's Pixel · the Windows CI mail" },
  ],
  open(mac, pc, thinkpad, iphone, pixel, iphone2, pixel2) {
    // Alice mails Bob the green Windows run, then goes back to the test she is writing.
    mac.launch('mail');
    mac.click('mail:compose');
    mac.type('bob');
    mac.step('application.v1', 'shell', { target: 'window:0:content:mail:field:subject' });
    mac.type('atlas CI: sort-refs green on Windows, 3 of 3');
    mac.step('application.v1', 'shell', { target: 'window:0:content:mail:field:body' });
    mac.type('The Windows matrix ran the BFS path test three times on sort-refs and it passed three times. ' +
      'That is pull request 15 (ATL-1, github.com/northstar/atlas/pull/15): src/bfs.rs takes a BTreeMap and ' +
      'tests/path.rs pins the neighbour order. Merge it once the runners are patched.');
    mac.step('application.v1', 'shell', { target: 'window:0:content:mail:send' });
    mac.click('window:0:close');

    // The real repository, checked out on the branch the pull request is from.
    mac.sh('cd ~ && git clone http://github.com/repos/atlas atlas && cd atlas && git switch sort-refs');
    mac.launch('code', '/Users/alice/atlas');
    mac.click('window:1:maximize');
    mac.click('code:tree:tests');
    mac.click('code:tree:tests/path.rs');
    mac.key('Ctrl+End');
    // The second half of Priya's answer: assert the path is valid, not that it is one path.
    [
      '',
      '#[test]',
      'fn the_path_is_valid_whatever_the_order() {',
      '    let links = fixture();',
      '    let path = shortest(&links, "alice-mac", "app-server").unwrap();',
      '    assert!(path.windows(2).all(|hop| links[&hop[0]].contains(&hop[1])));',
      '}',
    ].forEach(line => { mac.type(line); mac.key('Enter'); });
    mac.click('code:tree:src');
    mac.click('code:tree:src/bfs.rs');

    // The same change as a diff: two files, src/bfs.rs first.
    pc.visit('http://github.com/northstar/atlas/pull/15/files');
    pc.click('window:0:maximize');
    pc.step('browser.v1', 'scroll', { y: 150 });

    // The ticket, with the branch it is from and Alice's approval on it.
    thinkpad.visit('http://linear.app/projects/ATL/issues/1');
    thinkpad.click('window:0:maximize');
    thinkpad.step('browser.v1', 'fill', { id: 'comment-body', value: 'Windows matrix is green three runs in a row. Merging once the runners are patched.' });
    thinkpad.step('browser.v1', 'submit', { id: 'comment' });

    // #eng, where the fix was posted, with a line from the phone.
    iphone.visit('http://northstar.slack.com/archives/eng');
    iphone.step('browser.v1', 'fill', { id: 'send-text', value: 'Approved pull/15. Merging after the runner patch.' });
    iphone.step('browser.v1', 'submit', { id: 'send' });
    // The transcript is its own scroll pane: wheel it down to the line just sent.
    for (let i = 0; i < 3; i++) iphone.step('pointer.v1', 'wheel', { x: 275, y: 400, width: 390, height: 844, delta_y: 1400 });

    // The pull request itself, in a phone browser.
    pixel.visit('http://github.com/northstar/atlas/pull/15');
    pixel.step('browser.v1', 'scroll', { y: 190 });

    // The bug this change closes.
    iphone2.visit('http://github.com/northstar/atlas/issues/14');
    iphone2.step('browser.v1', 'scroll', { y: 175 });

    // Bob reads the build mail Alice sent. (The phone keyboard covers the composer, so
    // the reply is left for his desk rather than half typed behind it.)
    pixel2.launch('mail');
    pixel2.click('mail:open:mail-2');
  },
};
