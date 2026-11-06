// TextMorph - the Motion Primitives pattern for a word that changes (motion-primitives.com, MIT; our own
// implementation of the pattern).
//
// A state word that flips between ALIVE, STALE and DEAD must be readable in the moment it changes: a reader
// glancing at the screen must see that the word moved, not that the page blinked. So the old word fades and
// lifts out, the new word fades and lifts in, and the two never sit on top of each other.
//
// The text in the DOM is always the whole word - there is no half-rendered state, and no character-by-character
// scramble: a monitoring surface must not show a word that is not a word. Under prefers-reduced-motion the word
// simply changes.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { cn } from './cn.js';
import { useReducedMotion } from './useReducedMotion.js';

/**
 * @param {Object} p
 * @param {string} p.text
 * @param {number} [p.durationMs]   the whole morph (half out, half in)
 * @param {boolean} [p.reducedMotion]  forces the choice; by default the browser's preference decides
 * @param {string} [p.className]
 */
export function TextMorph({ text, durationMs = 180, reducedMotion, className, children, ...rest }) {
  const prefersReduced = useReducedMotion();
  const honour = reducedMotion === undefined ? prefersReduced : reducedMotion;
  const [shown, setShown] = useState(text);
  const [phase, setPhase] = useState('idle'); // idle | out | in
  const shownRef = useRef(text);
  const timerRef = useRef(0);
  const frameRef = useRef(0);

  useEffect(() => {
    if (text === shownRef.current) return undefined;
    if (honour) { shownRef.current = text; setShown(text); return undefined; }
    const half = Math.round(durationMs / 2);
    setPhase('out');
    timerRef.current = setTimeout(() => {
      shownRef.current = text;
      setShown(text);
      setPhase('in');
      frameRef.current = requestAnimationFrame(() => setPhase('idle'));
    }, half);
    return () => { if (timerRef.current) clearTimeout(timerRef.current); cancelAnimationFrame(frameRef.current); };
  }, [text, durationMs, honour]);

  return h('span', {
    className: cn('inline-block', className),
    'data-motion': 'morph',
    'data-phase': phase,
    ...rest,
  }, shown, children);
}
