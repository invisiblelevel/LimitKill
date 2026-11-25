const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Keeps keyboard focus inside a modal/popover and restores focus on destroy.
 * Usage: <div use:focusTrap>
 */
export function focusTrap(node: HTMLElement) {
  const previous = document.activeElement as HTMLElement | null;
  const fallback =
    document.querySelector<HTMLElement>('.settings-btn, .target-pill, .new-chat') ?? null;

  const items = () =>
    Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
      (el) => el.offsetParent !== null || el === document.activeElement
    );

  const focusFirst = () => {
    const first = items()[0];
    first?.focus();
  };

  const onKeydown = (event: KeyboardEvent) => {
    if (event.key !== 'Tab') return;
    const list = items();
    if (list.length === 0) return;
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

  node.addEventListener('keydown', onKeydown);
  queueMicrotask(focusFirst);

  return {
    destroy() {
      node.removeEventListener('keydown', onKeydown);
      if (previous && previous !== document.body) {
        previous.focus?.();
      } else {
        fallback?.focus?.();
      }
    },
  };
}
