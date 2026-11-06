import { forwardRef, type HTMLAttributes } from 'react';
import { cn } from '../../lib/utils';

/** Square UI grammar card: solid surface, one tone step + quiet border, no shadow. */
export const Card = forwardRef<HTMLDivElement, HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => <div ref={ref} className={cn('ui-card', className)} {...props} />,
);
Card.displayName = 'Card';

export const CardHeader = forwardRef<HTMLDivElement, HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => <div ref={ref} className={cn('ui-card-header', className)} {...props} />,
);
CardHeader.displayName = 'CardHeader';

export const CardTitle = forwardRef<HTMLHeadingElement, HTMLAttributes<HTMLHeadingElement>>(
  ({ className, ...props }, ref) => <h3 ref={ref} className={cn('ui-card-title', className)} {...props} />,
);
CardTitle.displayName = 'CardTitle';

export const CardContent = forwardRef<HTMLDivElement, HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => <div ref={ref} className={cn('ui-card-content', className)} {...props} />,
);
CardContent.displayName = 'CardContent';
