export default {
  id: 'docs', title: "Drafting a launch plan for review",
  summary: "A product lead writes the Atlas 1.0 launch plan in Pages on macOS, dated Tuesday the 22nd, typed through to the line asking a colleague to proofread it.",
  machines: [{ id: 'mac-docs', like: 'alice-mac', size: [1280, 800] }],
  open(pc) {
    pc.launch('docs');
    pc.click('window:0:maximize');
    pc.click('docs:open:launch');
    pc.click('docs:body');
    [
      'Plan for launch week',
      '',
      'Atlas 1.0 goes out on Tuesday the 22nd. We are not adding anything between now and then: the branch is frozen except for fixes that Bob or I have reproduced from a replay, and every one of those needs a second pair of eyes before it merges. If a fix cannot be explained in two sentences it waits for 1.0.1.',
      '',
      'Monday is the dress rehearsal. Bob cuts the release candidate in the morning, Carol runs the full replay suite against it, and I walk through the install guide on a clean machine exactly as a new customer would. Anything that makes me stop and think goes into OPS-1 as a blocker, however small it looks. We would rather slip a day than ship a first run that needs a support ticket.',
      '',
      'On the day itself the order is: tag, publish the packages, flip the documentation site, then the announcement. Carol reads the release code out of this document on the 9:30 call and Bob confirms it against the tag before anything is published. The post on the engineering blog and the note to the customer list go out together at 10:00, and the three of us stay in #incidents until the end of the day.',
      '',
      'What I still need: a final word from Carol on the pricing page, a rollback rehearsal on Friday (Bob, thirty minutes, it is in the calendar), and someone other than me to proofread the announcement, because I have read it so many times that I can no longer see',
    ].forEach(line => { pc.key('Enter'); pc.type(line); });
  },
};
