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
      webhook_host: "0.0.0.0",
      webhook_port: 3000,
      discord_allowed_users: [],
      dm_policy: "pairing",
      dm_pairing: false,
      vision_provider: "gemini_cli",
      vision_gemini_path: "gemini",
      app_version: "0.1.0",
    },
    history: [],
    // Persisted conversations (most recent first is computed at read time).
    // Each: { id, title, updated_at, messages:[Message] }
    conversations: [],
    convSeq: 0,
    activeConvId: null,
    gatewayRunning: false,
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

  // Derive a short title for a conversation from its first user message.
  function mockConvTitle(messages) {
    const firstUser = messages.find((m) => m.role === "user");
    const raw = (firstUser && firstUser.content) || "New conversation";
    return raw.length > 48 ? raw.slice(0, 48) + "…" : raw;
  }
  // Persist the current history into mockState.conversations (if non-empty),
  // returning the persisted conversation's id (or null). Updates in place when
  // the active conversation already exists.
  function mockPersistCurrent() {
    if (!mockState.history.length) return null;
    let conv = mockState.conversations.find((c) => c.id === mockState.activeConvId);
    if (!conv) {
      conv = { id: "conv-" + (++mockState.convSeq), title: "", updated_at: "", messages: [] };
      mockState.conversations.push(conv);
      // Adopt as the active conversation so subsequent turns update it in place
      // rather than spawning duplicates.
      mockState.activeConvId = conv.id;
    }
    conv.messages = mockState.history.slice();
    conv.title = mockConvTitle(conv.messages);
    conv.updated_at = new Date().toISOString();
    return conv.id;
  }

  async function defaultMock(cmd, args) {
    switch (cmd) {
      // ── Conversations ──
      case "list_conversations":
        // newest first
        return mockState.conversations
          .slice()
          .sort((a, b) => (b.updated_at || "").localeCompare(a.updated_at || ""))
          .map((c) => ({
            id: c.id,
            title: c.title || mockConvTitle(c.messages),
            updated_at: c.updated_at,
            message_count: c.messages.length,
          }));
      case "new_conversation":
        mockPersistCurrent();
        mockState.history = [];
        mockState.activeConvId = null;
        return null;
      case "switch_conversation": {
        // Persist the current (possibly unsaved) conversation first.
        mockPersistCurrent();
        const conv = mockState.conversations.find((c) => c.id === args.id);
        if (!conv) return [];
        mockState.history = conv.messages.slice();
        mockState.activeConvId = conv.id;
        return conv.messages.slice();
      }
      case "delete_conversation":
        mockState.conversations = mockState.conversations.filter((c) => c.id !== args.id);
        if (mockState.activeConvId === args.id) {
          mockState.activeConvId = null;
          mockState.history = [];
        }
        return null;

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
          webhook_host: d.webhook_host !== undefined ? d.webhook_host : mockState.config.webhook_host,
          webhook_port: d.webhook_port !== undefined ? d.webhook_port : mockState.config.webhook_port,
          discord_allowed_users: Array.isArray(d.discord_allowed_users) ? d.discord_allowed_users : mockState.config.discord_allowed_users,
          dm_policy: d.dm_policy || mockState.config.dm_policy,
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
        // Now persists the current conversation (if any) then resets.
        mockPersistCurrent();
        mockState.history = [];
        mockState.activeConvId = null;
        return null;
      case "send_message": {
        if (!mockState.config.api_key_set) throw "No API key configured.";
        const user = { id: String(Math.random()), role: "user", content: args.message, timestamp: new Date().toISOString(), metadata: null };
        const asst = { id: String(Math.random()), role: "assistant", content: "(mock reply) " + args.message, timestamp: new Date().toISOString(), metadata: null };
        mockState.history.push(user, asst);
        // Lazily persist so the conversation appears in list_conversations after
        // its first completed turn (mirrors the real backend).
        mockPersistCurrent();
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
      case "gateway_readiness": {
        const c = mockState.config;
        const host = c.webhook_host || "0.0.0.0";
        const port = c.webhook_port || 3000;
        const gateOpen = (c.discord_allowed_users && c.discord_allowed_users.length) || c.dm_policy === "open";
        return [
          { name: "API key", ok: !!c.api_key_set, required: true, detail: c.api_key_set ? "Model API key is set." : "No API key — set it in Settings → Model." },
          { name: "WebChat server", ok: true, required: false, detail: `Will listen on http://${host}:${port}.` },
          { name: "Discord", ok: !!c.discord_token_set && gateOpen, required: false, detail: !c.discord_token_set ? "Not configured (optional)." : (gateOpen ? "Ready. Enable MESSAGE CONTENT intent in the Developer Portal." : "Token set but no allowlist and dm_policy isn't 'open' — bot ignores everyone.") },
          { name: "Telegram", ok: !!c.telegram_token_set, required: false, detail: c.telegram_token_set ? "Ready." : "Not configured (optional)." },
          { name: "Slack", ok: false, required: false, detail: "Needs slack_token + signing secret + a public URL." },
          { name: "How to run", ok: true, required: false, detail: "Start with `openassistant gateway` (or `cargo run -- gateway` if not on PATH)." },
        ];
      }
      case "gateway_status":
        return { running: !!mockState.gatewayRunning, address: mockState.gatewayRunning ? "http://0.0.0.0:3000" : null };
      case "gateway_start":
        mockState.gatewayRunning = true;
        return "http://0.0.0.0:3000";
      case "gateway_stop":
        mockState.gatewayRunning = false;
        return null;
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
  // Persona identity shown on assistant turns and the empty state.
  let personaName = "openAssistant";
  const PERSONA_TAGLINE = "Your local, private AI companion.";
  const STARTER_PROMPTS = [
    "Summarize what you remember about me",
    "Help me plan my day",
    "What can you do?",
  ];

  function updatePersonaLabels() {
    $$(".msg-author:not(.error-label)").forEach((el) => { el.textContent = personaName; });
    const en = messageList.querySelector(".empty-name");
    if (en) en.textContent = personaName;
  }

  function emptyState() {
    const d = document.createElement("div");
    d.className = "empty-state";
    const name = document.createElement("div");
    name.className = "empty-name";
    name.textContent = personaName;
    const tag = document.createElement("div");
    tag.className = "empty-tagline";
    tag.textContent = PERSONA_TAGLINE;
    const chips = document.createElement("div");
    chips.className = "starter-chips";
    STARTER_PROMPTS.forEach((prompt) => {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "starter-chip";
      chip.textContent = prompt;
      chip.addEventListener("click", () => {
        if (chatInput.disabled) return;
        chatInput.value = prompt;
        autoGrow();
        send();
      });
      chips.appendChild(chip);
    });
    d.appendChild(name);
    d.appendChild(tag);
    d.appendChild(chips);
    return d;
  }

  function authorLabel(text, isError) {
    const el = document.createElement("div");
    el.className = "msg-author" + (isError ? " error-label" : "");
    el.textContent = text;
    return el;
  }

  function renderMessage(msg) {
    const role = msg.role === "user" ? "user" : msg.role === "error" ? "error" : "assistant";
    const wrap = document.createElement("div");
    wrap.className = "msg " + role;
    wrap.dataset.testid = "message-" + role;
    const bubble = document.createElement("div");
    bubble.className = "bubble";
    bubble.dataset.testid = "message-bubble";
    if (role === "assistant") {
      wrap.appendChild(authorLabel(personaName, false));
      if (window.OAMarkdown) {
        OAMarkdown.render(bubble, msg.content);
        OAMarkdown.enhance(bubble);
      } else {
        bubble.textContent = msg.content;
      }
    } else if (role === "error") {
      wrap.appendChild(authorLabel("Error", true));
      bubble.textContent = msg.content;
    } else {
      bubble.textContent = msg.content;
    }
    wrap.appendChild(bubble);
    return wrap;
  }

  // Auto-scroll: follow the bottom unless the user has scrolled up; a
  // "jump to bottom" pill resumes following.
  let followBottom = true;
  const jumpBtn = $("#jump-bottom");
  function scrollToBottom(force) {
    if (force) followBottom = true;
    if (followBottom) {
      messageList.scrollTop = messageList.scrollHeight;
      if (jumpBtn) jumpBtn.classList.add("hidden");
    }
  }
  messageList.addEventListener("scroll", () => {
    const nearBottom =
      messageList.scrollHeight - messageList.scrollTop - messageList.clientHeight < 48;
    followBottom = nearBottom;
    if (jumpBtn) jumpBtn.classList.toggle("hidden", nearBottom);
  });
  if (jumpBtn) jumpBtn.addEventListener("click", () => scrollToBottom(true));

  function appendMessage(msg) {
    const es = messageList.querySelector(".empty-state");
    if (es) es.remove();
    messageList.appendChild(renderMessage(msg));
    scrollToBottom(true);
  }
  function showTyping() {
    const wrap = document.createElement("div");
    wrap.className = "msg assistant";
    wrap.id = "typing-indicator";
    wrap.dataset.testid = "typing-indicator";
    wrap.appendChild(authorLabel(personaName, false));
    const bubble = document.createElement("div");
    bubble.className = "bubble";
    bubble.innerHTML = '<span class="dots"><span></span><span></span><span></span></span>'; // static markup
    wrap.appendChild(bubble);
    messageList.appendChild(wrap);
    scrollToBottom(true);
  }
  function hideTyping() { const t = $("#typing-indicator"); if (t) t.remove(); }

  // Open links from rendered markdown externally instead of navigating the webview.
  messageList.addEventListener("click", async (e) => {
    const a = e.target.closest && e.target.closest("a[href]");
    if (!a || !messageList.contains(a)) return;
    e.preventDefault();
    try { await backend("open_external_url", { url: a.href }); } catch (_) {}
  });

  // ── Streaming chat (Tauri only) ───────────────────
  // When running inside Tauri with the event API available, chat goes through
  // `send_message_stream` and live `chat-event` window events. In a plain
  // browser (mock/Playwright pathway) the classic `send_message` flow is used.
  const streamingSupported = !!(
    realInvoke && tauri.event && typeof tauri.event.listen === "function"
  );

  let streamSeq = 0;       // generation counter for in-flight streams
  let activeSendId = 0;    // which send currently owns the composer
  let stream = null;       // active stream context, or null

  function errText(err) {
    return typeof err === "string" ? err : (err && err.message) || "Request failed.";
  }

  function setComposerStreaming(on) {
    sendBtn.classList.toggle("stop", on);
    sendBtn.textContent = on ? "Stop" : "Send";
    sendBtn.setAttribute("aria-label", on ? "Stop" : "Send");
    if (on) sendBtn.disabled = false;
  }

  function createStreamContext(id) {
    const es = messageList.querySelector(".empty-state");
    if (es) es.remove();
    const wrap = document.createElement("div");
    wrap.className = "msg assistant streaming";
    wrap.dataset.testid = "message-assistant";
    wrap.appendChild(authorLabel(personaName, false));
    const body = document.createElement("div");
    body.className = "bubble";
    body.dataset.testid = "message-bubble";
    const cursor = document.createElement("span");
    cursor.className = "stream-cursor";
    cursor.textContent = "▍";
    body.appendChild(cursor);
    wrap.appendChild(body);
    messageList.appendChild(wrap);
    const ctx = {
      id, wrap, body, cursor,
      segEl: null, segText: "",
      steps: [], raf: 0,
      stopped: false, finalized: false,
    };
    newStreamSegment(ctx);
    scrollToBottom(true);
    return ctx;
  }

  function newStreamSegment(ctx) {
    const seg = document.createElement("div");
    seg.className = "stream-text";
    ctx.body.insertBefore(seg, ctx.cursor);
    ctx.segEl = seg;
    ctx.segText = "";
  }

  function renderStreamSegment(ctx) {
    if (window.OAMarkdown) OAMarkdown.render(ctx.segEl, ctx.segText);
    else ctx.segEl.textContent = ctx.segText;
  }

  // rAF-throttled re-render of the active text segment while tokens stream.
  function scheduleStreamRender() {
    if (!stream || stream.raf) return;
    stream.raf = requestAnimationFrame(() => {
      if (!stream) return;
      stream.raf = 0;
      renderStreamSegment(stream);
      scrollToBottom(false);
    });
  }

  function addToolStep(p) {
    const ctx = stream;
    renderStreamSegment(ctx); // flush text accumulated before the tool ran
    const det = document.createElement("details");
    det.className = "tool-step running";
    det.dataset.testid = "tool-step";
    det.open = false;
    const sum = document.createElement("summary");
    const icon = document.createElement("span");
    icon.className = "tool-step-icon";
    icon.innerHTML = window.OAIcons ? OAIcons.spinner : ""; // static markup
    const nm = document.createElement("span");
    nm.className = "tool-step-name";
    nm.textContent = p.name || "tool";
    const sm = document.createElement("span");
    sm.className = "tool-step-detail";
    sm.textContent = p.summary || "";
    sum.appendChild(icon);
    sum.appendChild(nm);
    sum.appendChild(sm);
    det.appendChild(sum);
    const preview = document.createElement("pre");
    preview.className = "tool-step-preview";
    det.appendChild(preview);
    ctx.body.insertBefore(det, ctx.cursor);
    ctx.steps.push(det);
    newStreamSegment(ctx); // text after the tool goes into a fresh segment
    scrollToBottom(false);
  }

  function endToolStep(p) {
    const ctx = stream;
    const det =
      ctx.steps.slice().reverse().find(
        (d) =>
          d.classList.contains("running") &&
          (!p.name || d.querySelector(".tool-step-name").textContent === p.name)
      ) || ctx.steps.slice().reverse().find((d) => d.classList.contains("running"));
    if (!det) return;
    det.classList.remove("running");
    det.classList.add(p.ok ? "ok" : "fail");
    const icon = det.querySelector(".tool-step-icon");
    if (icon) icon.innerHTML = window.OAIcons ? (p.ok ? OAIcons.check : OAIcons.cross) : ""; // static markup
    const preview = det.querySelector(".tool-step-preview");
    if (preview) preview.textContent = p.preview || "";
    det.open = !p.ok; // collapsed when ok, open when not
    scrollToBottom(false);
  }

  // Finalize the in-flight message. When finalText is a string, it replaces
  // all streamed text segments (tool step rows are kept); otherwise the
  // buffered text is kept as-is.
  function finalizeStream(finalText) {
    const s = stream;
    if (!s || s.finalized) return;
    s.finalized = true;
    if (s.raf) { cancelAnimationFrame(s.raf); s.raf = 0; }
    s.cursor.remove();
    if (typeof finalText === "string" && finalText.length) {
      s.body.querySelectorAll(".stream-text").forEach((n) => n.remove());
      const el = document.createElement("div");
      el.className = "stream-text";
      if (window.OAMarkdown) OAMarkdown.render(el, finalText);
      else el.textContent = finalText;
      s.body.appendChild(el);
    } else {
      renderStreamSegment(s);
    }
    // Any step still marked running was interrupted.
    s.steps.forEach((d) => {
      if (d.classList.contains("running")) {
        d.classList.remove("running");
        d.classList.add("fail");
        const icon = d.querySelector(".tool-step-icon");
        if (icon) icon.innerHTML = window.OAIcons ? OAIcons.cross : ""; // static markup
      }
    });
    if (window.OAMarkdown) OAMarkdown.enhance(s.body);
    s.wrap.classList.remove("streaming");
    stream = null;
    scrollToBottom(false);
  }

  function resetComposerAfterStream() {
    activeSendId = 0;
    sending = false;
    setComposerStreaming(false);
    chatInput.focus();
    refreshStatus();
    // A turn just completed: the conversation may have been newly persisted.
    refreshConversations(true);
  }

  // Stop button: stop APPLYING events client-side and finalize the message.
  // (No backend cancel is available; late events/results are ignored.)
  function stopStream() {
    if (!stream) return;
    stream.stopped = true;
    finalizeStream(null);
    resetComposerAfterStream();
  }

  function handleChatEvent(p) {
    if (!p || !stream || stream.stopped || stream.finalized) return;
    switch (p.type) {
      case "token":
        stream.segText += p.text || "";
        scheduleStreamRender();
        break;
      case "tool_start":
        addToolStep(p);
        break;
      case "tool_end":
        endToolStep(p);
        break;
      case "done":
        finalizeStream(typeof p.text === "string" ? p.text : null);
        resetComposerAfterStream();
        break;
      case "error":
        finalizeStream(null);
        appendMessage({ role: "error", content: p.message || "Stream error." });
        resetComposerAfterStream();
        break;
    }
  }

  if (streamingSupported) {
    tauri.event.listen("chat-event", (evt) => handleChatEvent(evt && evt.payload));
  }

  async function sendStreaming(text) {
    const id = ++streamSeq;
    activeSendId = id;
    sending = true;
    setComposerStreaming(true);
    chatInput.value = "";
    autoGrow();
    appendMessage({ role: "user", content: text });
    stream = createStreamContext(id);
    try {
      const reply = await backend("send_message_stream", { message: text });
      if (stream && stream.id === id && !stream.finalized) {
        finalizeStream(reply && typeof reply.content === "string" ? reply.content : null);
      }
    } catch (err) {
      if (activeSendId === id) {
        if (stream && stream.id === id) finalizeStream(null);
        appendMessage({ role: "error", content: errText(err) });
      }
    } finally {
      if (activeSendId === id) resetComposerAfterStream();
    }
  }

  // ── Chat send flow ────────────────────────────────
  let sending = false;
  async function send() {
    const text = chatInput.value.trim();
    if (!text || sending) return;
    if (streamingSupported) { sendStreaming(text); return; }
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
      appendMessage({ role: "error", content: errText(err) });
    } finally {
      sending = false;
      sendBtn.disabled = false;
      chatInput.focus();
      refreshStatus();
      // A turn just completed: the conversation may have been newly persisted.
      refreshConversations(true);
    }
  }
  sendBtn.addEventListener("click", () => {
    if (sendBtn.classList.contains("stop")) { stopStream(); return; }
    send();
  });
  chatInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
  });
  function autoGrow() {
    chatInput.style.height = "auto";
    chatInput.style.height = Math.min(chatInput.scrollHeight, 160) + "px";
  }
  chatInput.addEventListener("input", autoGrow);

  // "New chat" (repurposed clear button): persists+resets the conversation,
  // then starts a fresh empty chat. Keeps data-testid="clear-conversation".
  $("#clear-btn").addEventListener("click", async () => {
    if (!confirm("Start a new chat? The current conversation is saved to your history.")) return;
    try { await backend("clear_conversation", {}); } catch (_) {}
    resetToEmptyChat();
    await refreshConversations();
    refreshStatus();
    if (!chatInput.disabled) chatInput.focus();
  });

  // ── Conversation history sidebar ──────────────────
  // activeConvId tracks the currently-shown stored conversation. It is null for
  // a brand-new, never-persisted chat, and is set to the newest list entry once
  // that chat's first reply has landed (see refreshConversations).
  let activeConvId = null;
  const convList = $("#conv-list");

  // Relative time: "just now", "5m ago", "2h ago", "yesterday", "3d ago",
  // then falls back to a short date.
  function relativeTime(iso) {
    if (!iso) return "";
    const then = new Date(iso).getTime();
    if (isNaN(then)) return "";
    const diff = Date.now() - then;
    const sec = Math.floor(diff / 1000);
    if (sec < 60) return "just now";
    const min = Math.floor(sec / 60);
    if (min < 60) return min + "m ago";
    const hr = Math.floor(min / 60);
    if (hr < 24) return hr + "h ago";
    const day = Math.floor(hr / 24);
    if (day === 1) return "yesterday";
    if (day < 7) return day + "d ago";
    const wk = Math.floor(day / 7);
    if (wk < 5) return wk + "w ago";
    try {
      return new Date(then).toLocaleDateString(undefined, { month: "short", day: "numeric" });
    } catch (_) {
      return day + "d ago";
    }
  }

  function trashIcon() {
    return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><line x1="10" y1="11" x2="10" y2="17"/><line x1="14" y1="11" x2="14" y2="17"/></svg>';
  }

  // Clear the message list back to the empty starter state.
  function resetToEmptyChat() {
    messageList.innerHTML = "";
    messageList.appendChild(emptyState());
    activeConvId = null;
    highlightActiveConv();
  }

  // Render a list of Message objects (from switch_conversation / get_history)
  // into #message-list using the shared render path.
  function renderConversationMessages(messages) {
    messageList.innerHTML = "";
    if (Array.isArray(messages) && messages.length) {
      messages.forEach(appendMessage);
    } else {
      messageList.appendChild(emptyState());
    }
  }

  function highlightActiveConv() {
    convList.querySelectorAll(".conv-item").forEach((li) => {
      const isActive = li.dataset.id === activeConvId;
      li.classList.toggle("active", isActive);
      if (isActive) li.setAttribute("aria-current", "true");
      else li.removeAttribute("aria-current");
    });
  }

  function renderConversationList(convs) {
    convList.innerHTML = "";
    if (!convs.length) {
      const empty = document.createElement("li");
      empty.className = "conv-empty";
      empty.textContent = "No conversations yet. Start chatting to build your history.";
      convList.appendChild(empty);
      return;
    }
    convs.forEach((c) => {
      const li = document.createElement("li");
      li.className = "conv-item";
      li.dataset.id = c.id;
      li.dataset.testid = "conversation-item";
      li.tabIndex = 0;
      if (c.id === activeConvId) { li.classList.add("active"); li.setAttribute("aria-current", "true"); }

      const body = document.createElement("div");
      body.className = "conv-item-body";
      const title = document.createElement("span");
      title.className = "conv-item-title";
      title.textContent = c.title || "Untitled conversation";
      const time = document.createElement("span");
      time.className = "conv-item-time";
      time.textContent = relativeTime(c.updated_at);
      body.appendChild(title);
      body.appendChild(time);

      const del = document.createElement("button");
      del.type = "button";
      del.className = "conv-delete-btn";
      del.dataset.testid = "conversation-delete";
      del.setAttribute("aria-label", "Delete conversation");
      del.innerHTML = trashIcon(); // static markup
      del.addEventListener("click", (e) => {
        e.stopPropagation();
        deleteConversation(c.id);
      });

      li.appendChild(body);
      li.appendChild(del);
      li.addEventListener("click", () => selectConversation(c.id));
      li.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") { e.preventDefault(); selectConversation(c.id); }
      });
      convList.appendChild(li);
    });
  }

  // Re-fetch the conversation list and re-highlight. When the active chat has
  // just been persisted (first reply landed) but activeConvId is still null,
  // adopt the newest entry as active.
  async function refreshConversations(adoptNewest) {
    let convs = [];
    try {
      const res = await backend("list_conversations", {});
      if (Array.isArray(res)) convs = res;
    } catch (_) {
      convs = [];
    }
    if (adoptNewest && activeConvId === null && convs.length) {
      activeConvId = convs[0].id;
    }
    renderConversationList(convs);
    return convs;
  }

  async function selectConversation(id) {
    if (id === activeConvId) return;
    try {
      const messages = await backend("switch_conversation", { id });
      activeConvId = id;
      renderConversationMessages(messages);
      highlightActiveConv();
      scrollToBottom(true);
      refreshStatus();
    } catch (err) {
      showToast(typeof err === "string" ? err : "Failed to open conversation", true);
    }
  }

  async function deleteConversation(id) {
    if (!confirm("Delete this conversation? This cannot be undone.")) return;
    const wasActive = id === activeConvId;
    try {
      await backend("delete_conversation", { id });
    } catch (err) {
      showToast(typeof err === "string" ? err : "Failed to delete conversation", true);
      return;
    }
    if (wasActive) resetToEmptyChat();
    await refreshConversations();
    refreshStatus();
  }

  $("#conv-new-btn").addEventListener("click", async () => {
    try { await backend("new_conversation", {}); } catch (_) {}
    resetToEmptyChat();
    await refreshConversations();
    refreshStatus();
    if (!chatInput.disabled) chatInput.focus();
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
      // While streaming, the button is a Stop button and must stay clickable.
      const isStop = sendBtn.classList.contains("stop");
      sendBtn.disabled = !isStop && (!s.api_key_set || sending);
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
      const hostEl = $("#cfg-webhook-host");
      if (hostEl) hostEl.value = c.webhook_host || "";
      const portEl = $("#cfg-webhook-port");
      if (portEl) portEl.value = (c.webhook_port !== undefined && c.webhook_port !== 0) ? c.webhook_port : "";
      const allowedEl = $("#cfg-discord-allowed");
      if (allowedEl) allowedEl.value = Array.isArray(c.discord_allowed_users) ? c.discord_allowed_users.join(", ") : "";
      const dmPolicyEl = $("#cfg-dm-policy");
      if (dmPolicyEl) dmPolicyEl.value = c.dm_policy || "pairing";
      // Advanced
      const visionEl = $("#cfg-vision-provider");
      if (visionEl) visionEl.value = c.vision_provider || "gemini_cli";
      const geminiEl = $("#cfg-gemini-path");
      if (geminiEl) geminiEl.value = c.vision_gemini_path || "gemini";
      const appVerEl = $("#settings-app-version");
      if (appVerEl) appVerEl.textContent = c.app_version || "—";
      loadGatewayReadiness();
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
      webhook_host: c.webhook_host || "",
      webhook_port: c.webhook_port || 0,
      discord_allowed_users: Array.isArray(c.discord_allowed_users) ? c.discord_allowed_users : [],
      dm_policy: c.dm_policy || "pairing",
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
    const allowed = ($("#cfg-discord-allowed").value || "")
      .split(",").map(s => s.trim()).filter(Boolean);
    try {
      await saveFullConfigWith({
        discord_token: discordVal || null,
        telegram_token: telegramVal || null,
        slack_token: slackVal || null,
        webhook_host: $("#cfg-webhook-host").value.trim(),
        webhook_port: parseInt($("#cfg-webhook-port").value, 10) || 0,
        discord_allowed_users: allowed,
        dm_policy: $("#cfg-dm-policy").value,
        dm_pairing: $("#cfg-dm-pairing").checked,
      });
      status.textContent = "Saved ✓";
      showToast("Channel settings saved");
      await loadConfig();
      loadGatewayReadiness();
    } catch (err) {
      status.className = "save-status err";
      status.textContent = "Save failed";
      showToast(typeof err === "string" ? err : "Save failed", true);
    }
  });

  // Gateway readiness panel (Channels section)
  async function loadGatewayReadiness() {
    const container = $("#gateway-readiness");
    if (!container) return;
    try {
      const rows = await backend("gateway_readiness", {});
      container.innerHTML = "";
      rows.forEach(r => {
        const cls = r.ok ? "ok" : r.required ? "fail" : "warn";
        const row = document.createElement("div");
        row.className = "doctor-row " + cls;
        const iconEl = document.createElement("span");
        iconEl.className = "doctor-icon";
        // static SVG markup from OAIcons
        iconEl.innerHTML = window.OAIcons
          ? (r.ok ? OAIcons.check : r.required ? OAIcons.cross : OAIcons.warn)
          : "";
        const nameEl = document.createElement("span");
        nameEl.className = "doctor-name";
        nameEl.textContent = r.name;
        const msgEl = document.createElement("span");
        msgEl.className = "doctor-msg";
        msgEl.textContent = r.detail;
        row.appendChild(iconEl);
        row.appendChild(nameEl);
        row.appendChild(msgEl);
        container.appendChild(row);
      });
    } catch (_) {
      container.innerHTML = '<div class="hint" style="color:var(--danger)">Could not load readiness.</div>';
    }
    refreshGatewayRunStatus();
  }
  const gwCheckBtn = $("#gateway-check-btn");
  if (gwCheckBtn) gwCheckBtn.addEventListener("click", loadGatewayReadiness);
  const gwCopyBtn = $("#gateway-copy-cmd");
  if (gwCopyBtn) gwCopyBtn.addEventListener("click", async () => {
    const cmd = $("#gateway-run-cmd").value;
    try { await navigator.clipboard.writeText(cmd); showToast("Command copied"); }
    catch (_) { showToast("Copy failed", true); }
  });

  // Start/stop the in-process gateway
  async function refreshGatewayRunStatus() {
    const el = $("#gateway-run-status");
    const startBtn = $("#gateway-start-btn");
    const stopBtn = $("#gateway-stop-btn");
    if (!el) return;
    try {
      const s = await backend("gateway_status", {});
      if (s.running) {
        el.textContent = "● Running" + (s.address ? " · " + s.address : "");
        el.style.color = "var(--success, #10b981)";
        if (startBtn) startBtn.disabled = true;
        if (stopBtn) stopBtn.disabled = false;
      } else {
        el.textContent = "○ Stopped";
        el.style.color = "var(--text-muted)";
        if (startBtn) startBtn.disabled = false;
        if (stopBtn) stopBtn.disabled = true;
      }
    } catch (_) {
      el.textContent = "—";
    }
  }
  const gwStartBtn = $("#gateway-start-btn");
  if (gwStartBtn) gwStartBtn.addEventListener("click", async () => {
    gwStartBtn.disabled = true;
    const el = $("#gateway-run-status");
    if (el) el.textContent = "Starting…";
    try {
      const url = await backend("gateway_start", {});
      showToast("Gateway started" + (url ? " on " + url : ""));
    } catch (err) {
      showToast(typeof err === "string" ? err : "Failed to start gateway", true);
    }
    refreshGatewayRunStatus();
  });
  const gwStopBtn = $("#gateway-stop-btn");
  if (gwStopBtn) gwStopBtn.addEventListener("click", async () => {
    try { await backend("gateway_stop", {}); showToast("Gateway stopped"); }
    catch (_) { showToast("Failed to stop gateway", true); }
    refreshGatewayRunStatus();
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

  // Memory viewer mode: "edit" shows the raw textarea, "preview" shows
  // sanitized rendered markdown. Read-only files default to preview.
  let memoryMode = "edit";
  function setMemoryMode(mode) {
    memoryMode = mode;
    const ta = $("#memory-content");
    const rendered = $("#memory-rendered");
    const btn = $("#memory-mode-btn");
    const preview = mode === "preview";
    if (preview && rendered && window.OAMarkdown) {
      OAMarkdown.render(rendered, ta.value);
      OAMarkdown.enhance(rendered);
    }
    if (rendered) rendered.classList.toggle("hidden", !preview);
    ta.classList.toggle("hidden", preview);
    if (btn) btn.textContent = preview ? (memoryIsEditable ? "Edit" : "Source") : "Preview";
  }
  const memoryModeBtn = $("#memory-mode-btn");
  if (memoryModeBtn) memoryModeBtn.addEventListener("click", () => {
    setMemoryMode(memoryMode === "preview" ? "edit" : "preview");
  });

  function showMemoryContent(filename, content, editable) {
    const ta = $("#memory-content");
    const toolbar = $("#memory-toolbar");
    const label = $("#memory-file-label");
    const saveBtn = $("#memory-save-btn");
    ta.value = content;
    ta.readOnly = !editable;
    label.textContent = filename;
    toolbar.classList.remove("hidden");
    saveBtn.classList.toggle("hidden", !editable);
    // Editable files open in edit mode; read-only notes open rendered.
    setMemoryMode(editable || !window.OAMarkdown || !OAMarkdown.available() ? "edit" : "preview");
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
        const row = document.createElement("div");
        row.className = "doctor-row " + cls;
        const iconEl = document.createElement("span");
        iconEl.className = "doctor-icon";
        // static SVG markup from OAIcons
        iconEl.innerHTML = window.OAIcons
          ? (r.ok ? OAIcons.check : r.is_optional ? OAIcons.warn : OAIcons.cross)
          : "";
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
      // static SVG + static copy
      badge.innerHTML = ok
        ? (window.OAIcons ? OAIcons.check : "") + "<span>Writable</span>"
        : (window.OAIcons ? OAIcons.cross : "") + "<span>Cannot write — choose another folder</span>";
    } catch (_) {
      badge.className = "writable-badge fail";
      badge.innerHTML = (window.OAIcons ? OAIcons.cross : "") + "<span>Check failed</span>"; // static markup
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
  // Sets a pill's state + label using static SVG icons from OAIcons.
  function setPill(pill, state, label) {
    pill.className = "status-pill " + state;
    const icons = window.OAIcons || {};
    const icon =
      state === "ok" ? icons.check :
      state === "warn" ? icons.warn :
      state === "fail" ? icons.cross : icons.clock;
    pill.innerHTML = '<span class="pill-icon">' + (icon || "") + "</span> "; // static markup
    pill.appendChild(document.createTextNode(label));
  }

  async function wizRunScreen4Pills() {
    // Pill 1: Config saved — verify api_key_set from get_app_state
    const pillConfig = $("#onboard-pill-config");
    const pillDatadir = $("#onboard-pill-datadir");
    const pillVision = $("#onboard-pill-vision");

    setPill(pillConfig, "pending", "Config saved");
    setPill(pillDatadir, "pending", "Data directory ready");
    setPill(pillVision, "pending", "Vision tools");

    // Pill 1: verify connection test passed (config is not yet saved at this point)
    if (wiz.connTested && wiz.apiKey) {
      setPill(pillConfig, "ok", "Config ready");
    } else {
      setPill(pillConfig, "warn", "Config pending");
    }

    // Pill 2: data dir was validated in screen 1
    if (wiz.writableChecked) {
      setPill(pillDatadir, "ok", "Data directory ready");
    } else {
      setPill(pillDatadir, "warn", "Data directory not verified");
    }

    // Pill 3: run_doctor filtered to vision — 200ms before showing amber
    const visionTimer = setTimeout(() => {
      if (pillVision.className.includes("pending")) {
        setPill(pillVision, "warn", "Checking…");
      }
    }, 200);
    try {
      const rows = await backend("run_doctor", {});
      clearTimeout(visionTimer);
      const visionRow = rows.find(r => r.name && r.name.toLowerCase().includes("vision"));
      if (visionRow) {
        if (visionRow.ok) {
          setPill(pillVision, "ok", "Gemini CLI detected");
        } else {
          setPill(pillVision, "warn", "Not found — image analysis unavailable");
        }
      } else {
        setPill(pillVision, "warn", "Vision status unknown");
      }
    } catch (_) {
      clearTimeout(visionTimer);
      setPill(pillVision, "warn", "Check failed");
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
    // Load the persona name for assistant turn labels and the empty state.
    backend("get_persona", {})
      .then((p) => {
        if (p && p.name) { personaName = p.name; updatePersonaLabels(); }
      })
      .catch(() => {});
    let hadHistory = false;
    try {
      const history = await backend("get_history", {});
      if (Array.isArray(history) && history.length) {
        messageList.innerHTML = "";
        history.forEach(appendMessage);
        hadHistory = true;
      }
    } catch (_) {}

    // Populate the conversation history sidebar. If the boot history is a
    // persisted conversation, adopt the newest list entry as active.
    try { await refreshConversations(hadHistory); } catch (_) {}

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
