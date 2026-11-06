import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { cn } from '../../lib/utils';

/**
 * Kitchen "Hold to Confirm" pattern, re-implemented in original code for
 * Open-Ro (Kitchen publishes no licence; the pattern is the fleet's to
 * rebuild — the same route the Ergon face took). A tap can never fire it:
 * the button must be held while it fills, and releasing early drains it
 * back. That makes the approval doctrine physical: nothing sends, writes
 * or deletes on an accidental click. Keyboard: hold Space or Enter.
 */
export function HoldToConfirm({
  children,
  onConfirm,
  disabled,
  holdMs = 900,
  className,
  title,
}: {
  children: ReactNode;
  onConfirm: () => void;
  disabled?: boolean;
  holdMs?: number;
  className?: string;
  title?: string;
}) {
  const [holding, setHolding] = useState(false);
  const [done, setDone] = useState(false);
  const timer = useRef<number | null>(null);

  const cancel = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    setHolding(false);
  }, []);

  const start = useCallback(() => {
    if (disabled || done) {
      return;
    }
    cancel();
    setHolding(true);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      setHolding(false);
      setDone(true);
      onConfirm();
    }, holdMs);
  }, [cancel, disabled, done, holdMs, onConfirm]);

  useEffect(() => cancel, [cancel]);

  return (
    <button
      type="button"
      className={cn('kitchen-hold', className)}
      disabled={disabled}
      title={title}
      aria-keyshortcuts={undefined}
      onPointerDown={(event) => { event.preventDefault(); start(); }}
      onPointerUp={cancel}
      onPointerLeave={cancel}
      onContextMenu={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if ((event.key === ' ' || event.key === 'Enter') && !event.repeat) {
          event.preventDefault();
          start();
        }
      }}
      onKeyUp={(event) => {
        if (event.key === ' ' || event.key === 'Enter') {
          event.preventDefault();
          cancel();
        }
      }}
      onClick={(event) => event.preventDefault()}
    >
      <span
        className="kitchen-hold-fill"
        style={{ transform: holding ? 'scaleX(1)' : 'scaleX(0)', transitionDuration: holding ? `${holdMs}ms` : '180ms' }}
        aria-hidden="true"
      />
      <span className="kitchen-hold-label">{children}</span>
    </button>
  );
}
