for (const s of ['{bad', '', '[1,]', '{"a":1}x', '{"a" 1}', '[1 2]', '{"a":1 "b"}', '"abc', 'undefined', "'a'", '"\\x"', '"a\tb"', '-', '1e', '1.', '{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa": tru}', 'nul', '[1,\n2,\n]', '{"a":[1,2,{"b":3}],"cccccccccccccccccccccccc":"ddddddddddddddd" x}', '01', '{,}', '[', '{"a":']) { try { JSON.parse(s) } catch (e) { console.log(JSON.stringify(s), '=>', e.message) } }
console.log(JSON.stringify({a:[1,{b:2}],c:"x\ny",d:undefined,e:()=>1,f:NaN,g:new Date(0),h:[undefined,function(){}]}, null, 2))
console.log(JSON.stringify(" "), JSON.stringify({a:1n===1n}), JSON.stringify([new Map([[1,2]]), new Set([1])]))
try { const a = {}; a.self = a; JSON.stringify(a) } catch (e) { console.log(e.message) }
try { const a = {x: {y: []}}; a.x.y.push(a); JSON.stringify(a) } catch (e) { console.log(e.message) }
try { class Foo { constructor() { this.me = this } }; JSON.stringify({f: new Foo()}) } catch (e) { console.log(e.message) }
try { JSON.stringify({a: 1n}) } catch (e) { console.log(e.message) }
console.log(JSON.stringify({a:1,b:[1,2]}, ['a']), JSON.stringify({a:1,b:2}, (k,v)=> k==='a'? undefined : v), JSON.stringify([1,[2,[3]]], null, '--'))
console.log(JSON.parse('{"a":[1,2,{"b":null}],"c":true}', (k, v) => typeof v === 'number' ? v * 10 : v))
