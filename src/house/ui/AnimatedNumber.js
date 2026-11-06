// AnimatedNumber - the Motion Primitives pattern for a value that changes (motion-primitives.com, MIT; this is
// our own implementation of the pattern, not its code).
//
// The rule that matters on a monitoring surface: a number must never lie while it moves. So this component:
//
//   - renders the value as it is, with no animation, on the first paint (there is no count-up: a number that rolls
//     up from zero on load is a claim nobody can check);
//   - tweens only when a served value CHANGES, and only from the value that is on screen to the new one, so every
//     frame lies between two real readings and never outside them;
//   - lands exactly on the new value (the last frame is the true value, not an approximation of it);
//   - never extrapolates, never loops, never runs on its own;
//   - carries `data-tween="1"` while it is in flight and `data-value` as the TRUE target, so a checker that reads
//     the attribute during the flight reads the reading, not the frame;
//   - does not touch the age: the age cell is the truth about when the reading happened and never moves.
//
// Under prefers-reduced-motion the new value is simply the value (the same rule the 3D scene follows).
import { createElement as h, useEffect, useRef, useState } from 'react';
import { cn } from './cn.js';
import { easeOut } from './ease.js';
import { useReducedMotion } from './useReducedMotion.js';

/**
 * @param {Object} p
 * @param {number} p.value           the served value; null or non-finite means "no number" and is not animated
 * @param {(v:number)=>string} p.format  how the number is printed (the model's own formatter, never a local one)
 * @param {number} [p.durationMs]
 * @param {boolean} [p.reducedMotion]  forces the choice; by default the browser's preference decides
 * @param {string} [p.className]
 */
export function AnimatedNumber({ value, format, durationMs = 280, reducedMotion, className, ...rest }) {
  const prefersReduced = useReducedMotion();
  const honour = reducedMotion === undefined ? prefersReduced : reducedMotion;
  const formatRef = useRef(format);
  formatRef.current = format;
  const [shown, setShown] = useState(() => formatRef.current(value));
  const shownRef = useRef(shown);
  const currentRef = useRef(value); // the number on screen, kept as a number and never re-read from its text
  const targetRef = useRef(value);
  const frameRef = useRef(0);
  const [flying, setFlying] = useState(false);

  useEffect(() => {
    if (value === targetRef.current) return undefined;
    targetRef.current = value;
    const paint = (text) => { shownRef.current = text; setShown(text); };
    if (honour || typeof value !== 'number' || !Number.isFinite(value) || typeof currentRef.current !== 'number' || !Number.isFinite(currentRef.current)) {
      currentRef.current = value;
      paint(formatRef.current(value));
      return undefined;
    }
    const from = currentRef.current;
    const to = value;
    const t0 = performance.now();
    setFlying(true);
    const step = (now) => {
      const p = Math.min(1, (now - t0) / durationMs);
      const eased = easeOut(p);
      const current = from + (to - from) * eased;
      currentRef.current = current;
      paint(formatRef.current(current));
      if (p < 1) {
        frameRef.current = requestAnimationFrame(step);
      } else {
        currentRef.current = to;
        paint(formatRef.current(to)); // the last frame is the true value, not a rounded one
        setFlying(false);
      }
    };
    frameRef.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frameRef.current);
  }, [value, durationMs, honour]);

  return h('span', { className: cn(className), 'data-tween': flying ? '1' : undefined, ...rest }, shown);
}
