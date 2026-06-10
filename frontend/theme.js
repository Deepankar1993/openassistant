// openAssistant — theme system.
// Light/dark palettes live in styles.css as CSS custom properties keyed off
// html[data-theme]. Default follows prefers-color-scheme; a manual choice made
// via the sun/moon toggle is persisted to localStorage("oa-theme").
// Loaded in <head> so the palette applies before first paint (no flash).

(() => {
  "use strict";

  const KEY = "oa-theme";

  const SUN_ICON =
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
    '<circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/>' +
    '<line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/>' +
    '<line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/>' +
    '<line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>';
  const MOON_ICON =
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
    '<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>';

  function stored() {
    try {
      const v = localStorage.getItem(KEY);
      return v === "light" || v === "dark" ? v : null;
    } catch (_) {
      return null;
    }
  }

  function systemPref() {
    try {
      return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
    } catch (_) {
      return "light";
    }
  }

  function current() {
    return stored() || systemPref();
  }

  function apply(theme) {
    document.documentElement.setAttribute("data-theme", theme);
    const link = document.getElementById("hljs-theme");
    if (link) {
      link.setAttribute(
        "href",
        theme === "dark" ? "vendor/hljs-github-dark.min.css" : "vendor/hljs-github.min.css"
      );
    }
    const btn = document.getElementById("theme-toggle");
    if (btn) {
      // Show the icon for the theme the click switches TO.
      btn.innerHTML = theme === "dark" ? SUN_ICON : MOON_ICON;
      btn.setAttribute(
        "aria-label",
        theme === "dark" ? "Switch to light theme" : "Switch to dark theme"
      );
    }
  }

  window.OATheme = {
    get: () => document.documentElement.getAttribute("data-theme") || current(),
    set(theme) {
      try { localStorage.setItem(KEY, theme); } catch (_) {}
      apply(theme);
    },
    toggle() {
      this.set(this.get() === "dark" ? "light" : "dark");
    },
  };

  // Apply immediately (script runs in <head>; the toggle button does not exist
  // yet — re-applied on DOMContentLoaded to set its icon).
  apply(current());
  document.addEventListener("DOMContentLoaded", () => apply(window.OATheme.get()));

  // Follow OS changes only while the user hasn't made a manual choice.
  try {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => { if (!stored()) apply(systemPref()); };
    if (mq.addEventListener) mq.addEventListener("change", onChange);
    else if (mq.addListener) mq.addListener(onChange);
  } catch (_) {}

  document.addEventListener("click", (e) => {
    if (e.target.closest && e.target.closest("#theme-toggle")) window.OATheme.toggle();
  });
})();
