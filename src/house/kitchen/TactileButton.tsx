// Kitchen — the Tactile Button, carried into React from the fleet's own dashboard build.
//
// Source on this machine: /home/cayleb/.dsh-fleet-state/missions/m-0035/work/build-dashboard.mjs
// (sha256 82223b141867c2ce6df0c40561a2b2c81b0ddb1f253a9b978d8d5073af96547a), the accepted
// ticket-001 dashboard. The recipe is the one its report names: "a shaped face, firm edge,
// compressible depth". `drawPanelNode()` draws a wall behind the face (its `.n-depth` and
// `.n-face` classes) and lays a 1 px lip on the top edge (`.n-lip`); this component keeps the
// same three parts — `.tactile-edge` (the wall), `.tactile-face` (the plate) and its top lip —
// so the face can press down into the wall on :active.
//
// House rules: the 8 px control radius, a 1 px hairline, no shadow, no gradient, no idle motion.
// Colour is material only, from the public/house/house.css custom properties; the press is a
// transform plus a background-colour change, never a transition on every property.
import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { cn } from '../ui/cn.js';

export type TactileButtonVariant = 'primary' | 'quiet';
export type TactileButtonSize = 'md' | 'sm';

export interface TactileButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** `primary` is the white plate on the dark surface; `quiet` is a hairline panel. */
  variant?: TactileButtonVariant;
  /** `md` keeps the 44 px target; `sm` is the 28 px in-card control. */
  size?: TactileButtonSize;
  className?: string;
  children: ReactNode;
}

export function TactileButton({
  variant = 'primary',
  size = 'md',
  className,
  children,
  ...rest
}: TactileButtonProps) {
  return (
    <button
      {...rest}
      type="button"
      className={cn('tactile', `tactile-${variant}`, `tactile-${size}`, className)}
      data-variant={variant}
      data-size={size}
    >
      <span className="tactile-edge" aria-hidden="true" />
      <span className="tactile-face">{children}</span>
    </button>
  );
}
