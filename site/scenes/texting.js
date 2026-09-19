// Messages fetches when its user acts, so the phone that did not just act re-opens the
// conversation: a text sent from one lands on the other.
const refresh = phone => phone.click('chat:back') && phone.click('chat:channel:general');
const say = (phone, text) => { refresh(phone); phone.click('chat:compose'); phone.type(text); phone.key('Enter'); };

export default {
  id: 'texting', title: 'Two phones in one conversation: type on either',
  machines: [
    { id: 'iphone-texting', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
    { id: 'pixel-texting', like: 'bob-android', size: [412, 892], label: "Bob's Pixel" },
  ],
  open(alice, bob) {
    for (const phone of [alice, bob]) { phone.launch('chat'); phone.click('chat:channel:general'); }
    say(alice, 'Are you still at the office? The Q3 deck needs one more pass');
    say(bob, 'Just left. I can look from the train, send it over');
    say(alice, 'Shared in Documents. Slide 7 is the one I am unsure about');
    say(bob, 'The churn chart? I would cut it and lead with the forecast');
    say(alice, 'Agreed. Cutting it now');
    refresh(bob); refresh(alice);   // both caught up, and the keyboard put away
  },
  sync: others => others.forEach(refresh),
};
