// The house stat value (owner order, 4 Nov 2026: "stat values the only big type (text-2xl/3xl semibold)",
// "Geist Mono for every number/ID/timestamp").
//
// A stat is a number, so it is set in Geist Mono with tabular figures: the digits do not jitter when the value
// changes, and a column of them lines up. The label above it is UI text and stays in Geist Sans.
//
// The size is a variant, not a free number: `stat` is 30 px (text-3xl) on a front plate, `stat-compact` is 24 px
// (text-2xl) on a shelf plate and on the phone, and `meta` is 14 px for a figure that is not the headline of its
// card. Nothing else in the pack may be bigger than 14 px.
import { createElement as h } from 'react';
import { cva } from 'class-variance-authority';
import { cn } from './cn.js';

export const stat = cva('font-mono tabular-nums', {
  variants: {
    size: {
      stat: 'text-stat font-label',
      'stat-compact': 'text-stat-compact font-label',
      meta: 'text-meta font-regular',
    },
  },
  defaultVariants: { size: 'meta' },
});

/**
 * @param {Object} p
 * @param {'stat'|'stat-compact'|'meta'} [p.size]
 * @param {string} [p.className]
 */
export function Stat({ size = 'meta', className, children, ...rest }) {
  return h('span', { className: cn(stat({ size }), className), ...rest }, children);
}
