// The house primitives (owner order, 4 Nov 2026: the shared token and shell foundation, in a form the later
// surfaces - the DSH face, the Ergon surface, the landing page - can consume).
//
// Every component here is our own code, written in the shadcn shape (a cva variant map, a `cn` joiner, a
// `...rest` passthrough). No component is copied from the Square UI demos or from a free template: those were
// design reference only.
//
// What the pack's own surface uses today: Button, Card, Pill, Stat, AnimatedNumber, TransitionPanel, TextMorph.
// MorphingDialog is here for the surfaces that need a floating layer and is not mounted here (SPEC 17 allows two
// controls on this page).
export { cn } from './cn.js';
export { Button, button } from './Button.js';
export { Card, CARD } from './Card.js';
export { Pill, pill } from './Pill.js';
export { Stat, stat } from './Stat.js';
export { AnimatedNumber } from './AnimatedNumber.js';
export { TransitionPanel } from './TransitionPanel.js';
export { TextMorph } from './TextMorph.js';
export { MorphingDialog } from './MorphingDialog.js';
export { easeOut, EASE_OUT, EASE_IN_OUT } from './ease.js';
export { useReducedMotion } from './useReducedMotion.js';
