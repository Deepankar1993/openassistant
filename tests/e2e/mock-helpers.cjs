// Shared mock-backend helper for openAssistant Playwright tests.
//
// Usage:
//   const { installMock } = require("./mock-helpers.cjs");
//   await installMock(page, { apiKeySet: true, initialView: "chat" });
//
// The mock mirrors the exact snake_case contract of the Rust command layer so
// tests exercise real frontend logic without Tauri.

"use strict";

/**
 * Install the mock backend into the page before load.
 *
 * @param {import('@playwright/test').Page} page
 * @param {object} opts
 * @param {boolean}  [opts.apiKeySet=false]       - whether an API key is already saved
 * @param {"chat"|"onboarding"} [opts.initialView] - what get_app_state returns (defaults to apiKeySet ? "chat" : "onboarding")
 * @param {boolean}  [opts.failSend=false]         - make send_message throw
 * @param {"success"|"auth_failure"|"network_error"} [opts.probeResult="success"]
 * @param {boolean}  [opts.pathWritable=true]      - check_path_writable return
 * @param {Function} [opts.onCommand]              - JS source string called with (cmd, args, log)
 */
async function installMock(page, opts = {}) {
  const {
    apiKeySet = false,
    initialView = apiKeySet ? "chat" : "onboarding",
    failSend = false,
    probeResult = "success",
    pathWritable = true,
  } = opts;

  await page.addInitScript(
    ({ apiKeySet, initialView, failSend, probeResult, pathWritable }) => {
      // Track which commands were called and with what args (for assertion)
      window.__MOCK_CALLS__ = [];

      const mockState = {
        config: {
          provider: "openrouter",
          model: "openrouter/owl-alpha",
          api_base: "https://openrouter.ai/api/v1",
          api_key_set: apiKeySet,
          api_key_masked: apiKeySet ? "••••••••abcd" : "",
          user_name: "friend",
          data_dir: "~/.openassistant",
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
          app_version: "0.1.0-test",
        },
        history: [],
        memoryMd:
          "# Long-term Memory\n\nTest memory content for Playwright.",
        todayNote: "# Today\n\n*(test notes)*",
        memoryFiles: [
          ["MEMORY.md", "# Long-term Memory\n\nTest memory content for Playwright."],
          ["memory/2026-06-03.md", "# Today\n\n*(test notes)*"],
        ],
        skills: [
          {
            name: "summarize",
            description: "Summarize a piece of text concisely.",
            category: "productivity",
            is_builtin: true,
            content: "# summarize\n\nSummarize the following text in 3 bullet points.\n",
          },
          {
            name: "code-review",
            description: "Review code for correctness and style.",
            category: "development",
            is_builtin: true,
            content: "# code-review\n\nReview the provided code for bugs, style issues, and improvements.\n",
          },
          {
            name: "daily-plan",
            description: "Help plan your day.",
            category: "productivity",
            is_builtin: true,
            content: "# daily-plan\n\nHelp create a prioritized plan for the day.\n",
          },
        ],
        agents: [],
      };

      window.__MOCK_BACKEND__ = async (cmd, args) => {
        args = args || {};
        // Record every call for assertion
        window.__MOCK_CALLS__.push({ cmd, args: JSON.parse(JSON.stringify(args)) });

        switch (cmd) {
          // ── Onboarding ──────────────────────────────────────────────────────
          case "get_app_state":
            return {
              initial_view: initialView,
              api_key_set: mockState.config.api_key_set,
              user_name: mockState.config.user_name,
              data_dir: mockState.config.data_dir,
            };

          case "probe_connection": {
            if (probeResult === "success") {
              // Also accept any non-empty key in success mode
              if (!args.api_key || !args.api_key.trim()) {
                return {
                  ok: false,
                  latency_ms: 0,
                  error_type: "auth_failure",
                  error_message: "No API key provided.",
                };
              }
              return { ok: true, latency_ms: 42, error_type: null, error_message: null };
            }
            if (probeResult === "auth_failure") {
              return {
                ok: false,
                latency_ms: 0,
                error_type: "auth_failure",
                error_message: "Invalid API key.",
              };
            }
            // network_error
            return {
              ok: false,
              latency_ms: 0,
              error_type: "network_error",
              error_message: "Could not reach endpoint.",
            };
          }

          case "check_path_writable":
            return pathWritable;

          case "pick_data_dir":
            return mockState.config.data_dir + "-picked";

          case "save_onboarding_config": {
            const d = args.dto || args;
            if (d.api_key) {
              mockState.config.api_key_set = true;
              mockState.config.api_key_masked = "••••" + String(d.api_key).slice(-4);
            }
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

          // ── Config ──────────────────────────────────────────────────────────
          case "get_config":
            return { ...mockState.config };

          case "save_config":
            mockState.config.model = args.model || mockState.config.model;
            mockState.config.api_base = args.api_base || mockState.config.api_base;
            if (args.api_key) {
              mockState.config.api_key_set = true;
              mockState.config.api_key_masked =
                "••••••••" + String(args.api_key).slice(-4);
            }
            return null;

          case "save_full_config": {
            const d = args.dto || args;
            Object.assign(mockState.config, {
              provider: d.provider || mockState.config.provider,
              model: d.model || mockState.config.model,
              api_base: d.api_base || mockState.config.api_base,
              user_name: d.user_name || mockState.config.user_name,
              log_level: d.log_level || mockState.config.log_level,
              tools_enabled:
                typeof d.tools_enabled === "boolean"
                  ? d.tools_enabled
                  : mockState.config.tools_enabled,
              memory_max_entries:
                d.memory_max_entries !== undefined
                  ? d.memory_max_entries
                  : mockState.config.memory_max_entries,
              memory_fts_enabled:
                typeof d.memory_fts_enabled === "boolean"
                  ? d.memory_fts_enabled
                  : mockState.config.memory_fts_enabled,
              skills_dirs: Array.isArray(d.skills_dirs)
                ? d.skills_dirs
                : mockState.config.skills_dirs,
              skills_auto_create:
                typeof d.skills_auto_create === "boolean"
                  ? d.skills_auto_create
                  : mockState.config.skills_auto_create,
              dm_pairing:
                typeof d.dm_pairing === "boolean"
                  ? d.dm_pairing
                  : mockState.config.dm_pairing,
              vision_provider: d.vision_provider || mockState.config.vision_provider,
              vision_gemini_path:
                d.vision_gemini_path || mockState.config.vision_gemini_path,
            });
            if (d.api_key) {
              mockState.config.api_key_set = true;
              mockState.config.api_key_masked =
                "••••" + String(d.api_key).slice(-4);
            }
            if (d.discord_token) {
              mockState.config.discord_token_set = true;
              mockState.config.discord_token_masked =
                "••••" + String(d.discord_token).slice(-4);
            }
            if (d.telegram_token) {
              mockState.config.telegram_token_set = true;
              mockState.config.telegram_token_masked =
                "••••" + String(d.telegram_token).slice(-4);
            }
            if (d.slack_token) {
              mockState.config.slack_token_set = true;
              mockState.config.slack_token_masked =
                "••••" + String(d.slack_token).slice(-4);
            }
            return null;
          }

          case "set_tools_enabled":
            mockState.config.tools_enabled = !!args.enabled;
            return null;

          // ── Chat / Status ────────────────────────────────────────────────────
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
            if (failSend) throw "LLM request failed: HTTP 401 — invalid key";
            if (!mockState.config.api_key_set) throw "No API key configured.";
            const u = {
              id: "u" + mockState.history.length,
              role: "user",
              content: args.message,
              timestamp: "",
              metadata: null,
            };
            const a = {
              id: "a" + mockState.history.length,
              role: "assistant",
              content: "Echo: " + args.message,
              timestamp: "",
              metadata: null,
            };
            mockState.history.push(u, a);
            return a;
          }

          // ── Memory ────────────────────────────────────────────────────────
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
            return mockState.memoryFiles.filter(
              ([n, e]) =>
                n.toLowerCase().includes(q) || e.toLowerCase().includes(q)
            );
          }

          // ── Skills ────────────────────────────────────────────────────────
          case "list_skills":
            return mockState.skills.map(
              ({ name, description, category, is_builtin }) => ({
                name,
                description,
                category,
                is_builtin,
              })
            );

          case "read_skill": {
            const sk = mockState.skills.find((s) => s.name === args.name);
            if (!sk) throw `Skill \`${args.name}\` not found.`;
            return sk.content;
          }

          case "create_skill": {
            const name = (args.name || "").trim();
            if (!name) throw "Skill name cannot be empty.";
            if (mockState.skills.find((s) => s.name === name))
              throw `Skill \`${name}\` already exists.`;
            mockState.skills.push({
              name,
              description: "(custom)",
              category: "custom",
              is_builtin: false,
              content: args.content || "",
            });
            return null;
          }

          // ── System ────────────────────────────────────────────────────────
          case "run_doctor":
            return [
              {
                name: "Config",
                ok: true,
                message: "Loaded successfully",
                is_optional: false,
              },
              {
                name: "Memory database",
                ok: true,
                message: "SQLite + FTS5 OK",
                is_optional: false,
              },
              {
                name: "Memory workspace",
                ok: true,
                message: "Files initialized",
                is_optional: false,
              },
              {
                name: "Skills",
                ok: true,
                message: "3 built-in skills loaded",
                is_optional: false,
              },
              {
                name: "Gateway",
                ok: false,
                message: "No gateway tokens configured",
                is_optional: true,
              },
              {
                name: "Vision (Gemini CLI)",
                ok: false,
                message: "Not found — image analysis unavailable",
                is_optional: true,
              },
            ];

          case "list_agents":
            return mockState.agents.slice();
          case "get_persona":
            return {
              name: "openAssistant", emoji: "🦞", tone: "friendly", language: "English",
              personality: "You are a helpful, honest, and harmless AI assistant.",
              principles: ["Always be honest"], boundaries: ["Will not pretend to be human"],
              capabilities: ["Memory search and management"],
            };
          case "save_persona":
            return null;

          default:
            return null;
        }
      };
    },
    { apiKeySet, initialView, failSend, probeResult, pathWritable }
  );
}

module.exports = { installMock };
