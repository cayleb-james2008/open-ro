import { useEffect, useRef, type ReactNode } from 'react';

/** Modal with Escape, a focus trap, and restoration of the control that opened it. */
export function Dialog({ labelledBy, onClose, children, className }: { labelledBy: string; onClose: () => void; children: ReactNode; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    const root = ref.current;
    if (!root) {
      return;
    }
    const selector = 'button, [href], input, textarea, select, [tabindex]:not([tabindex="-1"])';
    const items = (): HTMLElement[] => [...root.querySelectorAll<HTMLElement>(selector)].filter((element) => !element.hasAttribute('disabled'));
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    (root.querySelector<HTMLElement>('input, textarea, select') ?? items()[0])?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        closeRef.current();
        return;
      }
      if (event.key !== 'Tab') {
        return;
      }
      const list = items();
      if (list.length === 0) {
        return;
      }
      const first = list[0];
      const last = list[list.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('keydown', onKey);
      previous?.focus();
    };
  }, []);

  return (
    <div className="modal-backdrop" onMouseDown={() => closeRef.current()}>
      <div ref={ref} className={className} role="dialog" aria-modal="true" aria-labelledby={labelledBy} onMouseDown={(event) => event.stopPropagation()}>
        {children}
      </div>
    </div>
  );
}
