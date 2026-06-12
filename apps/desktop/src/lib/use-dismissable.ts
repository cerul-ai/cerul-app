// Shared dismissal behaviour for transient surfaces (dialogs, sheets,
// popovers, row menus). Escape closes the surface; pointerdown outside the
// given ref closes it too. Page-level Escape handlers (e.g. detail "back")
// must stay quiet while any surface is open — they coordinate through
// hasOpenModalSurface() in App.tsx, which checks the DOM for open surfaces.

import { useEffect } from "react";
import type { RefObject } from "react";

export function useEscapeToClose(onClose: () => void, enabled = true) {
  useEffect(() => {
    if (!enabled) {
      return;
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, enabled]);
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
