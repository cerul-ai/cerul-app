// Shared dismissal behaviour for transient surfaces (dialogs, sheets,
// popovers, row menus). Escape closes the surface; pointerdown outside the
// given ref closes it too. Page-level Escape handlers (e.g. detail "back")
// must stay quiet while any surface is open — they coordinate through
// hasOpenModalSurface() in App.tsx, which checks the DOM for open surfaces.

import { useEffect, useRef } from "react";
import type { RefObject } from "react";

// Stack of currently-open dismissable surfaces. Escape only closes the most
// recently opened one; without this, a confirm dialog stacked on top of a
// form dialog used to close both at once and lose the user's input.
const escapeStack: Array<{ close: () => void }> = [];
let escapeListenerAttached = false;

function onGlobalEscape(event: KeyboardEvent) {
  if (event.key !== "Escape" || escapeStack.length === 0) {
    return;
  }
  event.preventDefault();
  escapeStack[escapeStack.length - 1].close();
}

function ensureEscapeListener() {
  if (!escapeListenerAttached) {
    window.addEventListener("keydown", onGlobalEscape);
    escapeListenerAttached = true;
  }
}

export function useEscapeToClose(onClose: () => void, enabled = true) {
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    if (!enabled) {
      return;
    }
    ensureEscapeListener();
    const entry = { close: () => closeRef.current() };
    escapeStack.push(entry);
    return () => {
      const index = escapeStack.indexOf(entry);
      if (index >= 0) escapeStack.splice(index, 1);
    };
  }, [enabled]);
}

export function useClickOutside(
  ref: RefObject<HTMLElement | null>,
  onClose: () => void,
  enabled = true,
) {
  useEffect(() => {
    if (!enabled) {
      return;
    }
    function onPointerDown(event: PointerEvent) {
      const node = ref.current;
      if (node && event.target instanceof Node && !node.contains(event.target)) {
        onClose();
      }
    }
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [ref, onClose, enabled]);
}

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

// Dialog focus management: move focus into the surface on open, keep Tab
// cycling inside it, and hand focus back to the trigger on close. Without
// this, aria-modal dialogs left focus on the background page.
export function useDialogFocus(ref: RefObject<HTMLElement | null>, enabled = true) {
  useEffect(() => {
    if (!enabled) {
      return;
    }
    const node = ref.current;
    if (!node) {
      return;
    }
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusables = () =>
      [...node.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)].filter(
        (el) => el.offsetParent !== null,
      );
    const first = focusables()[0];
    if (first) {
      first.focus();
    } else {
      node.tabIndex = -1;
      node.focus();
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Tab") {
        return;
      }
      const items = focusables();
      if (items.length === 0) {
        return;
      }
      const firstItem = items[0];
      const lastItem = items[items.length - 1];
      if (event.shiftKey && document.activeElement === firstItem) {
        event.preventDefault();
        lastItem.focus();
      } else if (!event.shiftKey && document.activeElement === lastItem) {
        event.preventDefault();
        firstItem.focus();
      }
    }
    node.addEventListener("keydown", onKeyDown);
    return () => {
      node.removeEventListener("keydown", onKeyDown);
      previous?.focus();
    };
  }, [ref, enabled]);
}
