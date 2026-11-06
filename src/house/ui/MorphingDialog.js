// MorphingDialog - the Motion Primitives pattern for a dialog that grows out of its trigger (motion-primitives.com,
// MIT; our own implementation of the pattern).
//
// The dialog is a floating layer, so it is the one place the house rules allow a shadow (tokens.json's
// `elevation.float`): it floats above the page, and the hairline that separates everything else does not apply
// to it. It grows from the shape of its trigger to the panel it becomes, so the eye follows one object instead
// of two.
//
// NOT MOUNTED IN THE WATCHOVER SURFACE, on purpose: SPEC 17 allows exactly two controls on this page (the 3D
// toggle and the details disclosures) and a dialog would be a third. The primitive is here for the surfaces that
// need one (the DSH face, the Ergon surface, the landing page), on the same tokens.
//
// The trap: Tab and Shift+Tab cycle inside the dialog, Escape closes it, and the focus moves to the dialog when it
// opens and back to the trigger when it closes.
import { createElement as h, useEffect, useRef } from 'react';
import { cn } from './cn.js';

const FOCUSABLE = 'a[href], button:not([disabled]), input, select, textarea, [tabindex]:not([tabindex="-1"])';

/**
 * @param {Object} p
 * @param {boolean} p.open
 * @param {()=>void} p.onClose
 * @param {string} [p.title]
 * @param {boolean} [p.reducedMotion]
 * @param {string} [p.className]
 */
export function MorphingDialog({ open, onClose, title, reducedMotion = false, className, children, ...rest }) {
  const panelRef = useRef(null);
  const restoreRef = useRef(null);

  useEffect(() => {
    if (!open) return undefined;
    restoreRef.current = document.activeElement;
    const panel = panelRef.current;
    if (panel) {
      const first = panel.querySelector(FOCUSABLE);
      (first ?? panel).focus({ preventScroll: true });
    }
    const onKey = (e) => {
      if (e.key === 'Escape') { e.stopPropagation(); onClose(); return; }
      if (e.key !== 'Tab' || !panelRef.current) return;
      const items = [...panelRef.current.querySelectorAll(FOCUSABLE)];
      if (!items.length) { e.preventDefault(); return; }
      const first = items[0];
      const last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    };
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      const back = restoreRef.current;
      if (back && typeof back.focus === 'function') back.focus({ preventScroll: true });
    };
  }, [open, onClose]);

  if (!open) return null;
  return h('div', {
    className: cn('fixed inset-0 z-header flex items-center justify-center p-gutter', className),
    'data-motion': 'dialog',
    'data-open': reducedMotion ? '1' : '0',
    ...rest,
  },
    h('div', {
      className: 'absolute inset-0 bg-base',
      'data-dialog': 'scrim',
      onClick: onClose,
    }),
    h('div', {
      ref: panelRef,
      role: 'dialog',
      'aria-modal': 'true',
      'aria-label': title,
      tabIndex: -1,
      className: 'relative bg-panel border border-line rounded-base',
      'data-dialog': 'panel',
    },
      title ? h('h2', { className: 'text-label font-label uppercase tracking-label text-text-1 px-card-padding-x pt-card-padding-y' }, title) : null,
      children));
}
