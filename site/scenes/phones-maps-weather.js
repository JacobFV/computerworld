export default {
  id: 'phones-maps-weather', title: 'Maps on an iPhone, Weather on a Pixel',
  summary: 'A transit route from Northstar HQ to the DevCon Center, and Seattle’s forecast beside it',
  machines: [
    { id: 'iphone-maps', like: 'alice-phone', size: [390, 844], label: "Alice's iPhone" },
    { id: 'pixel-weather', like: 'bob-android', size: [412, 892], label: "Bob's Pixel" },
  ],
  open(iphone, pixel) {
    const press = (phone, control) => phone.step('application.v1', 'shell', { target: `window:0:content:${control}` });
    iphone.launch('maps');
    iphone.click('maps:search-field');
    iphone.type('Seattle'); // narrows the map to the city
    iphone.key('Enter');
    press(iphone, 'maps:from:northstar-hq');
    press(iphone, 'maps:to:devcon-center');
    press(iphone, 'maps:mode:transit');
    press(iphone, 'maps:route');
    press(iphone, 'maps:place:devcon-center');
    pixel.launch('weather');
    pixel.click('weather:city:seattle');
  },
};
