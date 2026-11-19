'use client';
import { cn } from '@/lib/utils';
import { motion, Transition, useReducedMotion } from 'motion/react';

export type BorderTrailProps = {
  className?: string;
  size?: number;
  transition?: Transition;
  onAnimationComplete?: () => void;
  style?: React.CSSProperties;
};

// Local edit: the registry tint was a zinc class and its mask named a literal black; the tint is now the blue accent and
// the mask uses the opaque system colour CanvasText (a mask reads only alpha). Mount it only while the thing
// is running (live work); when the work ends, unmount it. Under prefers-reduced-motion nothing travels: the whole
// edge is drawn in the same colour, still, which says "running" without moving.
export function BorderTrail({
  className,
  size = 60,
  transition,
  onAnimationComplete,
  style,
}: BorderTrailProps) {
  const reduce = useReducedMotion();
  const defaultTransition: Transition = {
    repeat: Infinity,
    duration: 5,
    ease: 'linear',
  };

  return (
    <div
      aria-hidden
      data-border-trail={reduce ? 'still' : 'moving'}
      className='pointer-events-none absolute inset-0 rounded-[inherit] border border-transparent [mask-clip:padding-box,border-box] [mask-composite:intersect] [mask-image:linear-gradient(transparent,transparent),linear-gradient(CanvasText,CanvasText)]'
    >
      {reduce ? (
        <div className={cn('absolute inset-0 bg-blue', className)} />
      ) : (
        <motion.div
          className={cn('absolute aspect-square bg-blue', className)}
          style={{
            width: size,
            offsetPath: `rect(0 auto auto 0 round ${size}px)`,
            ...style,
          }}
          animate={{
            offsetDistance: ['0%', '100%'],
          }}
          transition={transition || defaultTransition}
          onAnimationComplete={onAnimationComplete}
        />
      )}
    </div>
  );
}
