// openAssistant desktop frontend logic.
// Talks to the Rust core via Tauri's invoke when running inside the app, and
// falls back to an injectable mock backend when loaded in a plain browser
// (used by Playwright UI tests that have no native WebDriver — see tasks 5.x).

(() => {
  "use strict";

  const tauri = window.__TAURI__;
  const realInvoke = tauri && tauri.core && tauri.core.invoke;

  // Backend abstraction. In a browser without Tauri, allow tests to inject
  // window.__MOCK_BACKEND__ = async (cmd, args) => {...}; otherwise use a
  // minimal default mock so the shell is still explorable.
  const backend = realInvoke
    ? (cmd, args) => realInvoke(cmd, args)
    : (cmd, args) =>
        (window.__MOCK_BACKEND__ || defaultMock)(cmd, args || {});

  const mockState = { config: { model: "openrouter/owl-alpha", api_base: "https://openrouter.ai/api/v1", api_key_set: false, api_key_masked: "", provider: "openrouter" }, history: [], tools: false };
  async function defaultMock(cmd, args) {
    switch (cmd) {
      case "get_config": return { ...mockState.config };
      case "get_status": return { model: mockState.config.model, provider: "openrouter", mode: mockState.tools ? "tools" : "chat", workspace: "(mock)", message_count: mockState.history.length, tools_enabled: mockState.tools, api_key_set: mockState.config.api_key_set };
      case "get_history": return mockState.history.slice();
      case "clear_conversation": mockState.history = []; return null;
      case "set_tools_enabled": mockState.tools = !!args.enabled; return null;
      case "save_config":
        mockState.config.model = args.model; mockState.config.api_base = args.api_base;
        if (args.api_key) { mockState.config.api_key_set = true; mockState.config.api_key_masked = "••••••••" + String(args.api_key).slice(-4); }
        return null;
      case "send_message": {
        if (!mockState.config.api_key_set) throw "No API key configured.";
        const user = { id: String(Math.random()), role: "user", content: args.message, timestamp: new Date().toISOString(), metadata: null };
        const asst = { id: String(Math.random()), role: "assistant", content: "(mock reply) " + args.message, timestamp: new Date().toISOString(), metadata: null };
        mockState.history.push(user, asst);
        return asst;
      }
      default: return null;
    }
  }

  // ── DOM helpers ───────────────────────────────────
  const $ = (sel) => document.querySelector(sel);
  const messageList = $("#message-list");
  const chatInput = $("#chat-input");
  const sendBtn = $("#send-btn");

  function showToast(msg, isError) {
    const t = $("#toast");
    t.textContent = msg;
    t.classList.toggle("err", !!isError);
    t.classList.remove("hidden");
    clearTimeout(showToast._t);
    showToast._t = setTimeout(() => t.classList.add("hidden"), 2600);
  }

  // ── View routing ──────────────────────────────────
  const views = ["chat", "settings", "memory", "integrations", "skills"];
  function switchView(name) {
    if (!views.includes(name)) return;
    views.forEach((v) => $("#view-" + v).classList.toggle("hidden", v !== name));
    document.querySelectorAll(".nav-item").forEach((b) =>
      b.classList.toggle("active", b.dataset.view === name));
    if (name === "settings") loadConfig();
    if (name === "chat") refreshStatus();
  }
  document.addEventListener("click", (e) => {
    const el = e.target.closest("[data-view]");
    if (el && !el.disabled) switchView(el.dataset.view);
  });

  // ── Rendering ─────────────────────────────────────
  function emptyState() {
    const d = document.createElement("div");
    d.className = "empty-state";
    d.innerHTML = '<div class="big">🦉</div><div>Ask me anything to get started.</div>';
    return d;
  }
  function renderMessage(msg) {
    const wrap = document.createElement("div");
    const role = msg.role === "user" ? "user" : msg.role === "error" ? "error" : "assistant";
    wrap.className = "msg " + role;
    wrap.dataset.testid = "message-" + role;
    const avatar = role === "user" ? "🧑" : role === "error" ? "⚠️" : "🦉";
    wrap.innerHTML = `<div class="avatar">${avatar}</div><div class="bubble" data-testid="message-bubble"></div>`;
    wrap.querySelector(".bubble").textContent = msg.content;
    return wrap;
  }
  function appendMessage(msg) {
    const es = messageList.querySelector(".empty-state");
    if (es) es.remove();
    messageList.appendChild(renderMessage(msg));
    messageList.scrollTop = messageList.scrollHeight;
  }
  function showTyping() {
    const wrap = document.createElement("div");
    wrap.className = "msg assistant";
    wrap.id = "typing-indicator";
    wrap.dataset.testid = "typing-indicator";
    wrap.innerHTML = '<div class="avatar">🦉</div><div class="bubble"><span class="dots"><span></span><span></span><span></span></span></div>';
    messageList.appendChild(wrap);
    messageList.scrollTop = messageList.scrollHeight;
  }
  function hideTyping() { const t = $("#typing-indicator"); if (t) t.remove(); }

  // ── Chat send flow ────────────────────────────────
  let sending = false;
  async function send() {
    const text = chatInput.value.trim();
    if (!text || sending) return;
    sending = true;
    sendBtn.disabled = true;
    chatInput.value = "";
    autoGrow();
    appendMessage({ role: "user", content: text });
    showTyping();
    try {
      const reply = await backend("send_message", { message: text });
      hideTyping();
      appendMessage(reply);
    } catch (err) {
      hideTyping();
      appendMessage({ role: "error", content: typeof err === "string" ? err : (err && err.message) || "Request failed." });
    } finally {
      sending = false;
      sendBtn.disabled = false;
      chatInput.focus();
      refreshStatus();
    }
  }
  sendBtn.addEventListener("click", send);
  chatInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
  });
  function autoGrow() {
    chatInput.style.height = "auto";
    chatInput.style.height = Math.min(chatInput.scrollHeight, 160) + "px";
  }
  chatInput.addEventListener("input", autoGrow);

  $("#clear-btn").addEventListener("click", async () => {
    if (!confirm("Clear the current conversation? (Daily notes and learned memory are kept.)")) return;
    await backend("clear_conversation", {});
    messageList.innerHTML = "";
    messageList.appendChild(emptyState());
    refreshStatus();
  });

  // ── Status / API-key gate ─────────────────────────
  async function refreshStatus() {
    try {
      const s = await backend("get_status", {});
      $("#conn-model").textContent = s.model || "—";
      $("#conn-dot").className = "dot " + (s.api_key_set ? "ok" : "err");
      $("#chat-subtitle").textContent = s.api_key_set
        ? `${s.model} · ${s.message_count} message${s.message_count === 1 ? "" : "s"}`
        : "No API key configured";
      const banner = $("#apikey-banner");
      banner.classList.toggle("hidden", !!s.api_key_set);
      sendBtn.disabled = !s.api_key_set || sending;
      chatInput.disabled = !s.api_key_set;
    } catch (err) {
      $("#conn-dot").className = "dot err";
    }
  }

  // ── Settings ──────────────────────────────────────
  async function loadConfig() {
    try {
      const c = await backend("get_config", {});
      $("#cfg-model").value = c.model || "";
      $("#cfg-api-base").value = c.api_base || "";
      $("#cfg-api-key").value = "";
      $("#cfg-key-hint").textContent = c.api_key_set
        ? `A key is set (${c.api_key_masked}). Leave blank to keep it.`
        : "No key set. Paste one to enable chat.";
      const s = await backend("get_status", {});
      $("#cfg-tools").checked = !!s.tools_enabled;
    } catch (err) { showToast("Failed to load settings", true); }
  }

  $("#settings-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-status");
    status.classList.remove("err");
    status.textContent = "Saving…";
    const key = $("#cfg-api-key").value.trim();
    try {
      // Use snake_case keys matching the Rust command params directly, so the
      // binding is correct independent of Tauri's camelCase conversion.
      await backend("save_config", {
        model: $("#cfg-model").value.trim(),
        api_base: $("#cfg-api-base").value.trim(),
        api_key: key.length ? key : null,
      });
      await backend("set_tools_enabled", { enabled: $("#cfg-tools").checked });
      status.textContent = "Saved ✓";
      showToast("Settings saved");
      await loadConfig();
      refreshStatus();
    } catch (err) {
      status.classList.add("err");
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // ── Boot ──────────────────────────────────────────
  async function boot() {
    messageList.appendChild(emptyState());
    try {
      const history = await backend("get_history", {});
      if (Array.isArray(history) && history.length) {
        messageList.innerHTML = "";
        history.forEach(appendMessage);
      }
    } catch (_) {}
    await refreshStatus();
    // First-run: if no key, route to Settings (task 4.2).
    try {
      const s = await backend("get_status", {});
      if (!s.api_key_set) switchView("settings");
    } catch (_) {}
  }
  boot();
})();
