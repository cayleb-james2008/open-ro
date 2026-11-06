import { forwardRef, type ButtonHTMLAttributes } from 'react';
import { cn } from '../../lib/utils';

/**
 * Kitchen tactile button pattern, re-implemented in original code: a
 * physical key with a compressible bottom edge. Resting, the key sits on
 * its edge; pressed, it travels down and the edge collapses. Used only for
 * the composer's send key — a frequent action keeps its motion to the
 * press itself (Emil frequency gate: feedback, never decoration).
 */
export const TactileButton = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement>>(
  ({ className, type = 'button', children, ...props }, ref) => (
    <button ref={ref} type={type} className={cn('kitchen-tactile', className)} {...props}>
      <span className="kitchen-tactile-face">{children}</span>
    </button>
  ),
);
TactileButton.displayName = 'TactileButton';
