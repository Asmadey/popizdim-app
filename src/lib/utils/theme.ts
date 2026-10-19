import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands, type Theme } from "@/bindings";

/**
 * Appearance theme handling.
 *
 * Handy already ships a full light palette and a full dark palette (see
 * `App.css`). This module lets the user pick which one is used instead of
 * always following the OS:
 *  - `system` removes the override so the `prefers-color-scheme` media query
 *    governs (the historical behaviour).
 *  - `light` / `dark` set `data-theme` on the document root, whose
 *    higher-specificity CSS selectors win over the media query.
 *
 * The choice is persisted in `AppSettings` (source of truth) and mirrored to
 * localStorage so it can be applied synchronously on boot, before React mounts,
 * avoiding a flash of the wrong palette.
 */

export const THEME_STORAGE_KEY = "handy.theme";

export const THEME_OPTIONS: Theme[] = ["system", "light", "dark"];

const isTheme = (value: unknown): value is Theme =>
  value === "system" || value === "light" || value === "dark";

/**
 * WebKit keeps stale text colors in composited layers (backdrop-filter panels)
 * when the palette changes at runtime, leaving e.g. black labels on a dark
 * sidebar until restart. Dropping the root out of layout for one frame forces
 * a full repaint.
 */
const forceRepaint = (): void => {
  const root = document.documentElement;
  root.style.display = "none";
  void root.offsetHeight;
  root.style.display = "";
};

// With the "system" theme the palette flips when the OS (or the native window
// theme set below) changes, which can land after our own repaint.
if (typeof window !== "undefined" && window.matchMedia) {
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", forceRepaint);
}

/** Apply a theme to the document root and remember it for the next launch. */
export const applyTheme = (theme: Theme): void => {
  const root = document.documentElement;
  if (theme === "system") {
    delete root.dataset.theme;
  } else {
    root.dataset.theme = theme;
  }
  // Keep the native window material (vibrancy) in step with the chosen theme
  forceRepaint();
  getCurrentWindow()
    .setTheme(theme === "system" ? null : theme)
    .then(() => {
      requestAnimationFrame(forceRepaint);
      window.setTimeout(forceRepaint, 300);
    })
    .catch(() => {});
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // localStorage may be unavailable (e.g. private mode); the setting still
    // persists in AppSettings, so this only costs a one-frame flash on boot.
  }
};

/** Read the last-applied theme for synchronous boot-time application. */
export const getStoredTheme = (): Theme => {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (isTheme(stored)) return stored;
  } catch {
    // ignore
  }
  return "system";
};

/** Apply the persisted theme from AppSettings (the source of truth). */
export const syncThemeFromSettings = async (): Promise<void> => {
  try {
    const result = await commands.getAppSettings();
    if (result.status === "ok") {
      applyTheme(result.data.theme ?? "system");
    }
  } catch (e) {
    console.warn("Failed to sync theme from settings:", e);
  }
};
