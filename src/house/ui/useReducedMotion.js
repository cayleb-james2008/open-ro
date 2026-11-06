// The reduced-motion preference, read from the browser rather than from a prop.
//
// The house motion (SPEC 8.4) is instant under `prefers-reduced-motion: reduce`, and that has to hold for the
// primitives themselves: a caller that forgets to pass the preference must not get an animated value. So the
// primitives ask the browser. In node (the SSR renders the tests use) there is no `matchMedia`, and the answer is
// `false` - the same as a browser that does not reduce motion.
//
// The listener keeps up with the setting changing while the page is open (a reader who turns reduced motion on
// gets it on the next change).
import { useEffect, useState } from 'react';

const QUERY = '(prefers-reduced-motion: reduce)';

const current = () => (typeof matchMedia === 'function' ? matchMedia(QUERY).matches : false);

/**
 * @returns {boolean} true when the reader asked for reduced motion
 */
export function useReducedMotion() {
  const [reduced, setReduced] = useState(current);
  useEffect(() => {
    if (typeof matchMedia !== 'function') return undefined;
    const mq = matchMedia(QUERY);
    const onChange = () => setReduced(mq.matches);
    onChange();
    if (typeof mq.addEventListener === 'function') mq.addEventListener('change', onChange);
    else if (typeof mq.addListener === 'function') mq.addListener(onChange); // an older WebKit
    return () => {
      if (typeof mq.removeEventListener === 'function') mq.removeEventListener('change', onChange);
      else if (typeof mq.removeListener === 'function') mq.removeListener(onChange);
    };
  }, []);
  return reduced;
}
