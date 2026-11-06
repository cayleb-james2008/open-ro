// The house button (owner order, 4 Nov 2026: "primary button black-on-light/white-on-dark").
//
// One control, three variants, written here in the shadcn shape: a cva variant map, a `cn` joiner, our own code.
// The house rules it obeys:
//   - the primary variant is the inverse of the surface it sits on: on this dark surface it is a white plate with
//     the base colour as its text; on a light surface it would be the base colour with the light text. The token
//     names carry both, so a surface only picks the variant that fits;
//   - a 10 px radius (the house base radius), a 1 px hairline, no shadow: nothing here floats;
//   - no hover reaction and no idle motion (SPEC 17): the state a button carries is in its own text and in
//     `aria-pressed`, never in a colour that only appears under the pointer;
//   - the focus ring is the 2 px token ring with a 2 px offset (SPEC 15), never a blur.
import { createElement as h } from 'react';
import { cva } from 'class-variance-authority';
import { cn } from './cn.js';

export const button = cva('inline-flex items-center justify-center font-label leading-none whitespace-nowrap', {
  variants: {
    variant: {
      // white on the dark surface (the inverse on a light surface would be the base colour on the light text)
      primary: 'bg-text-1 text-base border border-text-1',
      // a hairline outline, for a control that sits next to the primary one
      secondary: 'bg-panel text-text-1 border border-line',
      // no plate, no hairline: a text control that still has a 44 px target
      ghost: 'bg-transparent text-text-2 border border-transparent',
    },
    size: {
      sm: 'h-chip px-2 text-small',
      md: 'min-h-target min-w-target px-3 text-small',
    },
    radius: {
      base: 'rounded-base',
      control: 'rounded-control',
    },
  },
  defaultVariants: { variant: 'secondary', size: 'md', radius: 'control' },
});

/**
 * @param {Object} p
 * @param {'primary'|'secondary'|'ghost'} [p.variant]
 * @param {'sm'|'md'} [p.size]
 * @param {'base'|'control'} [p.radius]
 * @param {string} [p.className]
 */
export function Button({ variant = 'secondary', size = 'md', radius = 'control', className, children, ...rest }) {
  return h('button', { type: 'button', className: cn(button({ variant, size, radius }), className), ...rest }, children);
}
