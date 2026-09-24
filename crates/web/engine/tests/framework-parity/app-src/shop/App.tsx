import { useState } from 'react';
import { Check, ChevronDown, ChevronRight, Heart, Menu, Minus, Plus, RotateCcw, Search, ShieldCheck, ShoppingBag, Star, Truck, X } from '../shared/icons';
import { product, type Colour } from './data';

type CartLine = { id: string; name: string; colour: Colour; size: string; price: number; qty: number };

const money = (n: number) => `$${n.toFixed(2)}`;

function Stars({ rating }: { rating: number }) {
  return (
    <div className="flex items-center">
      {[0, 1, 2, 3, 4].map((i) => (
        <svg key={i} viewBox="0 0 20 20" className={`h-5 w-5 ${i < Math.round(rating) ? 'text-amber-400' : 'text-gray-200'}`} fill="currentColor" aria-hidden="true">
          <path d="M10.868 2.884c-.321-.772-1.415-.772-1.736 0l-1.83 4.401-4.753.381c-.833.067-1.171 1.107-.536 1.651l3.62 3.102-1.106 4.637c-.194.813.691 1.456 1.405 1.02L10 15.591l4.069 2.485c.713.436 1.598-.207 1.404-1.02l-1.106-4.637 3.62-3.102c.635-.544.297-1.584-.536-1.65l-4.752-.382-1.831-4.401z" />
        </svg>
      ))}
    </div>
  );
}

function ProductArt({ colour, className }: { colour: Colour; className?: string }) {
  // A flat illustration of the bag, tinted with the selected colour, standing in for a photo.
  return (
    <svg viewBox="0 0 200 200" className={className} aria-hidden="true">
      <defs>
        <radialGradient id={`glow-${colour.id}`} cx="50%" cy="40%" r="60%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity={0.9} />
          <stop offset="100%" stopColor="#ffffff" stopOpacity={0} />
        </radialGradient>
      </defs>
      <rect width="200" height="200" fill={colour.backdrop} />
      <circle cx="100" cy="90" r="80" fill={`url(#glow-${colour.id})`} />
      <path d="M70 70 C70 40 130 40 130 70" fill="none" stroke={colour.shade} strokeWidth="8" strokeLinecap="round" />
      <rect x="45" y="70" width="110" height="95" rx="14" fill={colour.hex} />
      <rect x="45" y="70" width="110" height="22" rx="10" fill={colour.shade} opacity={0.35} />
      <rect x="88" y="100" width="24" height="16" rx="4" fill="#ffffff" opacity={0.85} />
      <ellipse cx="100" cy="178" rx="62" ry="6" fill="#000000" opacity={0.08} />
    </svg>
  );
}

function Accordion({ title, children, defaultOpen = false }: { title: string; children: string; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="border-b border-gray-200">
      <button onClick={() => setOpen(!open)} data-accordion={title} className="flex w-full items-center justify-between py-4 text-left text-sm font-medium text-gray-900">
        {title}
        <ChevronDown className={`h-5 w-5 text-gray-400 transition-transform duration-200 ${open ? 'rotate-180' : ''}`} />
      </button>
      {open && <p className="pb-4 text-sm leading-6 text-gray-600">{children}</p>}
    </div>
  );
}

function CartDrawer({ lines, onClose, onQty }: { lines: CartLine[]; onClose: () => void; onQty: (id: string, delta: number) => void }) {
  const subtotal = lines.reduce((s, l) => s + l.price * l.qty, 0);
  const shipping = subtotal >= 150 ? 0 : 9;
  return (
    <div className="fixed inset-0 z-40" role="dialog" aria-modal="true">
      <div className="absolute inset-0 bg-black/30 transition-opacity" onClick={onClose} />
      <aside className="absolute inset-y-0 right-0 flex w-full max-w-md flex-col bg-white shadow-xl">
        <div className="flex items-center justify-between border-b border-gray-200 px-6 py-5">
          <h2 className="text-lg font-semibold text-gray-900">Your cart</h2>
          <button onClick={onClose} className="-m-2 rounded-md p-2 text-gray-400 hover:text-gray-500" aria-label="Close cart">
            <X className="h-6 w-6" />
          </button>
        </div>
        <ul className="flex-1 divide-y divide-gray-200 overflow-y-auto px-6">
          {lines.map((l) => (
            <li key={l.id} className="flex gap-4 py-6">
              <ProductArt colour={l.colour} className="h-24 w-24 shrink-0 rounded-md border border-gray-200" />
              <div className="flex flex-1 flex-col">
                <div className="flex justify-between text-sm font-medium text-gray-900">
                  <h3>{l.name}</h3>
                  <p className="ml-4 tabular-nums">{money(l.price * l.qty)}</p>
                </div>
                <p className="mt-1 text-sm text-gray-500">
                  {l.colour.name} · {l.size}
                </p>
                <div className="mt-auto flex items-center justify-between">
                  <div className="flex items-center rounded-md border border-gray-300">
                    <button data-qty="minus" onClick={() => onQty(l.id, -1)} className="p-1.5 text-gray-500 hover:text-gray-700" aria-label="Decrease">
                      <Minus className="h-4 w-4" />
                    </button>
                    <span className="w-8 text-center text-sm tabular-nums">{l.qty}</span>
                    <button data-qty="plus" onClick={() => onQty(l.id, 1)} className="p-1.5 text-gray-500 hover:text-gray-700" aria-label="Increase">
                      <Plus className="h-4 w-4" />
                    </button>
                  </div>
                  <button className="text-sm font-medium text-indigo-600 hover:text-indigo-500">Remove</button>
                </div>
              </div>
            </li>
          ))}
        </ul>
        <div className="border-t border-gray-200 px-6 py-6">
          <dl className="space-y-2 text-sm">
            <div className="flex justify-between text-gray-600">
              <dt>Subtotal</dt>
              <dd className="tabular-nums">{money(subtotal)}</dd>
            </div>
            <div className="flex justify-between text-gray-600">
              <dt>Shipping</dt>
              <dd className="tabular-nums">{shipping === 0 ? 'Free' : money(shipping)}</dd>
            </div>
            <div className="flex justify-between border-t border-gray-200 pt-2 text-base font-medium text-gray-900">
              <dt>Total</dt>
              <dd className="tabular-nums">{money(subtotal + shipping)}</dd>
            </div>
          </dl>
          {shipping > 0 && <p className="mt-2 text-xs text-gray-500">Add {money(150 - subtotal)} more for free shipping.</p>}
          <button className="mt-6 w-full rounded-lg bg-indigo-600 px-6 py-3 text-base font-medium text-white shadow-sm hover:bg-indigo-700">Checkout</button>
        </div>
      </aside>
    </div>
  );
}

export default function App() {
  const [colour, setColour] = useState(product.colours[0]);
  const [size, setSize] = useState<string | null>(null);
  const [sizeError, setSizeError] = useState(false);
  const [liked, setLiked] = useState(false);
  const [cart, setCart] = useState<CartLine[]>([]);
  const [cartOpen, setCartOpen] = useState(false);
  const count = cart.reduce((n, l) => n + l.qty, 0);

  function add() {
    if (!size) {
      setSizeError(true);
      return;
    }
    const id = `${colour.id}-${size}`;
    setCart((c) =>
      c.some((l) => l.id === id)
        ? c.map((l) => (l.id === id ? { ...l, qty: l.qty + 1 } : l))
        : [...c, { id, name: product.name, colour, size, price: product.price, qty: 1 }],
    );
    setCartOpen(true);
  }

  function changeQty(id: string, delta: number) {
    setCart((c) => c.map((l) => (l.id === id ? { ...l, qty: Math.max(1, l.qty + delta) } : l)));
  }

  return (
    <div className="bg-white font-sans text-gray-900">
      <div className="bg-gradient-to-r from-indigo-600 via-purple-600 to-pink-500 px-4 py-2 text-center text-sm font-medium text-white">
        Free shipping on orders over $150 — this week only
      </div>
      <header className="sticky top-0 z-30 border-b border-gray-200 bg-white/90 backdrop-blur">
        <div className="mx-auto flex h-16 max-w-7xl items-center gap-8 px-8">
          <Menu className="h-6 w-6 text-gray-400" />
          <span className="text-xl font-bold tracking-tight">
            trail<span className="text-indigo-600">&amp;</span>co
          </span>
          <nav className="flex gap-6 text-sm font-medium text-gray-700">
            {['Women', 'Men', 'Bags', 'Journal'].map((n) => (
              <a key={n} href="#" className={`hover:text-gray-900 ${n === 'Bags' ? 'text-indigo-600' : ''}`}>
                {n}
              </a>
            ))}
          </nav>
          <div className="ml-auto flex items-center gap-5 text-gray-400">
            <Search className="h-5 w-5" />
            <button id="cart" onClick={() => setCartOpen(true)} className="group relative flex items-center gap-1.5 text-gray-700" aria-label="Open cart">
              <ShoppingBag className="h-6 w-6 text-gray-400 group-hover:text-gray-500" />
              <span className="text-sm font-medium">{count}</span>
              {count > 0 && <span className="absolute -right-1 -top-1 h-2.5 w-2.5 rounded-full bg-pink-500 ring-2 ring-white" />}
            </button>
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-7xl px-8 pb-24 pt-6">
        <nav aria-label="Breadcrumb" className="flex items-center gap-1 text-sm text-gray-500">
          {product.breadcrumbs.map((b) => (
            <span key={b} className="flex items-center gap-1">
              <a href="#" className="hover:text-gray-700">
                {b}
              </a>
              <ChevronRight className="h-4 w-4 text-gray-300" />
            </span>
          ))}
          <span className="font-medium text-gray-900">{product.name}</span>
        </nav>

        <div className="mt-6 grid grid-cols-2 gap-12">
          <div>
            <div className="relative overflow-hidden rounded-2xl bg-gray-100">
              <ProductArt colour={colour} className="aspect-square w-full" />
              <span className="absolute left-4 top-4 rounded-full bg-white/90 px-3 py-1 text-xs font-semibold text-gray-900 shadow-sm">New season</span>
            </div>
            <div className="mt-4 grid grid-cols-4 gap-4">
              {product.colours.map((c) => (
                <button
                  key={c.id}
                  onClick={() => setColour(c)}
                  className={`overflow-hidden rounded-lg ring-2 ring-offset-2 ${c.id === colour.id ? 'ring-indigo-500' : 'ring-transparent'}`}
                  aria-label={`Show ${c.name}`}
                >
                  <ProductArt colour={c} className="aspect-square w-full" />
                </button>
              ))}
            </div>
          </div>

          <div>
            <h1 className="text-3xl font-bold tracking-tight text-gray-900">{product.name}</h1>
            <div className="mt-3 flex items-center gap-4">
              <p className="text-3xl tracking-tight text-gray-900">{money(product.price)}</p>
              <span className="rounded-md bg-rose-50 px-2 py-1 text-xs font-semibold text-rose-600">-20%</span>
              <p className="text-lg text-gray-400 line-through">{money(product.was)}</p>
            </div>
            <div className="mt-3 flex items-center gap-2">
              <Stars rating={product.rating} />
              <a href="#" className="text-sm font-medium text-indigo-600 hover:text-indigo-500">
                {product.reviews} reviews
              </a>
            </div>
            <p className="mt-6 text-base leading-7 text-gray-600">{product.description}</p>

            <div className="mt-8">
              <h2 className="text-sm font-medium text-gray-900">
                Colour <span className="font-normal text-gray-500">— {colour.name}</span>
              </h2>
              <div className="mt-3 flex gap-3">
                {product.colours.map((c) => (
                  <button
                    key={c.id}
                    data-colour={c.id}
                    onClick={() => setColour(c)}
                    aria-label={c.name}
                    className={`relative flex h-9 w-9 items-center justify-center rounded-full ring-offset-2 ${c.id === colour.id ? 'ring-2 ring-gray-900' : 'ring-1 ring-black/10'}`}
                    style={{ backgroundColor: c.hex }}
                  >
                    {c.id === colour.id && <Check className="h-4 w-4 text-white" />}
                  </button>
                ))}
              </div>
            </div>

            <div className="mt-8">
              <div className="flex items-center justify-between">
                <h2 className="text-sm font-medium text-gray-900">Size</h2>
                <a href="#" className="text-sm font-medium text-indigo-600 hover:text-indigo-500">
                  Size guide
                </a>
              </div>
              <div className="mt-3 grid grid-cols-4 gap-3">
                {product.sizes.map((s) => (
                  <button
                    key={s.label}
                    data-size={s.label}
                    disabled={!s.inStock}
                    onClick={() => {
                      setSize(s.label);
                      setSizeError(false);
                    }}
                    className={`rounded-md border px-4 py-3 text-sm font-medium uppercase ${
                      !s.inStock
                        ? 'cursor-not-allowed border-gray-200 bg-gray-50 text-gray-300 line-through'
                        : size === s.label
                          ? 'border-transparent bg-indigo-600 text-white'
                          : 'border-gray-200 bg-white text-gray-900 hover:bg-gray-50'
                    }`}
                  >
                    {s.label}
                  </button>
                ))}
              </div>
              {sizeError && <p className="mt-2 text-sm text-rose-600">Please choose a size.</p>}
            </div>

            <div className="mt-8 flex gap-3">
              <button id="add" onClick={add} className="flex flex-1 items-center justify-center gap-2 rounded-lg bg-indigo-600 px-8 py-3 text-base font-medium text-white shadow-sm hover:bg-indigo-700">
                <ShoppingBag className="h-5 w-5" />
                Add to bag
              </button>
              <button
                onClick={() => setLiked(!liked)}
                className={`rounded-lg border px-3 py-3 ${liked ? 'border-rose-200 bg-rose-50 text-rose-500' : 'border-gray-200 text-gray-400 hover:bg-gray-50'}`}
                aria-label="Save"
              >
                <Heart className="h-6 w-6" />
              </button>
            </div>

            <ul className="mt-8 grid grid-cols-3 gap-4 text-center text-xs text-gray-600">
              {[
                { icon: Truck, text: 'Free delivery over $150' },
                { icon: RotateCcw, text: '60-day returns' },
                { icon: ShieldCheck, text: '5-year warranty' },
              ].map(({ icon: Icon, text }) => (
                <li key={text} className="rounded-lg bg-gray-50 px-3 py-4">
                  <Icon className="mx-auto h-6 w-6 text-gray-400" />
                  <p className="mt-2">{text}</p>
                </li>
              ))}
            </ul>

            <div className="mt-8 border-t border-gray-200">
              <Accordion title="Features" defaultOpen>
                Water-resistant waxed canvas, full-grain leather base, padded 16-inch laptop sleeve, two hidden quick-access pockets.
              </Accordion>
              <Accordion title="Care">Brush off dry dirt, spot clean with a damp cloth and re-wax once a year.</Accordion>
              <Accordion title="Shipping">Ships in 1-2 business days from our Portland warehouse.</Accordion>
            </div>
          </div>
        </div>
      </main>

      {cartOpen && <CartDrawer lines={cart} onClose={() => setCartOpen(false)} onQty={changeQty} />}
    </div>
  );
}
