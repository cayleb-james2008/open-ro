// The house status pill (owner order, 4 Nov 2026: "colour is DATA ONLY - status pills, deltas, chart series,
// progress fills").
//
// A pill is the one place a colour may sit on a surface, and only when it carries a state: LIVE, LIVE CHAIN,
// SHADOW, MANUAL, SYNTHETIC, a crit or a warn. Everything else on the page is a zero-chroma neutral. The shape
// (a solid fill, a heavy outline, a dotted outline) is the functional twin the spec's mode table asks for, so the
// pill reads the same in a monochrome screenshot as it does in colour.
//
// No hover reaction (SPEC 17): a pill is a fact, not a control.
import { createElement as h } from 'react';
import { cva } from 'class-variance-authority';
import { cn } from './cn.js';

export const pill = cva('relative inline-flex items-center h-chip px-2 text-chip font-strong leading-none uppercase whitespace-nowrap rounded-base', {
  variants: {
    tone: {
      // the mode table (SPEC 5.1): the fill, the outline and the notch are the twins of the pad shapes
      live: 'bg-chip-live text-chip-live-text',
      'live-chain': 'bg-transparent text-text-1 border-2 border-chip-live-chain',
      shadow: 'bg-transparent text-text-1 border-2 border-shadow-chip',
      manual: 'bg-transparent text-text-1 border border-dotted border-chip-manual',
      synthetic: 'bg-transparent text-text-1 border border-synthetic',
      // the state colours: crit and warn are the only filled state pills
      crit: 'bg-red text-base border border-red',
      warn: 'bg-amber text-base border border-amber',
      info: 'bg-info-bar text-text-1 border border-info-bar',
      // a neutral tag (a plain word, no state): a hairline, no colour
      neutral: 'bg-transparent text-text-2 border border-line',
    },
  },
  defaultVariants: { tone: 'neutral' },
});

/**
 * @param {Object} p
 * @param {'live'|'live-chain'|'shadow'|'manual'|'synthetic'|'crit'|'warn'|'info'|'neutral'} [p.tone]
 * @param {string} [p.className]
 */
export function Pill({ tone = 'neutral', className, children, ...rest }) {
  return h('span', { className: cn(pill({ tone }), className), ...rest }, children);
}
