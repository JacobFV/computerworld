export default {
  id: 'notes', title: "A release checklist in a plain text file",
  summary: "An engineer keeps the release-day order in ~/Notes/atlas-launch.txt on Ubuntu and adds step 5, stay in #incidents, the evening before.",
  machines: [{ id: 'ubuntu-notes', like: 'carol-ubuntu', size: [1280, 800] }],
  open(pc) {
    // Notes are text files in ~/Notes, so a few older ones are simply written there.
    const note = (name, lines) => pc.sh(`printf '%s\\n' ${lines.map(l => `'${l}'`).join(' ')} > ~/Notes/${name}.txt`);
    note('books', ['To read', '', 'The Soul of a New Machine', 'A Philosophy of Software Design', 'Piranesi']);
    note('groceries', ['Saturday shop', '', 'oat milk', 'coffee beans (the Ethiopian one)', 'lemons', 'rice noodles', 'something for Sunday lunch']);
    note('standup', ['Standup, Thursday', '', 'Yesterday: replay suite green on the candidate, fixed the flaky clock test.', 'Today: pricing page with Alice at 2, then the release code section of the launch doc.', 'Blocked: nothing, but the package mirror change needs a review from Bob.']);
    note('retro-ideas', ['For the launch retro', '', 'Freeze worked. Keep the two-sentence rule for fixes.', 'The install guide should be tested on a clean machine every release, not just this one.']);
    note('atlas-launch', [
      'Atlas launch: things only I know',
      '',
      'The release code lives in the launch doc and nowhere else. Read it out on the 9:30 call, Bob checks it against the tag, then and only then publish.',
      '',
      'Replay suite: run it against the release candidate, not main. Last time main had drifted by two commits and we chased a failure that was never going to ship.',
      '',
      'If the docs site flip fails, the old site stays up. Do not panic and do not roll back the packages for it.',
      '',
      'Order on the day',
      '1. tag',
      '2. publish packages',
      '3. flip the docs site',
      '4. announcement and customer note, together, 10:00',
    ]);
    pc.launch('notes');
    pc.click('notes:open:atlas-launch.txt');
    pc.click('notes:body');
    pc.type('5. stay in #incidents until the end of the day'); pc.key('Enter'); pc.key('Enter');
    pc.type('Ask Alice whether the pricing page');
  },
};
