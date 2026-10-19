import { useLayoutEffect, type RefObject } from "react";

/**
 * Single-line fields never wrap. When the text is wider than the field, its
 * end stays in view (the last part is the informative one for paths, keys and
 * names). A text input being edited keeps its own caret position.
 */
export const useShowEndOfText = (
  ref: RefObject<HTMLElement | null>,
  value: unknown,
): void => {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const toEnd = () => {
      if (document.activeElement !== el) el.scrollLeft = el.scrollWidth;
    };
    toEnd();
    const observer = new ResizeObserver(toEnd);
    observer.observe(el);
    el.addEventListener("blur", toEnd);
    return () => {
      observer.disconnect();
      el.removeEventListener("blur", toEnd);
    };
  }, [ref, value]);
};
