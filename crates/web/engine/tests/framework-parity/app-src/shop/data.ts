export type Colour = { id: string; name: string; hex: string; shade: string; backdrop: string };

export const product = {
  name: 'Ridgeline Weekender',
  price: 148,
  was: 185,
  rating: 4.4,
  reviews: 1284,
  breadcrumbs: ['Home', 'Bags', 'Travel'],
  description:
    'A roomy carry-on in waxed canvas and leather that fits a long weekend and still slides under the seat in front of you. Structured base, soft sides, and a luggage sleeve that clips over a trolley handle.',
  colours: [
    { id: 'ochre', name: 'Ochre', hex: '#c98a2b', shade: '#7a4f12', backdrop: '#f7efe2' },
    { id: 'sage', name: 'Sage', hex: '#7f9c7a', shade: '#3f5a3b', backdrop: '#eef3ec' },
    { id: 'navy', name: 'Navy', hex: '#2f3e66', shade: '#151d36', backdrop: '#e8ebf3' },
    { id: 'clay', name: 'Clay', hex: '#b5654a', shade: '#6b2f1d', backdrop: '#f6e9e4' },
  ] as Colour[],
  sizes: [
    { label: 'S', inStock: true },
    { label: 'M', inStock: true },
    { label: 'L', inStock: true },
    { label: 'XL', inStock: false },
  ],
};
