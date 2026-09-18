// fetch's classes without a network: Headers, Request, Response, Blob, FormData.
const h = new Headers({ 'Content-Type': 'text/plain', 'X-B': '2' });
h.append('x-a', '1');
h.append('X-A', '3');
h.append('Set-Cookie', 'a=1');
h.append('Set-Cookie', 'b=2');
console.log([...h], h.get('x-a'), h.has('X-B'), h.get('nope'), h.getSetCookie());
h.delete('x-b');
h.set('x-a', ' spaced ');
console.log([...h.keys()], [...h.values()]);
h.forEach((v, k) => console.log('each', k, v));
try { new Headers({ 'bad header': 'x' }); } catch (e) { console.log(e.name, e.message); }

const req = new Request('http://example.test/path?q=1', { method: 'post', body: 'hi', headers: { a: 'b' } });
console.log(req.method, req.url, req.headers.get('a'), req.headers.get('content-type'), req.redirect, req.bodyUsed);
req.text().then((t) => console.log('request body', t, req.bodyUsed));
try { new Request('http://x/', { method: 'GET', body: 'no' }); } catch (e) { console.log(e.name, e.message); }
try { new Request('not a url'); } catch (e) { console.log(e.name, e.message); }

const res = new Response('{"a":[1,2]}', { status: 201, statusText: 'Created', headers: { 'content-type': 'application/json' } });
console.log(res.status, res.ok, res.statusText, res.headers.get('content-type'), res.type, res.url, res.redirected);
res.clone().json().then((j) => console.log('json', j));
res.text().then(async (t) => {
  console.log('text', t, res.bodyUsed);
  try { await res.text(); } catch (e) { console.log(e.name, e.message); }
});
const j = Response.json({ x: 1 }, { status: 200 });
console.log(j.headers.get('content-type'));
j.arrayBuffer().then((b) => console.log('bytes', b.byteLength));
const r0 = new Response(null, { status: 204 });
console.log(r0.body, r0.ok);
try { new Response('x', { status: 99 }); } catch (e) { console.log(e.name, e.message); }
console.log(Response.error().type, Response.error().status, Response.redirect('http://a.test/b', 301).headers.get('location'));

const b = new Blob(['ab', Buffer.from('cd')], { type: 'Text/Plain' });
console.log(b.size, b.type);
b.text().then((t) => console.log('blob', t));
const fd = new FormData();
fd.append('k', 'v1');
fd.append('k', 'v2');
fd.set('z', 'last');
console.log(fd.getAll('k'), fd.get('z'), fd.has('nope'), [...fd.keys()]);
new Response(new URLSearchParams({ a: '1', b: 'x y' })).text().then((t) => console.log('form', t));
