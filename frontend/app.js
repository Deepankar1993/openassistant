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

  // ── Default mock state ────────────────────────────
  const mockState = {
    config: {
      provider: "openrouter",
      model: "openrouter/owl-alpha",
      api_base: "https://openrouter.ai/api/v1",
      api_key_set: false,
      api_key_masked: "",
      user_name: "friend",
      data_dir: (navigator.platform || "").toLowerCase().includes("win")
        ? "%USERPROFILE%\\.openassistant"
        : "~/.openassistant",
      log_level: "info",
      tools_enabled: false,
      memory_max_entries: 200,
      memory_fts_enabled: true,
      memory_db_path: "~/.openassistant/memory.db",
      skills_dirs: [],
      skills_auto_create: true,
      discord_token_masked: "",
      discord_token_set: false,
      telegram_token_masked: "",
      telegram_token_set: false,
      slack_token_masked: "",
      slack_token_set: false,
      dm_pairing: false,
      vision_provider: "gemini_cli",
      vision_gemini_path: "gemini",
      app_version: "0.1.0",
    },
    history: [],
    memoryMd: "# Long-term Memory\n\nNothing yet. Chat with your assistant to build up memory.",
    todayNote: "# Today\n\n*(no notes yet)*",
    memoryFiles: [
      ["MEMORY.md", "# Long-term Memory\n\nNothing yet…"],
      ["memory/2026-06-03.md", "# Today\n\n*(no notes yet)*"],
    ],
    skills: [
      { name: "summarize", description: "Summarize a piece of text concisely.", category: "productivity", is_builtin: true, content: "# summarize\n\nSummarize the following text in 3 bullet points.\n" },
      { name: "code-review", description: "Review code for correctness and style.", category: "development", is_builtin: true, content: "# code-review\n\nReview the provided code for bugs, style issues, and improvements.\n" },
      { name: "daily-plan", description: "Help plan your day.", category: "productivity", is_builtin: true, content: "# daily-plan\n\nHelp create a prioritized plan for the day.\n" },
    ],
    agents: [],
    persona: {
      name: "openAssistant",
      emoji: "🦞",
      tone: "friendly",
      language: "English",
      personality: "You are a helpful, honest, and harmless AI assistant.",
      principles: ["Be genuinely helpful, not performatively helpful", "Always be honest — never make things up"],
      boundaries: ["Will not pretend to be human"],
      capabilities: ["File reading and writing", "Memory search and management"],
    },
  };

  async function defaultMock(cmd, args) {
    switch (cmd) {
      // ── Onboarding ──
      case "get_app_state":
        return {
          initial_view: mockState.config.api_key_set ? "chat" : "onboarding",
          api_key_set: mockState.config.api_key_set,
          user_name: mockState.config.user_name,
          data_dir: mockState.config.data_dir,
        };
      case "probe_connection":
        if (!args.api_key || !args.api_key.trim()) {
          return { ok: false, latency_ms: 0, error_type: "auth_failure", error_message: "No API key provided." };
        }
        return { ok: true, latency_ms: 42, error_type: null, error_message: null };
      case "check_path_writable":
        return true;
      case "pick_data_dir":
        return mockState.config.data_dir + "-picked";
      case "save_onboarding_config": {
        const d = args.dto || args;
        if (d.api_key) { mockState.config.api_key_set = true; mockState.config.api_key_masked = "••••" + String(d.api_key).slice(-4); }
        if (d.model) mockState.config.model = d.model;
        if (d.api_base) mockState.config.api_base = d.api_base;
        if (d.provider) mockState.config.provider = d.provider;
        if (d.data_dir) mockState.config.data_dir = d.data_dir;
        mockState.config.tools_enabled = !!d.tools_enabled;
        if (d.user_name) mockState.config.user_name = d.user_name;
        return null;
      }
      case "open_external_url":
        return null;

      // ── Config ──
      case "get_config":
        return { ...mockState.config };
      case "save_config":
        mockState.config.model = args.model || mockState.config.model;
        mockState.config.api_base = args.api_base || mockState.config.api_base;
        if (args.api_key) { mockState.config.api_key_set = true; mockState.config.api_key_masked = "••••••••" + String(args.api_key).slice(-4); }
        return null;
      case "save_full_config": {
        const d = args.dto || args;
        Object.assign(mockState.config, {
          provider: d.provider || mockState.config.provider,
          model: d.model || mockState.config.model,
          api_base: d.api_base || mockState.config.api_base,
          user_name: d.user_name || mockState.config.user_name,
          log_level: d.log_level || mockState.config.log_level,
          tools_enabled: typeof d.tools_enabled === "boolean" ? d.tools_enabled : mockState.config.tools_enabled,
          memory_max_entries: d.memory_max_entries !== undefined ? d.memory_max_entries : mockState.config.memory_max_entries,
          memory_fts_enabled: typeof d.memory_fts_enabled === "boolean" ? d.memory_fts_enabled : mockState.config.memory_fts_enabled,
          skills_dirs: Array.isArray(d.skills_dirs) ? d.skills_dirs : mockState.config.skills_dirs,
          skills_auto_create: typeof d.skills_auto_create === "boolean" ? d.skills_auto_create : mockState.config.skills_auto_create,
          dm_pairing: typeof d.dm_pairing === "boolean" ? d.dm_pairing : mockState.config.dm_pairing,
          vision_provider: d.vision_provider || mockState.config.vision_provider,
          vision_gemini_path: d.vision_gemini_path || mockState.config.vision_gemini_path,
        });
        if (d.api_key) { mockState.config.api_key_set = true; mockState.config.api_key_masked = "••••" + String(d.api_key).slice(-4); }
        if (d.discord_token) { mockState.config.discord_token_set = true; mockState.config.discord_token_masked = "••••" + String(d.discord_token).slice(-4); }
        if (d.telegram_token) { mockState.config.telegram_token_set = true; mockState.config.telegram_token_masked = "••••" + String(d.telegram_token).slice(-4); }
        if (d.slack_token) { mockState.config.slack_token_set = true; mockState.config.slack_token_masked = "••••" + String(d.slack_token).slice(-4); }
        return null;
      }
      case "set_tools_enabled":
        mockState.config.tools_enabled = !!args.enabled;
        return null;

      // ── Chat / Status ──
      case "get_status":
        return {
          model: mockState.config.model,
          provider: mockState.config.provider,
          mode: mockState.config.tools_enabled ? "tools" : "chat",
          workspace: "(mock)",
          data_dir: mockState.config.data_dir,
          message_count: mockState.history.length,
          tools_enabled: mockState.config.tools_enabled,
          api_key_set: mockState.config.api_key_set,
          memory_db_entries: 12,
          memory_md_chars: mockState.memoryMd.length,
        };
      case "get_history":
        return mockState.history.slice();
      case "clear_conversation":
        mockState.history = [];
        return null;
      case "send_message": {
        if (!mockState.config.api_key_set) throw "No API key configured.";
        const user = { id: String(Math.random()), role: "user", content: args.message, timestamp: new Date().toISOString(), metadata: null };
        const asst = { id: String(Math.random()), role: "assistant", content: "(mock reply) " + args.message, timestamp: new Date().toISOString(), metadata: null };
        mockState.history.push(user, asst);
        return asst;
      }

      // ── Memory ──
      case "get_memory_md":
        return mockState.memoryMd;
      case "write_memory_md":
        mockState.memoryMd = args.content || "";
        mockState.memoryFiles[0][1] = (args.content || "").slice(0, 80);
        return null;
      case "get_today_note":
        return mockState.todayNote;
      case "search_memory_files": {
        const q = (args.query || "").toLowerCase();
        if (!q) return mockState.memoryFiles.slice();
        return mockState.memoryFiles.filter(([n, e]) => n.toLowerCase().includes(q) || e.toLowerCase().includes(q));
      }

      // ── Skills ──
      case "list_skills":
        return mockState.skills.map(({ name, description, category, is_builtin }) => ({ name, description, category, is_builtin }));
      case "read_skill": {
        const sk = mockState.skills.find(s => s.name === args.name);
        if (!sk) throw `Skill \`${args.name}\` not found.`;
        return sk.content;
      }
      case "create_skill": {
        const name = (args.name || "").trim();
        if (!name) throw "Skill name cannot be empty.";
        if (mockState.skills.find(s => s.name === name)) throw `Skill \`${name}\` already exists.`;
        mockState.skills.push({ name, description: "(custom)", category: "custom", is_builtin: false, content: args.content || "" });
        return null;
      }

      // ── System ──
      case "run_doctor":
        return [
          { name: "Config", ok: true, message: "Loaded successfully", is_optional: false },
          { name: "Memory database", ok: true, message: "SQLite + FTS5 OK", is_optional: false },
          { name: "Memory workspace", ok: true, message: "Files initialized", is_optional: false },
          { name: "Skills", ok: true, message: "3 built-in skills loaded", is_optional: false },
          { name: "Gateway", ok: false, message: "No gateway tokens configured", is_optional: true },
          { name: "Vision (Gemini CLI)", ok: false, message: "Not found — image analysis unavailable", is_optional: true },
        ];
      case "list_agents":
        return mockState.agents.slice();
      case "get_persona":
        return { ...mockState.persona };
      case "save_persona": {
        const d = args.dto || args;
        Object.assign(mockState.persona, d);
        return null;
      }

      default:
        return null;
    }
  }

  // ── DOM helpers ───────────────────────────────────
  const $ = (sel) => document.querySelector(sel);
  const $$ = (sel) => document.querySelectorAll(sel);
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
  const views = ["chat", "settings", "memory", "status", "skills"];
  function switchView(name) {
    if (!views.includes(name)) return;
    views.forEach((v) => $("#view-" + v).classList.toggle("hidden", v !== name));
    $$(".nav-item").forEach((b) =>
      b.classList.toggle("active", b.dataset.view === name));
    if (name === "settings") { loadConfig(); loadPersona(); }
    if (name === "chat") refreshStatus();
    if (name === "memory") loadMemoryView();
    if (name === "skills") loadSkillsView();
    if (name === "status") loadStatusView();
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

  // Category navigation
  $$(".settings-nav-item").forEach((btn) => {
    btn.addEventListener("click", () => {
      $$(".settings-nav-item").forEach(b => b.classList.remove("active"));
      $$(".settings-section").forEach(s => s.classList.add("hidden"));
      btn.classList.add("active");
      const sec = $("#settings-section-" + btn.dataset.section);
      if (sec) sec.classList.remove("hidden");
    });
  });

  // Skills dir chips state
  let skillsDirsList = [];
  function renderSkillsDirsChips() {
    const container = $("#cfg-skills-dirs");
    container.innerHTML = "";
    skillsDirsList.forEach((dir, idx) => {
      const chip = document.createElement("span");
      chip.className = "chip";
      chip.textContent = dir;
      const rm = document.createElement("button");
      rm.className = "chip-remove";
      rm.textContent = "×";
      rm.setAttribute("aria-label", "Remove " + dir);
      rm.addEventListener("click", () => { skillsDirsList.splice(idx, 1); renderSkillsDirsChips(); });
      chip.appendChild(rm);
      container.appendChild(chip);
    });
  }
  $("#skills-dir-add-btn").addEventListener("click", () => {
    const input = $("#cfg-skills-dir-add");
    const val = input.value.trim();
    if (val && !skillsDirsList.includes(val)) { skillsDirsList.push(val); renderSkillsDirsChips(); }
    input.value = "";
  });

  // Show/hide toggle for cfg-api-key in settings
  $("#cfg-api-key-toggle").addEventListener("click", () => {
    const inp = $("#cfg-api-key");
    const isPass = inp.type === "password";
    inp.type = isPass ? "text" : "password";
    $("#cfg-api-key-toggle").textContent = isPass ? "Hide" : "Show";
  });

  // Show/hide toggles for channel tokens
  $$(".token-toggle").forEach(btn => {
    btn.addEventListener("click", () => {
      const inp = $("#" + btn.dataset.for);
      if (!inp) return;
      const isPass = inp.type === "password";
      inp.type = isPass ? "text" : "password";
      btn.textContent = isPass ? "Hide" : "Show";
    });
  });

  // Test connection from settings model section
  let settingsConnTested = false;
  async function settingsTestConnection() {
    const key = $("#cfg-api-key").value.trim();
    const base = $("#cfg-api-base").value.trim();
    const model = $("#cfg-model").value.trim();
    const resultEl = $("#settings-conn-result");
    const btn = $("#settings-test-btn");
    btn.disabled = true;
    btn.textContent = "Testing…";
    resultEl.className = "conn-result";
    resultEl.classList.remove("hidden");
    resultEl.textContent = "Testing…";
    try {
      const r = await backend("probe_connection", { api_key: key, api_base: base, model });
      if (r.ok) {
        resultEl.className = "conn-result ok";
        resultEl.textContent = `Connected! Responded in ${r.latency_ms}ms.`;
        settingsConnTested = true;
      } else if (r.error_type === "auth_failure") {
        resultEl.className = "conn-result fail";
        resultEl.textContent = "Invalid API key. Check the value and try again.";
        settingsConnTested = false;
      } else if (r.error_type === "model_unavailable") {
        resultEl.className = "conn-result warn";
        resultEl.textContent = "Model not found. Try a different model name, or your key may not have access.";
        settingsConnTested = false;
      } else {
        resultEl.className = "conn-result warn";
        resultEl.textContent = "Could not reach the endpoint. Check the URL and your network.";
        settingsConnTested = false;
      }
    } catch (err) {
      resultEl.className = "conn-result fail";
      resultEl.textContent = "Connection test failed.";
      settingsConnTested = false;
    } finally {
      btn.disabled = false;
      btn.textContent = "Test connection";
    }
  }
  $("#settings-test-btn").addEventListener("click", settingsTestConnection);
  // Invalidate test if fields change
  ["cfg-api-key", "cfg-api-base", "cfg-model"].forEach(id => {
    const el = $("#" + id);
    if (el) el.addEventListener("input", () => {
      settingsConnTested = false;
      const resultEl = $("#settings-conn-result");
      if (resultEl) resultEl.classList.add("hidden");
    });
  });

  // Open folder for data dir
  $("#settings-open-folder").addEventListener("click", async () => {
    const dir = $("#cfg-data-dir").textContent || "";
    if (dir) {
      try {
        await backend("open_external_url", { url: "file://" + dir });
      } catch (_) {}
    }
  });

  async function loadConfig() {
    try {
      const c = await backend("get_config", {});
      // Model section
      const providerEl = $("#cfg-provider");
      if (providerEl) providerEl.value = c.provider || "openrouter";
      $("#cfg-model").value = c.model || "";
      $("#cfg-api-base").value = c.api_base || "";
      $("#cfg-api-key").value = "";
      $("#cfg-key-hint").textContent = c.api_key_set
        ? `A key is set (${c.api_key_masked}). Leave blank to keep it.`
        : "No key set. Paste one to enable chat.";
      // General
      const userNameEl = $("#cfg-user-name");
      if (userNameEl) userNameEl.value = c.user_name || "";
      const dataDirEl = $("#cfg-data-dir");
      if (dataDirEl) dataDirEl.textContent = c.data_dir || "";
      const logEl = $("#cfg-log-level");
      if (logEl) logEl.value = c.log_level || "info";
      // Tools
      const toolsEl = $("#cfg-tools");
      if (toolsEl) toolsEl.checked = !!c.tools_enabled;
      // Memory
      const memMaxEl = $("#cfg-memory-max");
      if (memMaxEl) memMaxEl.value = c.memory_max_entries !== undefined ? c.memory_max_entries : 200;
      const memFtsEl = $("#cfg-memory-fts");
      if (memFtsEl) memFtsEl.checked = !!c.memory_fts_enabled;
      const memDbEl = $("#cfg-memory-db-path");
      if (memDbEl) memDbEl.textContent = c.memory_db_path || "";
      // Skills
      skillsDirsList = Array.isArray(c.skills_dirs) ? c.skills_dirs.slice() : [];
      renderSkillsDirsChips();
      const autoEl = $("#cfg-skills-auto-create");
      if (autoEl) autoEl.checked = !!c.skills_auto_create;
      // Channels
      const discEl = $("#cfg-discord-token");
      if (discEl) { discEl.value = ""; $("#discord-token-hint").textContent = c.discord_token_set ? `Set (${c.discord_token_masked}). Leave blank to keep.` : "Not set."; }
      const telEl = $("#cfg-telegram-token");
      if (telEl) { telEl.value = ""; $("#telegram-token-hint").textContent = c.telegram_token_set ? `Set (${c.telegram_token_masked}). Leave blank to keep.` : "Not set."; }
      const slackEl = $("#cfg-slack-token");
      if (slackEl) { slackEl.value = ""; $("#slack-token-hint").textContent = c.slack_token_set ? `Set (${c.slack_token_masked}). Leave blank to keep.` : "Not set."; }
      const pairingEl = $("#cfg-dm-pairing");
      if (pairingEl) pairingEl.checked = !!c.dm_pairing;
      // Advanced
      const visionEl = $("#cfg-vision-provider");
      if (visionEl) visionEl.value = c.vision_provider || "gemini_cli";
      const geminiEl = $("#cfg-gemini-path");
      if (geminiEl) geminiEl.value = c.vision_gemini_path || "gemini";
      const appVerEl = $("#settings-app-version");
      if (appVerEl) appVerEl.textContent = c.app_version || "—";
    } catch (err) { showToast("Failed to load settings", true); }
  }

  // Helper: read current full config, merge overrides, save
  async function saveFullConfigWith(overrides) {
    const c = await backend("get_config", {});
    const dto = {
      provider: c.provider,
      model: c.model,
      api_base: c.api_base,
      api_key: null,
      user_name: c.user_name,
      log_level: c.log_level,
      tools_enabled: c.tools_enabled,
      memory_max_entries: c.memory_max_entries,
      memory_fts_enabled: c.memory_fts_enabled,
      skills_dirs: c.skills_dirs,
      skills_auto_create: c.skills_auto_create,
      discord_token: null,
      telegram_token: null,
      slack_token: null,
      dm_pairing: c.dm_pairing,
      vision_provider: c.vision_provider,
      vision_gemini_path: c.vision_gemini_path,
      ...overrides,
    };
    await backend("save_full_config", { dto });
  }

  // Model section form (keep existing data-testid "settings-save" working)
  $("#settings-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-status");
    status.classList.remove("err");
    status.textContent = "Saving…";
    const key = $("#cfg-api-key").value.trim();
    try {
      await saveFullConfigWith({
        provider: $("#cfg-provider").value,
        model: $("#cfg-model").value.trim(),
        api_base: $("#cfg-api-base").value.trim(),
        api_key: key.length ? key : null,
      });
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

  // General section form
  $("#settings-general-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-general-status");
    status.textContent = "Saving…"; status.className = "save-status";
    try {
      await saveFullConfigWith({
        user_name: $("#cfg-user-name").value.trim() || "friend",
        log_level: $("#cfg-log-level").value,
      });
      status.textContent = "Saved ✓";
      showToast("General settings saved");
    } catch (err) {
      status.className = "save-status err";
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // Persona section
  async function loadPersona() {
    try {
      const p = await backend("get_persona", {});
      if (!p) return;
      $("#persona-name").value = p.name || "";
      $("#persona-emoji").value = p.emoji || "";
      $("#persona-tone").value = p.tone || "friendly";
      $("#persona-language").value = p.language || "";
      $("#persona-personality").value = p.personality || "";
      $("#persona-principles").value = (p.principles || []).join("\n");
      $("#persona-boundaries").value = (p.boundaries || []).join("\n");
      $("#persona-capabilities").value = (p.capabilities || []).join("\n");
    } catch (err) {
      showToast("Failed to load persona", true);
    }
  }
  const linesOf = (id) => $(id).value.split("\n").map((s) => s.trim()).filter(Boolean);
  $("#settings-persona-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-persona-status");
    status.textContent = "Saving…"; status.className = "save-status";
    try {
      await backend("save_persona", { dto: {
        name: $("#persona-name").value.trim() || "openAssistant",
        emoji: $("#persona-emoji").value.trim(),
        tone: $("#persona-tone").value,
        language: $("#persona-language").value.trim() || "English",
        personality: $("#persona-personality").value.trim(),
        principles: linesOf("#persona-principles"),
        boundaries: linesOf("#persona-boundaries"),
        capabilities: linesOf("#persona-capabilities"),
      } });
      status.textContent = "Saved ✓";
      showToast("Persona saved");
    } catch (err) {
      status.className = "save-status err";
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // Tools section form
  $("#settings-tools-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-tools-status");
    status.textContent = "Saving…"; status.className = "save-status";
    try {
      await saveFullConfigWith({ tools_enabled: $("#cfg-tools").checked });
      await backend("set_tools_enabled", { enabled: $("#cfg-tools").checked });
      status.textContent = "Saved ✓";
      showToast("Tools setting saved");
      refreshStatus();
    } catch (err) {
      status.className = "save-status err";
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // Memory section form
  $("#settings-memory-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-memory-status");
    status.textContent = "Saving…"; status.className = "save-status";
    try {
      await saveFullConfigWith({
        memory_max_entries: parseInt($("#cfg-memory-max").value, 10) || 200,
        memory_fts_enabled: $("#cfg-memory-fts").checked,
      });
      status.textContent = "Saved ✓";
      showToast("Memory settings saved");
    } catch (err) {
      status.className = "save-status err";
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // Skills section form
  $("#settings-skills-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-skills-status");
    status.textContent = "Saving…"; status.className = "save-status";
    try {
      await saveFullConfigWith({
        skills_dirs: skillsDirsList.slice(),
        skills_auto_create: $("#cfg-skills-auto-create").checked,
      });
      status.textContent = "Saved ✓";
      showToast("Skills settings saved");
    } catch (err) {
      status.className = "save-status err";
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // Channels section form
  $("#settings-channels-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-channels-status");
    status.textContent = "Saving…"; status.className = "save-status";
    const discordVal = $("#cfg-discord-token").value.trim();
    const telegramVal = $("#cfg-telegram-token").value.trim();
    const slackVal = $("#cfg-slack-token").value.trim();
    try {
      await saveFullConfigWith({
        discord_token: discordVal || null,
        telegram_token: telegramVal || null,
        slack_token: slackVal || null,
        dm_pairing: $("#cfg-dm-pairing").checked,
      });
      status.textContent = "Saved ✓";
      showToast("Channel settings saved");
      await loadConfig();
    } catch (err) {
      status.className = "save-status err";
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // Advanced section form
  $("#settings-advanced-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const status = $("#save-advanced-status");
    status.textContent = "Saving…"; status.className = "save-status";
    try {
      await saveFullConfigWith({
        vision_provider: $("#cfg-vision-provider").value,
        vision_gemini_path: $("#cfg-gemini-path").value.trim() || "gemini",
      });
      status.textContent = "Saved ✓";
      showToast("Advanced settings saved");
    } catch (err) {
      status.className = "save-status err";
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // Re-run wizard button in settings Advanced section
  $("#settings-rerun-wizard-btn").addEventListener("click", () => openWizard(true));

  // ── Memory view ───────────────────────────────────
  let memoryCurrentFile = null;
  let memoryIsEditable = false;

  async function loadMemoryView() {
    try {
      const files = await backend("search_memory_files", { query: "" });
      renderMemoryList(files);
      // Hydrate both MEMORY.md and today note in parallel, show MEMORY.md first
      const [mdContent, todayContent] = await Promise.all([
        backend("get_memory_md", {}),
        backend("get_today_note", {}),
      ]);
      // Store locally for quick access when user clicks a file
      memoryCurrentFile = "MEMORY.md";
      memoryIsEditable = true;
      showMemoryContent("MEMORY.md", mdContent, true);
    } catch (err) {
      showToast("Failed to load memory", true);
    }
  }

  function renderMemoryList(files) {
    const list = $("#memory-list");
    list.innerHTML = "";
    files.forEach(([filename, excerpt]) => {
      const li = document.createElement("li");
      li.className = "file-item";
      li.dataset.filename = filename;
      if (filename === memoryCurrentFile) li.classList.add("active");
      const nameSpan = document.createElement("span");
      nameSpan.className = "file-item-name";
      nameSpan.textContent = filename;
      const excerptSpan = document.createElement("span");
      excerptSpan.className = "file-item-excerpt";
      excerptSpan.textContent = excerpt.slice(0, 60);
      li.appendChild(nameSpan);
      li.appendChild(excerptSpan);
      li.addEventListener("click", () => selectMemoryFile(filename));
      list.appendChild(li);
    });
  }

  async function selectMemoryFile(filename) {
    memoryCurrentFile = filename;
    const isToday = filename !== "MEMORY.md";
    memoryIsEditable = !isToday;
    $$("#memory-list .file-item").forEach(li => li.classList.toggle("active", li.dataset.filename === filename));
    try {
      let content;
      if (filename === "MEMORY.md") {
        content = await backend("get_memory_md", {});
      } else {
        // Daily notes: use get_today_note for today's, otherwise search
        content = await backend("get_today_note", {});
      }
      showMemoryContent(filename, content, memoryIsEditable);
    } catch (err) {
      showToast("Failed to load file", true);
    }
  }

  function showMemoryContent(filename, content, editable) {
    const ta = $("#memory-content");
    const toolbar = $("#memory-toolbar");
    const label = $("#memory-file-label");
    const saveBtn = $("#memory-save-btn");
    ta.value = content;
    ta.readOnly = !editable;
    ta.style.background = editable ? "var(--bg-primary)" : "var(--bg-secondary)";
    ta.style.color = editable ? "var(--text-primary)" : "var(--text-secondary)";
    label.textContent = filename;
    toolbar.classList.remove("hidden");
    saveBtn.classList.toggle("hidden", !editable);
  }

  $("#memory-save-btn").addEventListener("click", async () => {
    const ta = $("#memory-content");
    try {
      await backend("write_memory_md", { content: ta.value });
      showToast("Memory saved");
    } catch (err) {
      showToast("Failed to save memory", true);
    }
  });

  // Search box with 300ms debounce
  let memorySearchTimer = null;
  $("#memory-search").addEventListener("input", (e) => {
    clearTimeout(memorySearchTimer);
    memorySearchTimer = setTimeout(async () => {
      try {
        const files = await backend("search_memory_files", { query: e.target.value });
        renderMemoryList(files);
      } catch (_) {}
    }, 300);
  });

  // ── Skills view ───────────────────────────────────
  let skillsCurrentName = null;

  async function loadSkillsView() {
    try {
      const skills = await backend("list_skills", {});
      renderSkillsList(skills);
    } catch (err) {
      showToast("Failed to load skills", true);
    }
  }

  function renderSkillsList(skills) {
    const list = $("#skills-list");
    list.innerHTML = "";
    const builtins = skills.filter(s => s.is_builtin);
    const customs = skills.filter(s => !s.is_builtin);

    function addSection(label, items) {
      if (!items.length) return;
      const hdr = document.createElement("li");
      hdr.className = "file-section-label";
      hdr.textContent = label;
      list.appendChild(hdr);
      items.forEach(s => {
        const li = document.createElement("li");
        li.className = "file-item" + (s.name === skillsCurrentName ? " active" : "");
        li.dataset.name = s.name;
        const nameSpan = document.createElement("span");
        nameSpan.className = "file-item-name";
        nameSpan.textContent = s.name;
        const descSpan = document.createElement("span");
        descSpan.className = "file-item-excerpt";
        descSpan.textContent = s.description;
        li.appendChild(nameSpan);
        li.appendChild(descSpan);
        li.addEventListener("click", () => selectSkill(s.name));
        list.appendChild(li);
      });
    }
    addSection("Built-in", builtins);
    addSection("Custom", customs);
  }

  async function selectSkill(name) {
    skillsCurrentName = name;
    $$("#skills-list .file-item").forEach(li => li.classList.toggle("active", li.dataset.name === name));
    try {
      const [skills, content] = await Promise.all([
        backend("list_skills", {}),
        backend("read_skill", { name }),
      ]);
      const skill = skills.find(s => s.name === name);
      const contentEl = $("#skills-content");
      contentEl.innerHTML = "";
      const nameEl = document.createElement("div");
      nameEl.className = "skill-detail-name";
      nameEl.textContent = name;
      const metaEl = document.createElement("div");
      metaEl.className = "skill-detail-meta";
      metaEl.textContent = `${skill ? skill.category : "custom"} · ${skill && skill.is_builtin ? "Built-in" : "Custom"}`;
      const preEl = document.createElement("pre");
      preEl.className = "skill-detail-content";
      preEl.textContent = content;
      const noteEl = document.createElement("div");
      noteEl.className = "skill-activation-note";
      noteEl.textContent = "Skill activation affects agent behavior and will be available in a future update.";
      contentEl.appendChild(nameEl);
      contentEl.appendChild(metaEl);
      contentEl.appendChild(preEl);
      contentEl.appendChild(noteEl);
    } catch (err) {
      showToast("Failed to load skill", true);
    }
  }

  // New skill modal
  $("#skills-new-btn").addEventListener("click", () => {
    $("#skill-modal-name").value = "";
    $("#skill-modal-content").value = "";
    $("#skill-modal").classList.remove("hidden");
    setTimeout(() => $("#skill-modal-name").focus(), 50);
  });
  $("#skill-modal-cancel").addEventListener("click", () => $("#skill-modal").classList.add("hidden"));
  $("#skill-modal-save").addEventListener("click", async () => {
    const name = $("#skill-modal-name").value.trim();
    const content = $("#skill-modal-content").value;
    if (!name) { showToast("Enter a skill name", true); return; }
    try {
      await backend("create_skill", { name, content });
      $("#skill-modal").classList.add("hidden");
      showToast("Skill created");
      loadSkillsView();
    } catch (err) {
      showToast(typeof err === "string" ? err : "Failed to create skill", true);
    }
  });

  // ── Status / Doctor view ──────────────────────────
  async function loadStatusView() {
    try {
      const s = await backend("get_status", {});
      renderStatusCard(s);
      // Load agents
      const agents = await backend("list_agents", {});
      renderAgentsList(agents);
    } catch (err) {
      showToast("Failed to load status", true);
    }
  }

  function renderStatusCard(s) {
    const grid = $("#status-grid");
    const rows = [
      ["Model", s.model || "—"],
      ["Provider", s.provider || "—"],
      ["Mode", s.mode || "—"],
      ["Data directory", s.data_dir || "—"],
      ["Messages", String(s.message_count || 0)],
      ["Memory DB entries", String(s.memory_db_entries || 0)],
      ["Memory MD chars", String(s.memory_md_chars || 0)],
      ["Tools enabled", s.tools_enabled ? "Yes" : "No"],
    ];
    grid.innerHTML = "";
    rows.forEach(([k, v]) => {
      const keyEl = document.createElement("span");
      keyEl.className = "status-key";
      keyEl.textContent = k;
      const valEl = document.createElement("span");
      valEl.className = "status-val";
      valEl.textContent = v;
      grid.appendChild(keyEl);
      grid.appendChild(valEl);
    });

    const wrap = $("#status-rerun-wizard-wrap");
    if (wrap) wrap.classList.toggle("hidden", !!s.api_key_set);
  }

  function renderAgentsList(agents) {
    const container = $("#agents-list");
    container.innerHTML = "";
    if (!agents.length) {
      const hint = document.createElement("div");
      hint.className = "hint";
      hint.textContent = "No agent definitions found in your data directory.";
      container.appendChild(hint);
      return;
    }
    agents.forEach(a => {
      const row = document.createElement("div");
      row.className = "agent-row";
      const nameEl = document.createElement("div");
      nameEl.className = "agent-name";
      nameEl.textContent = a.name;
      row.appendChild(nameEl);
      const descEl = document.createElement("div");
      descEl.className = "agent-desc";
      descEl.textContent = a.description;
      row.appendChild(descEl);
      if (a.tools && a.tools.length) {
        const toolsEl = document.createElement("div");
        toolsEl.className = "agent-tools";
        toolsEl.textContent = "Tools: " + a.tools.join(", ");
        row.appendChild(toolsEl);
      }
      if (a.model) {
        const modelEl = document.createElement("div");
        modelEl.className = "agent-tools";
        modelEl.textContent = "Model: " + a.model;
        row.appendChild(modelEl);
      }
      container.appendChild(row);
    });
  }

  $("#doctor-run-btn").addEventListener("click", async () => {
    const btn = $("#doctor-run-btn");
    const results = $("#doctor-results");
    btn.disabled = true;
    btn.textContent = "Running…";
    results.innerHTML = '<div class="hint">Running diagnostics…</div>';
    try {
      const rows = await backend("run_doctor", {});
      results.innerHTML = "";
      rows.forEach(r => {
        const cls = r.ok ? "ok" : r.is_optional ? "warn" : "fail";
        const icon = r.ok ? "✅" : r.is_optional ? "⚠️" : "❌";
        const row = document.createElement("div");
        row.className = "doctor-row " + cls;
        const iconEl = document.createElement("span");
        iconEl.className = "doctor-icon";
        iconEl.textContent = icon;
        const nameEl = document.createElement("span");
        nameEl.className = "doctor-name";
        nameEl.textContent = r.name;
        const msgEl = document.createElement("span");
        msgEl.className = "doctor-msg";
        msgEl.textContent = r.message;
        row.appendChild(iconEl);
        row.appendChild(nameEl);
        row.appendChild(msgEl);
        results.appendChild(row);
      });
    } catch (err) {
      results.innerHTML = "";
      const errEl = document.createElement("div");
      errEl.className = "hint";
      errEl.style.color = "var(--danger)";
      errEl.textContent = "Diagnostics failed.";
      results.appendChild(errEl);
    } finally {
      btn.disabled = false;
      btn.textContent = "Run Diagnostics";
    }
  });

  // Status rerun wizard button
  document.addEventListener("click", (e) => {
    if (e.target.closest("#status-rerun-wizard-btn")) openWizard(false);
  });

  // ── Onboarding Wizard ─────────────────────────────

  // Wizard state
  const wiz = {
    step: 1,
    dataDir: "",
    provider: "openrouter",
    apiBase: "https://openrouter.ai/api/v1",
    model: "openrouter/owl-alpha",
    apiKey: "",
    toolsEnabled: null,  // null = no choice yet
    userName: "",
    connTested: false,
    writableChecked: false,
  };

  const PROVIDER_DEFAULTS = {
    openrouter: { api_base: "https://openrouter.ai/api/v1", model: "openrouter/owl-alpha", link: "https://openrouter.ai/keys" },
    openai:     { api_base: "https://api.openai.com/v1",     model: "gpt-4o",               link: "https://platform.openai.com/api-keys" },
    custom:     { api_base: "",                              model: "",                      link: "" },
  };

  function wizUpdateStepBar() {
    $$(".wizard-step").forEach(el => {
      const s = parseInt(el.dataset.step, 10);
      el.classList.toggle("done", s < wiz.step);
      el.classList.toggle("current", s === wiz.step);
    });
    $$(".wizard-step-line").forEach((line, idx) => {
      line.classList.toggle("done", idx + 1 < wiz.step);
    });
  }

  function wizShowScreen(n) {
    for (let i = 1; i <= 4; i++) {
      $("#wizard-screen-" + i).classList.toggle("hidden", i !== n);
    }
    wiz.step = n;
    wizUpdateStepBar();
    const backBtn = $("#wizard-back-btn");
    backBtn.style.visibility = n === 1 ? "hidden" : "visible";
    // Relabel continue button on last screen
    const contBtn = $("#wizard-continue-btn");
    if (n === 4) {
      contBtn.textContent = "Start chatting →";
      contBtn.setAttribute("data-testid", "onboard-finish-cta");
    } else {
      contBtn.textContent = "Continue";
      contBtn.removeAttribute("data-testid");
    }
    wizUpdateContinue();
    if (n === 4) wizRunScreen4Pills();
  }

  function wizUpdateContinue() {
    const btn = $("#wizard-continue-btn");
    let enabled = true;
    if (wiz.step === 1) enabled = wiz.writableChecked;
    if (wiz.step === 2) enabled = wiz.connTested;
    if (wiz.step === 3) enabled = wiz.toolsEnabled !== null;
    btn.disabled = !enabled;
  }

  // Screen 1 – Workspace
  async function wizCheckWritable(path) {
    const badge = $("#onboard-workspace-writable");
    badge.className = "writable-badge";
    badge.classList.remove("hidden");
    badge.textContent = "Checking…";
    wiz.writableChecked = false;
    wizUpdateContinue();
    try {
      const ok = await backend("check_path_writable", { path });
      wiz.writableChecked = !!ok;
      badge.className = "writable-badge " + (ok ? "ok" : "fail");
      badge.textContent = ok ? "✓ Writable" : "✗ Cannot write — choose another folder";
    } catch (_) {
      badge.className = "writable-badge fail";
      badge.textContent = "✗ Check failed";
    }
    wizUpdateContinue();
  }

  $("#onboard-workspace-change").addEventListener("click", async () => {
    try {
      const picked = await backend("pick_data_dir", {});
      if (picked) {
        wiz.dataDir = picked;
        $("#onboard-workspace-path").textContent = picked;
        wizCheckWritable(picked);
      }
    } catch (_) {}
  });

  // Screen 2 – Provider
  $$("#onboard-provider-cards .provider-card").forEach(card => {
    card.addEventListener("click", () => {
      $$("#onboard-provider-cards .provider-card").forEach(c => c.classList.remove("selected"));
      card.classList.add("selected");
      card.querySelector("input[type=radio]").checked = true;
      const prov = card.dataset.provider;
      wiz.provider = prov;
      const def = PROVIDER_DEFAULTS[prov] || PROVIDER_DEFAULTS.custom;
      wiz.apiBase = def.api_base;
      wiz.model = def.model;
      $("#onboard-api-base").value = def.api_base;
      const baseWrap = $("#onboard-api-base-wrap");
      baseWrap.style.display = prov === "custom" ? "flex" : "flex"; // always show
      $("#onboard-model").value = def.model;
      // Update key link
      const link = $("#onboard-get-key-link");
      if (link) {
        link.href = def.link || "#";
        link.style.display = def.link ? "inline-block" : "none";
      }
      // Invalidate connection test
      wiz.connTested = false;
      const resultEl = $("#onboard-connection-result");
      if (resultEl) resultEl.classList.add("hidden");
      wizUpdateContinue();
    });
  });

  // Invalidate test on field change
  ["onboard-api-key", "onboard-api-base", "onboard-model"].forEach(id => {
    const el = $("#" + id);
    if (el) el.addEventListener("input", () => {
      wiz.connTested = false;
      const resultEl = $("#onboard-connection-result");
      if (resultEl) resultEl.classList.add("hidden");
      wizUpdateContinue();
    });
  });

  // "Get an API key" link — call open_external_url instead of navigating webview
  $("#onboard-get-key-link").addEventListener("click", async (e) => {
    e.preventDefault();
    const url = e.currentTarget.href;
    if (url && url !== "#") {
      try { await backend("open_external_url", { url }); } catch (_) {}
    }
  });

  // Show/hide API key
  $("#onboard-api-key-toggle").addEventListener("click", () => {
    const inp = $("#onboard-api-key");
    const isPass = inp.type === "password";
    inp.type = isPass ? "text" : "password";
    $("#onboard-api-key-toggle").textContent = isPass ? "Hide" : "Show";
  });

  // Test connection
  $("#onboard-test-connection").addEventListener("click", async () => {
    const btn = $("#onboard-test-connection");
    const resultEl = $("#onboard-connection-result");
    const key = $("#onboard-api-key").value.trim();
    const base = $("#onboard-api-base").value.trim() || wiz.apiBase;
    const model = $("#onboard-model").value.trim() || wiz.model;
    btn.disabled = true;
    btn.textContent = "Testing…";
    resultEl.className = "conn-result";
    resultEl.classList.remove("hidden");
    resultEl.textContent = "Testing…";
    try {
      const r = await backend("probe_connection", { api_key: key, api_base: base, model });
      if (r.ok) {
        resultEl.className = "conn-result ok";
        resultEl.textContent = `Connected! Responded in ${r.latency_ms}ms.`;
        wiz.connTested = true;
        wiz.apiKey = key;
        wiz.apiBase = base;
        wiz.model = model;
      } else if (r.error_type === "auth_failure") {
        resultEl.className = "conn-result fail";
        resultEl.textContent = "Invalid API key. Check the value and try again.";
        wiz.connTested = false;
      } else if (r.error_type === "model_unavailable") {
        resultEl.className = "conn-result warn";
        resultEl.textContent = "Model not found. Try a different model name, or your key may not have access.";
        wiz.connTested = false;
      } else {
        resultEl.className = "conn-result warn";
        resultEl.textContent = "Could not reach the endpoint. Check the URL and your network.";
        wiz.connTested = false;
      }
    } catch (_) {
      resultEl.className = "conn-result fail";
      resultEl.textContent = "Connection test failed.";
      wiz.connTested = false;
    } finally {
      btn.disabled = false;
      btn.textContent = "Test connection";
      wizUpdateContinue();
    }
  });

  // Screen 3 – Tools
  ["onboard-tools-card-a", "onboard-tools-card-b"].forEach(id => {
    const card = $("#" + id);
    card.addEventListener("click", () => {
      $$(".tools-card").forEach(c => { c.classList.remove("selected"); c.setAttribute("aria-checked", "false"); });
      card.classList.add("selected");
      card.setAttribute("aria-checked", "true");
      wiz.toolsEnabled = id === "onboard-tools-card-b";
      const consent = $("#onboard-tools-consent");
      consent.classList.toggle("hidden", !wiz.toolsEnabled);
      wizUpdateContinue();
    });
    card.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); card.click(); } });
  });

  // Screen 4 – Pills
  async function wizRunScreen4Pills() {
    // Pill 1: Config saved — verify api_key_set from get_app_state
    const pillConfig = $("#onboard-pill-config");
    const pillDatadir = $("#onboard-pill-datadir");
    const pillVision = $("#onboard-pill-vision");

    pillConfig.className = "status-pill pending";
    pillConfig.innerHTML = '<span class="pill-icon">⏳</span> Config saved';
    pillDatadir.className = "status-pill pending";
    pillDatadir.innerHTML = '<span class="pill-icon">⏳</span> Data directory ready';
    pillVision.className = "status-pill pending";
    pillVision.innerHTML = '<span class="pill-icon">⏳</span> Vision tools';

    // Pill 1: verify connection test passed (config is not yet saved at this point)
    if (wiz.connTested && wiz.apiKey) {
      pillConfig.className = "status-pill ok";
      pillConfig.innerHTML = '<span class="pill-icon">✓</span> Config ready';
    } else {
      pillConfig.className = "status-pill warn";
      pillConfig.innerHTML = '<span class="pill-icon">⚠️</span> Config pending';
    }

    // Pill 2: data dir was validated in screen 1
    if (wiz.writableChecked) {
      pillDatadir.className = "status-pill ok";
      pillDatadir.innerHTML = '<span class="pill-icon">✓</span> Data directory ready';
    } else {
      pillDatadir.className = "status-pill warn";
      pillDatadir.innerHTML = '<span class="pill-icon">⚠️</span> Data directory not verified';
    }

    // Pill 3: run_doctor filtered to vision — 200ms before showing amber
    const visionTimer = setTimeout(() => {
      if (pillVision.className.includes("pending")) {
        pillVision.className = "status-pill warn";
        pillVision.innerHTML = '<span class="pill-icon">⏳</span> Checking…';
      }
    }, 200);
    try {
      const rows = await backend("run_doctor", {});
      clearTimeout(visionTimer);
      const visionRow = rows.find(r => r.name && r.name.toLowerCase().includes("vision"));
      if (visionRow) {
        if (visionRow.ok) {
          pillVision.className = "status-pill ok";
          pillVision.innerHTML = '<span class="pill-icon">✓</span> Gemini CLI detected';
        } else {
          pillVision.className = "status-pill warn";
          pillVision.innerHTML = '<span class="pill-icon">⚠️</span> Not found — image analysis unavailable';
        }
      } else {
        pillVision.className = "status-pill warn";
        pillVision.innerHTML = '<span class="pill-icon">⚠️</span> Vision status unknown';
      }
    } catch (_) {
      clearTimeout(visionTimer);
      pillVision.className = "status-pill warn";
      pillVision.innerHTML = '<span class="pill-icon">⚠️</span> Check failed';
    }
  }

  // Skip name button
  $("#onboard-skip-name").addEventListener("click", () => {
    $("#onboard-username").value = "";
    wiz.userName = "";
    wizFinish();
  });

  // Wizard back/continue buttons
  $("#wizard-back-btn").addEventListener("click", () => {
    if (wiz.step > 1) wizShowScreen(wiz.step - 1);
  });
  $("#wizard-continue-btn").addEventListener("click", () => {
    if (wiz.step < 4) {
      // Save current screen values before advancing
      if (wiz.step === 2) {
        wiz.apiBase = $("#onboard-api-base").value.trim() || wiz.apiBase;
        wiz.model = $("#onboard-model").value.trim() || wiz.model;
        wiz.apiKey = $("#onboard-api-key").value.trim() || wiz.apiKey;
      }
      wizShowScreen(wiz.step + 1);
    } else {
      wiz.userName = ($("#onboard-username").value || "").trim();
      wizFinish();
    }
  });

  async function wizFinish() {
    const finishBtn = $("#wizard-continue-btn");
    finishBtn.disabled = true;
    finishBtn.textContent = "Saving…";
    try {
      const dto = {
        data_dir: wiz.dataDir,
        provider: wiz.provider,
        model: wiz.model,
        api_base: wiz.apiBase,
        api_key: wiz.apiKey,
        tools_enabled: !!wiz.toolsEnabled,
        user_name: wiz.userName || null,
        skills_dirs: [wiz.dataDir + "/skills"].filter(Boolean),
      };
      await backend("save_onboarding_config", { dto });
      closeWizard();
      switchView("chat");
      refreshStatus();
    } catch (err) {
      showToast(typeof err === "string" ? err : "Failed to save configuration", true);
      finishBtn.disabled = false;
      finishBtn.textContent = "Start chatting →";
    }
  }

  async function openWizard(reEntry) {
    // Reset wizard state
    wiz.step = 1;
    wiz.toolsEnabled = null;
    wiz.connTested = false;
    wiz.writableChecked = false;

    // Fetch current state
    try {
      const appState = await backend("get_app_state", {});
      wiz.dataDir = appState.data_dir || "";
      $("#onboard-workspace-path").textContent = wiz.dataDir;
      wiz.userName = appState.user_name || "";
      $("#onboard-username").value = wiz.userName;
    } catch (_) {
      wiz.dataDir = "~/.openassistant";
      $("#onboard-workspace-path").textContent = wiz.dataDir;
    }

    // For re-entry from settings: prefill from config
    if (reEntry) {
      try {
        const cfg = await backend("get_config", {});
        wiz.provider = cfg.provider || "openrouter";
        wiz.apiBase = cfg.api_base || "";
        wiz.model = cfg.model || "";
        // Show masked key; user must re-enter to change
        $("#onboard-api-key").value = cfg.api_key_masked || "";
        $("#onboard-api-key").type = "password";
        $("#onboard-api-base").value = wiz.apiBase;
        $("#onboard-model").value = wiz.model;
        // Pre-select provider card
        $$("#onboard-provider-cards .provider-card").forEach(c => {
          const sel = c.dataset.provider === wiz.provider;
          c.classList.toggle("selected", sel);
          c.querySelector("input[type=radio]").checked = sel;
        });
        // Invalidate test for re-entry
        wiz.connTested = false;
        const resultEl = $("#onboard-connection-result");
        if (resultEl) resultEl.classList.add("hidden");
      } catch (_) {}
    } else {
      // Fresh entry defaults
      wiz.provider = "openrouter";
      wiz.apiBase = PROVIDER_DEFAULTS.openrouter.api_base;
      wiz.model = PROVIDER_DEFAULTS.openrouter.model;
      $("#onboard-api-key").value = "";
      $("#onboard-api-base").value = wiz.apiBase;
      $("#onboard-model").value = wiz.model;
      $$("#onboard-provider-cards .provider-card").forEach(c => {
        c.classList.toggle("selected", c.dataset.provider === "openrouter");
        c.querySelector("input[type=radio]").checked = c.dataset.provider === "openrouter";
      });
    }

    // Update get-key link
    const def = PROVIDER_DEFAULTS[wiz.provider] || PROVIDER_DEFAULTS.openrouter;
    const link = $("#onboard-get-key-link");
    if (link) { link.href = def.link || "#"; link.style.display = def.link ? "inline-block" : "none"; }

    // Reset tools cards
    $$(".tools-card").forEach(c => { c.classList.remove("selected"); c.setAttribute("aria-checked", "false"); });
    wiz.toolsEnabled = null;
    $("#onboard-tools-consent").classList.add("hidden");

    // Reset connection result badge
    $("#onboard-connection-result").classList.add("hidden");
    $("#onboard-workspace-writable").classList.add("hidden");

    // Kick off writability check for current path
    wizCheckWritable(wiz.dataDir);

    wizShowScreen(1);
    $("#wizard-overlay").classList.remove("hidden");
  }

  function closeWizard() {
    $("#wizard-overlay").classList.add("hidden");
  }

  // ── Utility ───────────────────────────────────────
  function escHtml(str) {
    return String(str)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

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

    // Route based on get_app_state instead of raw status check
    try {
      const appState = await backend("get_app_state", {});
      if (appState.initial_view === "onboarding") {
        openWizard(false);
      } else {
        await refreshStatus();
      }
    } catch (_) {
      // Fallback: if get_app_state fails, check status the old way
      try {
        const s = await backend("get_status", {});
        if (!s.api_key_set) openWizard(false);
        else await refreshStatus();
      } catch (_) {}
    }
  }
  boot();
})();
