// openAssistant — markdown pipeline + shared inline SVG icons.
// Pipeline: DOMPurify.sanitize(marked.parse(text)) -> innerHTML, with a
// textContent fallback when the vendored libraries are unavailable.
// Loaded before app.js; exposes window.OAMarkdown and window.OAIcons.

(() => {
  "use strict";

  // ── Icons (Lucide-style line icons, stroke=currentColor) ──────────────────
  const svg = (paths, cls) =>
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="15" height="15" fill="none" ' +
    'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"' +
    (cls ? ' class="' + cls + '"' : "") +
    ">" + paths + "</svg>";

  window.OAIcons = {
    check: svg('<polyline points="20 6 9 17 4 12"/>'),
    cross: svg('<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>'),
    warn: svg(
      '<path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>' +
      '<line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/>'
    ),
    clock: svg('<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>'),
    spinner: svg('<path d="M21 12a9 9 0 1 1-6.219-8.56"/>', "spin"),
    copy: svg(
      '<rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>' +
      '<path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>'
    ),
  };

  // ── Markdown ───────────────────────────────────────────────────────────────
  // Text-based tool-call markup emitted by the agent; stripped from display.
  const TOOL_MARKUP_RE = /\[TOOL:\w+:\{.*?\}\]/g;

  function stripToolMarkup(text) {
    return String(text == null ? "" : text).replace(TOOL_MARKUP_RE, "").trim();
  }

  function available() {
    return typeof marked !== "undefined" && typeof DOMPurify !== "undefined";
  }

  /**
   * Render `text` as sanitized markdown into `el`.
   * Falls back to plain textContent when marked/DOMPurify are missing.
   */
  function render(el, text) {
    const clean = stripToolMarkup(text);
    if (available()) {
      try {
        const html = marked.parse(clean, { gfm: true, breaks: true });
        el.innerHTML = DOMPurify.sanitize(html);
        el.classList.add("md");
        return;
      } catch (_) {
        /* fall through to plain text */
      }
    }
    el.classList.remove("md");
    el.textContent = clean;
  }

  function languageOf(codeEl) {
    const m = (codeEl.className || "").match(/language-([\w#+-]+)/);
    return m ? m[1] : "";
  }

  /**
   * Post-completion pass: syntax-highlight each fenced code block and attach a
   * header bar (language label + copy button) to it. Idempotent.
   */
  function enhance(rootEl) {
    if (!rootEl) return;
    rootEl.querySelectorAll("pre").forEach((pre) => {
      if (pre.closest(".code-block")) return; // already enhanced
      const code = pre.querySelector("code");
      if (!code) return;

      if (typeof hljs !== "undefined") {
        try { hljs.highlightElement(code); } catch (_) {}
      }

      const wrap = document.createElement("div");
      wrap.className = "code-block";

      const head = document.createElement("div");
      head.className = "code-block-head";

      const lang = document.createElement("span");
      lang.className = "code-block-lang";
      lang.textContent = languageOf(code) || "text";

      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "code-copy-btn";
      btn.setAttribute("aria-label", "Copy code");
      btn.innerHTML = window.OAIcons.copy + "<span>Copy</span>"; // static markup
      btn.addEventListener("click", async () => {
        try { await navigator.clipboard.writeText(code.innerText); } catch (_) {}
        btn.innerHTML = "<span>Copied ✓</span>"; // static markup
        clearTimeout(btn._t);
        btn._t = setTimeout(() => {
          btn.innerHTML = window.OAIcons.copy + "<span>Copy</span>";
        }, 1500);
      });

      head.appendChild(lang);
      head.appendChild(btn);
      pre.parentNode.insertBefore(wrap, pre);
      wrap.appendChild(head);
      wrap.appendChild(pre);
    });
  }

  window.OAMarkdown = { stripToolMarkup, render, enhance, available };
})();
