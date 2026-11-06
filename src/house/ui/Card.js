// The house surface (owner order, 4 Nov 2026: "1 px hairline borders do all separation", "shadows only on floating
// layers", "radius base 10px").
//
// A card is a panel colour, one hairline and a 10 px radius. It never carries a shadow: the hairline is the
// separation, and a shadow would mean the card floats, which a plate does not. Colour on a card is data only -
// the surface itself is one of the zero-chroma neutrals.
import { createElement as h } from 'react';
import { cn } from './cn.js';

export const CARD = 'bg-panel border border-line rounded-base contrast-more:border-2';

/**
 * @param {Object} p
 * @param {'panel'|'base'|'raised'} [p.tone]  panel (the card), base (the page itself), raised (one step up)
 * @param {string} [p.as]      the element to render (an article keeps its own semantics)
 * @param {string} [p.className]
 */
export function Card({ tone = 'panel', as = 'div', className, children, ...rest }) {
  const surface = tone === 'base' ? 'bg-base border border-line' : tone === 'raised' ? 'bg-slab border border-line' : CARD;
  return h(as, { className: cn(surface, className), ...rest }, children);
}
