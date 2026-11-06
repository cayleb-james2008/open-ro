import { forwardRef, type HTMLAttributes } from 'react';
import { cn } from '../../lib/utils';

export type BadgeTone = 'neutral' | 'success' | 'warning' | 'danger' | 'accent';

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: BadgeTone;
}

/** State pill in the Square grammar: colour carries state only, always paired with words. */
export const Badge = forwardRef<HTMLSpanElement, BadgeProps>(
  ({ tone = 'neutral', className, ...props }, ref) => (
    <span ref={ref} className={cn('ui-badge', `ui-badge-${tone}`, className)} {...props} />
  ),
);
Badge.displayName = 'Badge';
