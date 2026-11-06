import { forwardRef, type ButtonHTMLAttributes } from 'react';
import { cn } from '../../lib/utils';

/**
 * Square UI grammar button (shadcn/ui pattern, fleet admission style):
 * variants restyled to the Open-Ro token variables, sentence case, no
 * gradients, radius capped at 10px on content surfaces.
 */
export type ButtonVariant = 'primary' | 'outline' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md' | 'icon';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'outline', size = 'md', className, type = 'button', ...props }, ref) => (
    <button ref={ref} type={type} className={cn('ui-btn', `ui-btn-${variant}`, `ui-btn-${size}`, className)} {...props} />
  ),
);
Button.displayName = 'Button';
