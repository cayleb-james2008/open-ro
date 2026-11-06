// The house easing curves, as functions (the CSS curves of tokens.json, so a JS animation and a CSS transition
// move on the same curve). A cubic-bezier(x1, y1, x2, y2) is solved the way the CSS specification solves it:
// the x of the curve is found by Newton-Raphson (or bisection when the derivative is too small) and the y of that
// point is the eased value.
export const EASE_OUT = [0.16, 1, 0.3, 1];
export const EASE_IN_OUT = [0.4, 0, 0.2, 1];

const curve = (p1x, p1y, p2x, p2y) => {
  const cx = 3 * p1x;
  const bx = 3 * (p2x - p1x) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * p1y;
  const by = 3 * (p2y - p1y) - cy;
  const ay = 1 - cy - by;
  const sampleX = (t) => ((ax * t + bx) * t + cx) * t;
  const sampleY = (t) => ((ay * t + by) * t + cy) * t;
  const sampleDX = (t) => (3 * ax * t + 2 * bx) * t + cx;
  return (x) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let t = x;
    for (let i = 0; i < 8; i += 1) {
      const d = sampleX(t) - x;
      if (Math.abs(d) < 1e-6) return sampleY(t);
      const dx = sampleDX(t);
      if (Math.abs(dx) < 1e-6) break;
      t -= d / dx;
    }
    let lo = 0;
    let hi = 1;
    t = x;
    for (let i = 0; i < 20; i += 1) {
      const v = sampleX(t);
      if (Math.abs(v - x) < 1e-6) break;
      if (v > x) hi = t; else lo = t;
      t = (lo + hi) / 2;
    }
    return sampleY(t);
  };
};

const cache = new Map();

/** The eased value of `x` (0..1) on the house curve. */
export const easeOut = (x) => {
  if (!cache.has('out')) cache.set('out', curve(...EASE_OUT));
  return cache.get('out')(x);
};
