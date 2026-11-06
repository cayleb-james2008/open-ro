// TransitionPanel - the Motion Primitives pattern for a panel that opens and closes (motion-primitives.com, MIT;
// our own implementation of the pattern).
//
// The panel is measured, not guessed: it opens from zero height to its own height over the house panel duration
// and closes back to zero. The content is never unmounted - A4 wants every freshness name in the DOM whether or
// not the reader opened the panel - so the panel carries the `hidden` attribute only once the close transition has
// finished. The height is the only thing that moves: the content inside does not shift.
//
// Under prefers-reduced-motion the panel opens and closes at once.
import { createElement as h, useEffect, useRef, useState } from 'react';
import { cn } from './cn.js';
import { useReducedMotion } from './useReducedMotion.js';

/**
 * @param {Object} p
 * @param {boolean} p.open
 * @param {number} [p.durationMs]
 * @param {boolean} [p.reducedMotion]  forces the choice; by default the browser's preference decides
 * @param {string} [p.className]
 */
export function TransitionPanel({ open, durationMs = 220, reducedMotion, className, children, ...rest }) {
  const prefersReduced = useReducedMotion();
  const honour = reducedMotion === undefined ? prefersReduced : reducedMotion;
  const [shown, setShown] = useState(!!open); // the panel is in the layout (not `hidden`)
  const [height, setHeight] = useState(open ? null : 0);
  const innerRef = useRef(null);
  const timerRef = useRef(0);

  useEffect(() => {
    if (timerRef.current) { clearTimeout(timerRef.current); timerRef.current = 0; }
    if (open) {
      setShown(true);
      if (honour) { setHeight(null); return undefined; }
      // one frame at zero height, then the measured height: the transition needs two states to move between
      setHeight(0);
      const frame = requestAnimationFrame(() => {
        const h = innerRef.current ? innerRef.current.scrollHeight : 0;
        setHeight(h);
        timerRef.current = setTimeout(() => setHeight(null), durationMs); // then the panel sizes itself
      });
      return () => cancelAnimationFrame(frame);
    }
    if (honour) { setShown(false); setHeight(0); return undefined; }
    setHeight(0);
    timerRef.current = setTimeout(() => { setShown(false); timerRef.current = 0; }, durationMs);
    return undefined;
  }, [open, durationMs, honour]);

  useEffect(() => () => { if (timerRef.current) clearTimeout(timerRef.current); }, []);

  return h('div', {
    className: cn('overflow-hidden', className),
    'data-motion': 'panel',
    'data-open': open ? '1' : '0',
    hidden: shown ? undefined : true,
    style: { height: height === null ? 'auto' : `${height}px` },
    ...rest,
  }, h('div', { ref: innerRef }, children));
}
