import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const UI_DIR = fileURLToPath(new URL("../ui", import.meta.url));
const DEFAULTS_PATH = fileURLToPath(new URL("../ui/defaults.json", import.meta.url));
const DEFAULTS_RAW = readFileSync(DEFAULTS_PATH, "utf8");
const DEFAULTS_JSON = JSON.parse(DEFAULTS_RAW);

function collectUiJsFiles(dir = UI_DIR, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".") || entry.isSymbolicLink()) continue;
    const abs = join(dir, entry.name);
    if (entry.isDirectory()) {
      collectUiJsFiles(abs, out);
    } else if (entry.isFile() && extname(entry.name) === ".js") {
      out.push({ abs, rel: `ui/${relative(UI_DIR, abs).replaceAll("\\", "/")}` });
    }
  }
  return out.sort((a, b) => a.rel.localeCompare(b.rel));
}

// Import pure leaf modules BEFORE installing any DOM globals, with fetch temporarily blocked,
// to prove zero-DOM and zero-fetch import purity under Node >= 20.
assert.equal(typeof globalThis.window, "undefined", "leaf import check expects no window global");
assert.equal(typeof globalThis.document, "undefined", "leaf import check expects no document global");
assert.equal(typeof globalThis.ResizeObserver, "undefined", "leaf import check expects no ResizeObserver global");
const originalNodeFetch = globalThis.fetch;
globalThis.fetch = async (url) => {
  throw new Error(`Leaf UI module must not call fetch during import: ${String(url)}`);
};
const bandModule = await import("../ui/band.js");
const mockModule = await import("../ui/mock.js");
const i18nModule = await import("../ui/i18n.js");
globalThis.fetch = originalNodeFetch;

function makeElementStub() {
  return {
    innerHTML: "",
    textContent: "",
    hidden: true,
    dataset: {},
    style: {},
    scrollTop: 0,
    scrollLeft: 0,
    scrollHeight: 0,
    clientHeight: 400,
    clientWidth: 644,
    offsetWidth: 120,
    offsetHeight: 32,
    classList: {
      add() {},
      remove() {},
      toggle() {},
      contains() {
        return false;
      },
    },
    querySelector() {
      return null;
    },
    querySelectorAll() {
      return [];
    },
    addEventListener() {},
    removeEventListener() {},
    appendChild() {},
    getBoundingClientRect() {
      return { width: 644, height: 400, top: 0, left: 0, right: 644, bottom: 400 };
    },
  };
}

// Install minimal browser globals before dynamically importing `ui/app.js` in mock mode.
const elementsById = new Map();
function getEl(id) {
  if (!elementsById.has(id)) {
    elementsById.set(id, makeElementStub());
  }
  return elementsById.get(id);
}

globalThis.fetch = async (url) => {
  const href = String(url);
  if (href.endsWith("/defaults.json") || href.includes("/defaults.json?")) {
    return {
      ok: true,
      status: 200,
      async json() {
        return JSON.parse(DEFAULTS_RAW);
      },
    };
  }
  throw new Error(`Unexpected fetch in unit test: ${href}`);
};

globalThis.ResizeObserver = class {
  observe() {}
  disconnect() {}
};

globalThis.window = {
  location: { search: "?mock=1" },
  innerWidth: 1280,
  innerHeight: 800,
  scrollY: 0,
  scrollTo() {},
  getComputedStyle() {
    return {
      paddingLeft: "0",
      paddingRight: "0",
      getPropertyValue() {
        return "";
      },
    };
  },
  addEventListener() {},
  requestAnimationFrame() {
    return 0;
  },
  matchMedia() {
    return { matches: false, addEventListener() {} };
  },
  localStorage: {
    getItem() {
      return null;
    },
    setItem() {},
  },
};

globalThis.document = {
  visibilityState: "visible",
  documentElement: { dataset: {}, lang: "" },
  body: makeElementStub(),
  getElementById: getEl,
  querySelector() {
    return null;
  },
  querySelectorAll() {
    return [];
  },
  createElement() {
    return makeElementStub();
  },
  addEventListener() {},
};

const {
  applyPlaneResponse,
  buildLabelTooltip,
  buildSyncButtonTooltip,
  buildUnsummarizedSummary,
  DEFAULT_SETTINGS,
  ensureMockWindowDays,
  fmtSyncBatchSummary,
  fmtSyncUsageCompact,
  I18N,
  isProcessorConfigured,
  loadSyncEstimate,
  localizeRecency,
  renderContributionPopoverBody,
  renderPopoverLayer,
  renderSettingsView,
  renderSyncEstimateReadout,
  runSyncBatches,
  setSelectedHarness,
  state,
} = await import("../ui/app.js");

function makePlanePayload(settings) {
  return {
    days: [
      {
        idx: 0,
        isoDate: "2026-11-14",
        short: "Sat 14",
        tiny: "Sat",
        full: "Today",
        isToday: true,
      },
    ],
    anchorDate: "2026-11-14",
    nowHour: 12,
    tzLabel: "UTC+8",
    archivedCount: 0,
    settings,
    lines: [],
    unsummarized: [],
    shortSessions: { totalCount: 0, items: [], weekMinutes: 0 },
    contributions: [],
    harnesses: [],
    lastSync: null,
  };
}

test("applyPlaneResponse preserves explicitly empty and whitespace-only apiProvider", () => {
  for (const blank of ["", "   "]) {
    applyPlaneResponse(
      makePlanePayload({
        processorConfigured: true,
        processorMode: "api",
        apiProvider: blank,
      }),
    );
    assert.equal(state.settings.apiProvider, blank);
    assert.equal(state.settings.apiModel, "");
    assert.equal(
      isProcessorConfigured(state.settings),
      false,
      `blank apiProvider ${JSON.stringify(blank)} must not be treated as configured`,
    );
  }
});

test("applyPlaneResponse preserves saved apiModel override even when apiProvider is blank", () => {
  applyPlaneResponse(
    makePlanePayload({
      processorConfigured: true,
      processorMode: "api",
      apiProvider: "",
      apiModel: "custom-model",
    }),
  );
  assert.equal(state.settings.apiProvider, "");
  assert.equal(state.settings.apiModel, "custom-model");
  assert.equal(isProcessorConfigured(state.settings), false);
});

test("applyPlaneResponse uses catalog defaults only when apiProvider or apiModel is omitted", () => {
  const defaultProvider = DEFAULTS_JSON.defaultSettings.apiProvider;
  const defaultProviderSpec = DEFAULTS_JSON.apiProviders.find((p) => p.id === defaultProvider);
  const openrouterSpec = DEFAULTS_JSON.apiProviders.find((p) => p.id === "openrouter");
  const deepseekSpec = DEFAULTS_JSON.apiProviders.find((p) => p.id === "deepseek");

  assert.equal(defaultProviderSpec.defaultModel, "gemini-4-hydrogen");
  assert.equal(openrouterSpec.defaultModel, "google/gemini-4-hydrogen");

  applyPlaneResponse(
    makePlanePayload({
      processorConfigured: true,
      processorMode: "api",
    }),
  );
  assert.equal(state.settings.apiProvider, DEFAULT_SETTINGS.apiProvider);
  assert.equal(state.settings.apiModel, defaultProviderSpec.defaultModel);
  assert.equal(isProcessorConfigured(state.settings), true);

  applyPlaneResponse(
    makePlanePayload({
      processorConfigured: true,
      processorMode: "api",
      apiProvider: "gemini",
      apiModel: "gemini-2.5-flash-lite",
    }),
  );
  assert.equal(state.settings.apiProvider, "gemini");
  assert.equal(state.settings.apiModel, "gemini-2.5-flash-lite");

  applyPlaneResponse(
    makePlanePayload({
      processorConfigured: true,
      processorMode: "api",
      apiProvider: "deepseek",
    }),
  );
  assert.equal(state.settings.apiProvider, "deepseek");
  assert.equal(state.settings.apiModel, deepseekSpec.defaultModel);
  assert.equal(isProcessorConfigured(state.settings), true);

  applyPlaneResponse(
    makePlanePayload({
      processorConfigured: true,
      processorMode: "api",
      apiProvider: "deepseek",
      apiModel: "deepseek-reasoner",
    }),
  );
  assert.equal(state.settings.apiProvider, "deepseek");
  assert.equal(state.settings.apiModel, "deepseek-reasoner");

  applyPlaneResponse(
    makePlanePayload({
      processorConfigured: true,
      processorMode: "api",
      apiProvider: "deepseek",
      apiModel: "",
    }),
  );
  assert.equal(state.settings.apiProvider, "deepseek");
  assert.equal(state.settings.apiModel, "");
});

test("Settings processor view renders bilingual Gemini model placeholder and fast-tier hint", () => {
  state.settingsTab = "processor";

  for (const [language, expectedSnippet] of [
    ["zh", "默认 gemini-4-hydrogen；追求极速与最低成本可选 gemini-3.5-flash-lite。"],
    [
      "en",
      "Defaults to gemini-4-hydrogen; use gemini-3.5-flash-lite for ultra-fast, lowest-cost runs.",
    ],
  ]) {
    applyPlaneResponse(
      makePlanePayload({
        language,
        processorConfigured: true,
        processorMode: "api",
        apiProvider: "gemini",
      }),
    );
    const html = renderSettingsView();
    assert.ok(
      html.includes('placeholder="gemini-4-hydrogen"'),
      `${language} Gemini input placeholder should be gemini-4-hydrogen`,
    );
    assert.ok(
      html.includes(expectedSnippet),
      `${language} Gemini helper hint missing expected copy`,
    );
  }

  applyPlaneResponse(
    makePlanePayload({
      language: "en",
      processorConfigured: true,
      processorMode: "api",
      apiProvider: "openrouter",
    }),
  );
  const orHtml = renderSettingsView();
  assert.ok(orHtml.includes('placeholder="google/gemini-4-hydrogen"'));
  assert.ok(orHtml.includes("google/gemini-3.5-flash-lite"));

  applyPlaneResponse(
    makePlanePayload({
      language: "en",
      processorConfigured: true,
      processorMode: "api",
      apiProvider: "deepseek",
    }),
  );
  const dsHtml = renderSettingsView();
  assert.ok(dsHtml.includes('placeholder="deepseek-chat"'));
  assert.ok(!dsHtml.includes("gemini-3.5-flash-lite"));
});

test("harness switch invalidates stale all-harness estimate and refreshes scoped estimate", async () => {
  const prevFetch = globalThis.fetch;
  const requestedUrls = [];
  globalThis.fetch = async (url) => {
    const href = String(url);
    requestedUrls.push(href);
    const isCodex = href.includes("harness=codex");
    return {
      ok: true,
      status: 200,
      async json() {
        return isCodex
          ? {
              sessionCount: 1,
              batchCount: 1,
              estimatedInputTokens: 3200,
              estimatedOutputTokens: 240,
              estimatedThinkingTokens: 0,
              estimatedCostUsdRange: { low: 0.01, high: 0.02 },
              model: "gemini-4-hydrogen",
              backend: "api:gemini",
              isLocalCli: false,
            }
          : {
              sessionCount: 4,
              batchCount: 1,
              estimatedInputTokens: 14000,
              estimatedOutputTokens: 800,
              estimatedThinkingTokens: 0,
              estimatedCostUsdRange: { low: 0.05, high: 0.09 },
              model: "gemini-4-hydrogen",
              backend: "api:gemini",
              isLocalCli: false,
            };
      },
    };
  };

  try {
    state.selectedHarness = "all";
    state.route = "main";
    applyPlaneResponse({
      ...makePlanePayload({
        language: "en",
        processorConfigured: true,
        processorMode: "api",
        apiProvider: "gemini",
        apiModel: "gemini-4-hydrogen",
      }),
      unsummarized: [
        { id: "codex:1", harness: "codex", firstPrompt: "a", steps: 4, userInputCount: 3, bursts: [] },
        { id: "claude:1", harness: "claude", firstPrompt: "b", steps: 4, userInputCount: 3, bursts: [] },
      ],
    });

    await loadSyncEstimate(true);
    assert.match(buildSyncButtonTooltip(2), /4 sessions · 1 batch · ~\$0\.05–\$0\.09/);

    // Switch harness to codex: stale all-harness estimate must NOT appear in tooltip immediately,
    // and scoped `/api/sync/estimate?...&harness=codex` request must be issued.
    setSelectedHarness("codex");
    const immediateTip = buildSyncButtonTooltip(1);
    assert.doesNotMatch(immediateTip, /4 sessions/);
    assert.doesNotMatch(immediateTip, /\$0\.05/);

    // Flush microtasks for the background scoped estimate fetch.
    await new Promise((r) => setTimeout(r, 10));
    assert.ok(
      requestedUrls.some((u) => u.includes("/api/sync/estimate?") && u.includes("harness=codex")),
      `expected scoped codex estimate fetch, got: ${JSON.stringify(requestedUrls)}`,
    );
    assert.match(buildSyncButtonTooltip(1), /1 session · 1 batch · ~\$0\.01–\$0\.02/);
  } finally {
    state.selectedHarness = "all";
    globalThis.fetch = prevFetch;
  }
});

test("out-of-order estimate JSON decode is rejected and fetch failure shows unavailable state", async () => {
  const prevFetch = globalThis.fetch;
  let resolveSlowJson;
  let callIndex = 0;

  globalThis.fetch = async (url) => {
    const idx = ++callIndex;
    if (idx === 1) {
      // First request (all harnesses) resolves its JSON body slowly AFTER request #2 finishes.
      return {
        ok: true,
        status: 200,
        json() {
          return new Promise((resolve) => {
            resolveSlowJson = () =>
              resolve({
                sessionCount: 9,
                batchCount: 3,
                estimatedInputTokens: 30000,
                estimatedOutputTokens: 1800,
                estimatedThinkingTokens: 0,
                estimatedCostUsdRange: { low: 0.25, high: 0.4 },
                model: "gemini-4-hydrogen",
                backend: "api:gemini",
                isLocalCli: false,
              });
          });
        },
      };
    }
    if (idx === 2) {
      // Second request (codex) completes quickly.
      return {
        ok: true,
        status: 200,
        async json() {
          return {
            sessionCount: 2,
            batchCount: 1,
            estimatedInputTokens: 6000,
            estimatedOutputTokens: 420,
            estimatedThinkingTokens: 0,
            estimatedCostUsdRange: { low: 0.02, high: 0.03 },
            model: "gemini-4-hydrogen",
            backend: "api:gemini",
            isLocalCli: false,
          };
        },
      };
    }
    // Third request fails with HTTP 500.
    return {
      ok: false,
      status: 500,
      async json() {
        return { error: `boom for ${url}` };
      },
    };
  };

  try {
    state.selectedHarness = "all";
    state.route = "settings";
    state.settingsTab = "processor";
    applyPlaneResponse(
      makePlanePayload({
        language: "en",
        processorConfigured: true,
        processorMode: "api",
        apiProvider: "gemini",
        apiModel: "gemini-4-hydrogen",
      }),
    );

    const slowPromise = loadSyncEstimate(true);
    state.selectedHarness = "codex";
    await loadSyncEstimate(true);
    assert.equal(state.syncEstimate?.sessionCount, 2);

    // Now resolve the stale first request's JSON decoding: it MUST be ignored.
    resolveSlowJson();
    await slowPromise;
    assert.equal(state.syncEstimate?.sessionCount, 2);
    assert.match(renderSyncEstimateReadout(), /2 sessions · 1 batch/);

    // Now trigger a failed estimate fetch: stale data must be cleared and unavailable shown.
    await loadSyncEstimate(true);
    assert.equal(state.syncEstimate, null);
    assert.equal(state.syncEstimateError, true);
    assert.match(renderSyncEstimateReadout(), /Estimate unavailable/);

    // Verify partial usage formatting marks incomplete totals with `*` and `partial`.
    const partialStr = fmtSyncUsageCompact({
      totalTokens: 18000,
      costUsd: 0.046,
      incomplete: true,
      isLocalCli: false,
    });
    assert.match(partialStr, /\$0\.05\*/);
    assert.match(partialStr, /partial/);
  } finally {
    state.selectedHarness = "all";
    globalThis.fetch = prevFetch;
  }
});

test("partial timeline Sync invalidates stale 2-session estimate and refreshes remaining 1-session estimate", async () => {
  const prevFetch = globalThis.fetch;
  let estimateCalls = 0;

  const settings = {
    language: "en",
    processorConfigured: true,
    processorMode: "api",
    apiProvider: "gemini",
    apiModel: "gemini-4-hydrogen",
  };
  const remainingPlane = {
    ...makePlanePayload(settings),
    unsummarized: [
      { id: "codex:2", harness: "codex", firstPrompt: "second", steps: 4, userInputCount: 3, bursts: [] },
    ],
  };

  globalThis.fetch = async (url) => {
    const href = String(url);
    if (href.startsWith("/api/sync/estimate?")) {
      estimateCalls += 1;
      const payload =
        estimateCalls === 1
          ? {
              sessionCount: 2,
              batchCount: 1,
              estimatedInputTokens: 9000,
              estimatedOutputTokens: 420,
              estimatedThinkingTokens: 0,
              estimatedCostUsdRange: { low: 0.05, high: 0.09 },
              model: "gemini-4-hydrogen",
              backend: "api:gemini",
              isLocalCli: false,
            }
          : {
              sessionCount: 1,
              batchCount: 1,
              estimatedInputTokens: 4500,
              estimatedOutputTokens: 240,
              estimatedThinkingTokens: 0,
              estimatedCostUsdRange: { low: 0.02, high: 0.04 },
              model: "gemini-4-hydrogen",
              backend: "api:gemini",
              isLocalCli: false,
            };
      return {
        ok: true,
        status: 200,
        async json() {
          return payload;
        },
      };
    }
    if (href.startsWith("/api/sync/stream?")) {
      const encoder = new TextEncoder();
      const lines = [
        JSON.stringify({ type: "start", total: 2, totalBatches: 1, batchSize: 4, backend: "api:gemini" }),
        JSON.stringify({
          type: "batchDone",
          batchIndex: 1,
          totalBatches: 1,
          batchProcessed: 1,
          batchFailed: 1,
          filed: [{ sessionId: "codex:1", title: "Filed 1", lineName: "planar" }],
          error: "partial failure",
          processed: 1,
          failed: 1,
          skipped: 0,
          completed: 2,
          total: 2,
          backendUsed: "api:gemini",
          plane: remainingPlane,
        }),
        JSON.stringify({
          type: "complete",
          summary: {
            processed: 1,
            failed: 1,
            skipped: 0,
            backendUsed: "api:gemini",
            lastError: "partial failure",
          },
          plane: remainingPlane,
        }),
      ].join("\n");
      const chunks = [encoder.encode(`${lines}\n`)];
      let i = 0;
      return {
        ok: true,
        status: 200,
        body: {
          getReader() {
            return {
              async read() {
                if (i < chunks.length) {
                  return { value: chunks[i++], done: false };
                }
                return { value: undefined, done: true };
              },
            };
          },
        },
      };
    }
    throw new Error(`Unexpected fetch in partial-sync test: ${href}`);
  };

  try {
    state.selectedHarness = "all";
    state.route = "main";
    applyPlaneResponse({
      ...makePlanePayload(settings),
      unsummarized: [
        { id: "codex:1", harness: "codex", firstPrompt: "first", steps: 4, userInputCount: 3, bursts: [] },
        { id: "codex:2", harness: "codex", firstPrompt: "second", steps: 4, userInputCount: 3, bursts: [] },
      ],
    });

    await loadSyncEstimate(true);
    assert.match(buildSyncButtonTooltip(2), /2 sessions · 1 batch · ~\$0\.05–\$0\.09/);

    // Run Sync on the main timeline: files 1 of 2 sessions, leaving 1 pending.
    await runSyncBatches();
    assert.equal(state.unsummarized.length, 1);

    // Flush the post-Sync background estimate refresh.
    await new Promise((r) => setTimeout(r, 10));
    assert.equal(estimateCalls, 2, "expected fresh estimate fetch after partial Sync");
    const retryTip = buildSyncButtonTooltip(1);
    assert.doesNotMatch(retryTip, /2 sessions/);
    assert.doesNotMatch(retryTip, /\$0\.05/);
    assert.match(retryTip, /1 session · 1 batch · ~\$0\.02–\$0\.04/);
  } finally {
    state.selectedHarness = "all";
    globalThis.fetch = prevFetch;
  }
});

test("Settings processor view renders syncConcurrency stepper cleanly in zh/en and API/CLI modes", () => {
  state.settingsTab = "processor";
  assert.equal(DEFAULTS_JSON.defaultSettings.syncConcurrency, 4);
  assert.equal(DEFAULT_SETTINGS.syncConcurrency, 4);

  for (const [language, mode, expectedTitle, expectedDesc] of [
    ["zh", "api", "并发归纳批次数", "同时并行处理的批次数（1–16，默认 4）。"],
    ["zh", "cli", "并发归纳批次数", "本地 CLI 模式自动限制为 ≤ 2 并发，避免子进程过载。"],
    ["en", "api", "Concurrent Sync batches", "Batches run in parallel (1–16, default 4)."],
    ["en", "cli", "Concurrent Sync batches", "Local CLI mode caps active subprocesses at 2."],
  ]) {
    applyPlaneResponse(
      makePlanePayload({
        language,
        processorConfigured: true,
        processorMode: mode,
        apiProvider: "gemini",
        cliHarness: "codex",
        syncConcurrency: 4,
      }),
    );
    const html = renderSettingsView();
    assert.ok(html.includes('data-field="syncConcurrency"'), "missing syncConcurrency stepper");
    assert.ok(html.includes(expectedTitle), `missing title for ${language}/${mode}`);
    assert.ok(html.includes(expectedDesc), `missing description for ${language}/${mode}`);
  }
});

test("lightweight batchDone without plane removes filed sessions and refreshes via lite=1 before complete", async () => {
  const prevFetch = globalThis.fetch;
  const requestedUrls = [];
  const settings = {
    language: "en",
    processorConfigured: true,
    processorMode: "api",
    apiProvider: "gemini",
    apiModel: "gemini-4-hydrogen",
    syncConcurrency: 4,
  };
  const finalPlane = {
    ...makePlanePayload(settings),
    unsummarized: [],
    lines: [
      {
        id: "planar",
        name: "planar",
        description: "",
        color: "#A43D74",
        pinned: false,
        isSystemBucket: false,
        priority: 0,
        status: "active",
        sessions: [],
      },
    ],
  };

  globalThis.fetch = async (url) => {
    const href = String(url);
    requestedUrls.push(href);
    if (href.startsWith("/api/sync/stream?")) {
      const encoder = new TextEncoder();
      const lines = [
        JSON.stringify({
          type: "start",
          total: 2,
          totalBatches: 2,
          batchSize: 1,
          concurrency: 2,
          backend: "api:gemini",
        }),
        JSON.stringify({
          type: "batchStart",
          batchIndex: 1,
          totalBatches: 2,
          batchSize: 1,
          items: [{ sessionId: "codex:1", harness: "codex", label: "first" }],
        }),
        JSON.stringify({
          type: "batchStart",
          batchIndex: 2,
          totalBatches: 2,
          batchSize: 1,
          items: [{ sessionId: "codex:2", harness: "codex", label: "second" }],
        }),
        JSON.stringify({
          type: "batchDone",
          batchIndex: 2,
          totalBatches: 2,
          batchProcessed: 1,
          batchFailed: 0,
          filed: [{ sessionId: "codex:2", title: "Filed 2", lineName: "planar" }],
          error: null,
          processed: 1,
          failed: 0,
          skipped: 0,
          completed: 1,
          total: 2,
          backendUsed: "api:gemini",
        }),
        JSON.stringify({
          type: "batchDone",
          batchIndex: 1,
          totalBatches: 2,
          batchProcessed: 1,
          batchFailed: 0,
          filed: [{ sessionId: "codex:1", title: "Filed 1", lineName: "planar" }],
          error: null,
          processed: 2,
          failed: 0,
          skipped: 0,
          completed: 2,
          total: 2,
          backendUsed: "api:gemini",
        }),
        JSON.stringify({
          type: "complete",
          summary: {
            processed: 2,
            failed: 0,
            skipped: 0,
            backendUsed: "api:gemini",
            lastError: null,
          },
          plane: finalPlane,
        }),
      ].join("\n");
      const chunks = [encoder.encode(`${lines}\n`)];
      let i = 0;
      return {
        ok: true,
        status: 200,
        body: {
          getReader() {
            return {
              async read() {
                if (i < chunks.length) return { value: chunks[i++], done: false };
                return { value: undefined, done: true };
              },
            };
          },
        },
      };
    }
    if (href.startsWith("/api/plane?") && href.includes("lite=1")) {
      return {
        ok: true,
        status: 200,
        async json() {
          return {
            days: finalPlane.days,
            anchorDate: finalPlane.anchorDate,
            nowHour: finalPlane.nowHour,
            tzLabel: finalPlane.tzLabel,
            archivedCount: 0,
            settings,
            lines: finalPlane.lines,
            unsummarized: [],
            shortSessions: finalPlane.shortSessions,
            lastSync: null,
          };
        },
      };
    }
    if (href.startsWith("/api/sync/estimate?")) {
      return {
        ok: true,
        status: 200,
        async json() {
          return {
            sessionCount: 0,
            batchCount: 0,
            estimatedInputTokens: 0,
            estimatedOutputTokens: 0,
            estimatedThinkingTokens: 0,
            estimatedCostUsdRange: null,
            model: "gemini-4-hydrogen",
            backend: "api:gemini",
            isLocalCli: false,
          };
        },
      };
    }
    throw new Error(`Unexpected fetch in lightweight batchDone test: ${href}`);
  };

  try {
    state.selectedHarness = "all";
    state.route = "main";
    applyPlaneResponse({
      ...makePlanePayload(settings),
      unsummarized: [
        { id: "codex:1", harness: "codex", firstPrompt: "first", steps: 4, userInputCount: 3, bursts: [] },
        { id: "codex:2", harness: "codex", firstPrompt: "second", steps: 4, userInputCount: 3, bursts: [] },
      ],
    });
    await runSyncBatches();
    assert.equal(state.unsummarized.length, 0);
    assert.equal(state.lines.length, 1);
    assert.ok(
      requestedUrls.some((u) => u.includes("/api/plane?") && u.includes("lite=1")),
      `expected throttled lite=1 refresh during stream, got ${JSON.stringify(requestedUrls)}`,
    );
    assert.ok(
      requestedUrls.some((u) => u.startsWith("/api/sync/stream?") && u.includes("runId=")),
      `expected scoped runId on /api/sync/stream, got ${JSON.stringify(requestedUrls)}`,
    );
  } finally {
    state.selectedHarness = "all";
    globalThis.fetch = prevFetch;
  }
});

test("lite snapshot reconciles against newer batchDone events and preserves known sourceExists across Pending-to-Line moves", async () => {
  const prevFetch = globalThis.fetch;
  let resolveLiteJson;
  let streamChunkRelease;
  const settings = {
    language: "en",
    processorConfigured: true,
    processorMode: "api",
    apiProvider: "gemini",
    apiModel: "gemini-4-hydrogen",
    syncConcurrency: 4,
  };
  const prunedSession = {
    id: "codex:pruned",
    harness: "codex",
    title: "Pruned Old Session",
    rawTitle: "Pruned Old Session",
    steps: 6,
    userInputCount: 3,
    recency: "2d ago",
    running: false,
    stoppedAt: "Locked",
    leftOpen: "",
    dependsOnId: null,
    hidden: false,
    archived: false,
    cwd: "/tmp/ws",
    sourcePath: "/tmp/missing.jsonl",
    sourceExists: false,
    model: null,
    totalTokens: 0,
    costUsd: null,
    lastActiveAt: 1000,
    bursts: [],
  };

  globalThis.fetch = async (url) => {
    const href = String(url);
    if (href.startsWith("/api/sync/stream?")) {
      const encoder = new TextEncoder();
      const firstBatchEvents = [
        JSON.stringify({
          type: "start",
          total: 2,
          totalBatches: 2,
          batchSize: 1,
          concurrency: 2,
          backend: "api:gemini",
        }),
        JSON.stringify({
          type: "batchDone",
          batchIndex: 1,
          totalBatches: 2,
          batchProcessed: 1,
          batchFailed: 0,
          // `codex:pruned-move` starts in `unsummarized` with `sourceExists: false` and moves to a Line!
          filed: [{ sessionId: "codex:pruned-move", title: "Filed Pruned", lineName: "planar" }],
          error: null,
          processed: 1,
          failed: 0,
          skipped: 0,
          completed: 1,
          total: 2,
          backendUsed: "api:gemini",
        }),
      ].join("\n");
      const secondBatchEvent = JSON.stringify({
        type: "batchDone",
        batchIndex: 2,
        totalBatches: 2,
        batchProcessed: 1,
        batchFailed: 0,
        filed: [{ sessionId: "codex:2", title: "Filed 2", lineName: "planar" }],
        error: null,
        processed: 2,
        failed: 0,
        skipped: 0,
        completed: 2,
        total: 2,
        backendUsed: "api:gemini",
      });
      const completeEvent = JSON.stringify({
        type: "complete",
        summary: {
          processed: 2,
          failed: 0,
          skipped: 0,
          backendUsed: "api:gemini",
          lastError: null,
        },
        plane: {
          ...makePlanePayload(settings),
          lines: [
            {
              id: "planar",
              name: "planar",
              description: "",
              color: "#A43D74",
              pinned: false,
              isSystemBucket: false,
              priority: 0,
              status: "active",
              sessions: [prunedSession],
            },
          ],
          unsummarized: [],
        },
      });
      let step = 0;
      return {
        ok: true,
        status: 200,
        body: {
          getReader() {
            return {
              async read() {
                if (step === 0) {
                  step += 1;
                  return { value: encoder.encode(`${firstBatchEvents}\n`), done: false };
                }
                if (step === 1) {
                  step += 1;
                  // Emit Batch 2 BEFORE the stale lite snapshot from Batch 1 resolves!
                  return { value: encoder.encode(`${secondBatchEvent}\n`), done: false };
                }
                if (step === 2) {
                  step += 1;
                  await new Promise((resolve) => {
                    streamChunkRelease = resolve;
                  });
                  return { value: encoder.encode(`${completeEvent}\n`), done: false };
                }
                return { value: undefined, done: true };
              },
            };
          },
        },
      };
    }
    if (href.startsWith("/api/plane?") && href.includes("lite=1")) {
      return {
        ok: true,
        status: 200,
        json() {
          return new Promise((resolve) => {
            resolveLiteJson = () =>
              resolve({
                isLite: true,
                days: makePlanePayload(settings).days,
                anchorDate: "2026-11-14",
                nowHour: 12,
                tzLabel: "UTC+8",
                archivedCount: 0,
                settings,
                lines: [
                  {
                    id: "planar",
                    name: "planar",
                    description: "",
                    color: "#A43D74",
                    pinned: false,
                    isSystemBucket: false,
                    priority: 0,
                    status: "active",
                    // Lite snapshot omits stat() and sends `sourceExists: null` (unknown) for all items:
                    // 1) previously filed `codex:pruned`, 2) moved `codex:pruned-move`, 3) brand-new unknown `codex:brand-new`.
                    sessions: [
                      { ...prunedSession, sourceExists: null },
                      { ...prunedSession, id: "codex:pruned-move", title: "Filed Pruned", sourceExists: null },
                      { ...prunedSession, id: "codex:brand-new", title: "Brand New", sourceExists: null },
                    ],
                  },
                ],
                // Stale snapshot taken before Batch 2 committed still includes `codex:2`!
                unsummarized: [
                  {
                    id: "codex:2",
                    harness: "codex",
                    firstPrompt: "second",
                    steps: 4,
                    userInputCount: 3,
                    sourceExists: null,
                    bursts: [],
                  },
                  {
                    id: "codex:pruned-pending",
                    harness: "codex",
                    firstPrompt: "pruned pending",
                    steps: 4,
                    userInputCount: 3,
                    sourceExists: null,
                    bursts: [],
                  },
                ],
                shortSessions: { totalCount: 0, items: [], weekMinutes: 0 },
                lastSync: null,
              });
          });
        },
      };
    }
    if (href.startsWith("/api/sync/estimate?")) {
      return {
        ok: true,
        status: 200,
        async json() {
          return {
            sessionCount: 0,
            batchCount: 0,
            estimatedInputTokens: 0,
            estimatedOutputTokens: 0,
            estimatedThinkingTokens: 0,
            estimatedCostUsdRange: null,
            model: "gemini-4-hydrogen",
            backend: "api:gemini",
            isLocalCli: false,
          };
        },
      };
    }
    throw new Error(`Unexpected fetch in lite reconciliation test: ${href}`);
  };

  try {
    state.selectedHarness = "all";
    state.route = "main";
    applyPlaneResponse({
      ...makePlanePayload(settings),
      lines: [
        {
          id: "planar",
          name: "planar",
          description: "",
          color: "#A43D74",
          pinned: false,
          isSystemBucket: false,
          priority: 0,
          status: "active",
          sessions: [prunedSession],
        },
      ],
      unsummarized: [
        {
          id: "codex:pruned-move",
          harness: "codex",
          firstPrompt: "pruned pending moved to line",
          steps: 4,
          userInputCount: 3,
          sourceExists: false,
          bursts: [],
        },
        { id: "codex:2", harness: "codex", firstPrompt: "second", steps: 4, userInputCount: 3, sourceExists: true, bursts: [] },
        {
          id: "codex:pruned-pending",
          harness: "codex",
          firstPrompt: "pruned pending",
          steps: 4,
          userInputCount: 3,
          sourceExists: false,
          bursts: [],
        },
      ],
    });

    const syncPromise = runSyncBatches();
    // Wait until Batch 1 and Batch 2 have been processed and lite=1 request is in flight.
    await new Promise((r) => setTimeout(r, 15));
    assert.equal(typeof resolveLiteJson, "function", "expected in-flight lite=1 refresh");
    // Resolve stale lite snapshot AFTER Batch 2 already filed `codex:2`.
    resolveLiteJson();
    await new Promise((r) => setTimeout(r, 15));

    // Stale lite snapshot MUST NOT resurrect `codex:2` in unsummarized!
    assert.deepEqual(
      state.unsummarized.map((u) => u.id),
      ["codex:pruned-pending"],
      "stale lite snapshot must not resurrect sessions filed by newer BatchDone events",
    );
    // Known `sourceExists: false` MUST be preserved on already-filed, Pending-to-Line moved, and still-pending sessions!
    const byId = new Map(state.lines[0].sessions.map((s) => [s.id, s]));
    assert.equal(
      byId.get("codex:pruned")?.sourceExists,
      false,
      "lite snapshot must preserve known sourceExists=false on already-filed sessions",
    );
    assert.equal(
      byId.get("codex:pruned-move")?.sourceExists,
      false,
      "lite snapshot must preserve known sourceExists=false across Pending-to-Line moves",
    );
    assert.equal(
      byId.get("codex:brand-new")?.sourceExists,
      null,
      "unchecked previously unknown sessions must remain null (unknown), not true",
    );
    assert.equal(
      state.unsummarized[0].sourceExists,
      false,
      "lite snapshot must preserve known sourceExists=false on unsummarized sessions",
    );

    streamChunkRelease();
    await syncPromise;
  } finally {
    state.selectedHarness = "all";
    globalThis.fetch = prevFetch;
  }
});

function findBalancedBlock(src, openIdx, openChar, closeChar) {
  assert.equal(src[openIdx], openChar, `expected '${openChar}' at offset ${openIdx}`);
  let depth = 0;
  let inTemplate = false;
  const templateExprBraces = [];

  for (let i = openIdx; i < src.length; i += 1) {
    const ch = src[i];
    const next = src[i + 1];

    if (inTemplate) {
      if (ch === "\\") {
        i += 1;
        continue;
      }
      if (ch === "`") {
        inTemplate = false;
        continue;
      }
      if (ch === "$" && next === "{") {
        inTemplate = false;
        templateExprBraces.push(1);
        i += 1;
      }
      continue;
    }

    if (ch === "/" && next === "/") {
      const nl = src.indexOf("\n", i + 2);
      if (nl === -1) break;
      i = nl;
      continue;
    }
    if (ch === "/" && next === "*") {
      const end = src.indexOf("*/", i + 2);
      assert.notEqual(end, -1, "unterminated block comment in UI JS module");
      i = end + 1;
      continue;
    }
    if (ch === "'" || ch === '"') {
      const quote = ch;
      for (i += 1; i < src.length; i += 1) {
        if (src[i] === "\\") {
          i += 1;
          continue;
        }
        if (src[i] === quote) break;
      }
      continue;
    }
    if (ch === "`") {
      inTemplate = true;
      continue;
    }

    if (templateExprBraces.length > 0) {
      if (ch === "{") {
        templateExprBraces[templateExprBraces.length - 1] += 1;
      } else if (ch === "}") {
        templateExprBraces[templateExprBraces.length - 1] -= 1;
        if (templateExprBraces[templateExprBraces.length - 1] === 0) {
          templateExprBraces.pop();
          inTemplate = true;
        }
      }
      continue;
    }

    if (ch === openChar) {
      depth += 1;
    } else if (ch === closeChar) {
      depth -= 1;
      if (depth === 0) {
        return { start: openIdx, end: i + 1 };
      }
    }
  }
  throw new Error(`Unbalanced '${openChar}...${closeChar}' block starting at offset ${openIdx}`);
}

function maskRangePreservingNewlines(src, start, end) {
  const before = src.slice(0, start);
  const middle = src.slice(start, end).replace(/[^\n]/g, " ");
  const after = src.slice(end);
  return `${before}${middle}${after}`;
}

function findMissingTranslationKeys(sources, zhSet, enSet) {
  const missing = [];
  for (const { rel, src } of sources) {
    for (const match of src.matchAll(/\bt\(\s*(["'`])([A-Za-z0-9_]+)\1/g)) {
      const key = match[2];
      if (!zhSet.has(key) || !enSet.has(key)) {
        missing.push(`${rel}:${key}`);
      }
    }
  }
  return missing;
}

function extractStaticImportSpecifiers(src) {
  const specs = [];
  const re =
    /^\s*(?:import\s+(?:["']([^"']+)["']|[^;]*?\bfrom\s+["']([^"']+)["'])|export\s+(?:\*(?:\s+as\s+[A-Za-z0-9_$]+)?|\{[^;{}]*\})\s+from\s+["']([^"']+)["'])\s*;?/gm;
  for (const match of src.matchAll(re)) {
    const spec = match[1] ?? match[2] ?? match[3];
    if (spec) specs.push(spec);
  }
  return specs;
}

function buildUiImportGraph(sources) {
  const knownFiles = new Set(sources.map((f) => f.rel));
  const importsByFile = new Map();
  for (const { rel, src } of sources) {
    const specs = [];
    for (const spec of extractStaticImportSpecifiers(src)) {
      assert.ok(spec.startsWith("./"), `${rel} must use relative ./ ESM imports (found ${spec})`);
      assert.ok(!spec.includes("?"), `${rel} must keep relative ESM imports unversioned (found ${spec})`);
      const targetRel = `ui/${spec.slice(2)}`;
      assert.ok(knownFiles.has(targetRel), `${rel} imports missing UI module ${spec}`);
      specs.push(targetRel);
    }
    importsByFile.set(rel, specs);
  }
  return importsByFile;
}

function findImportCycle(importsByFile) {
  const stateByNode = new Map(); // 0/undefined = unvisited, 1 = visiting, 2 = visited
  const stack = [];

  function dfs(node) {
    stateByNode.set(node, 1);
    stack.push(node);
    for (const dep of importsByFile.get(node) || []) {
      const depState = stateByNode.get(dep);
      if (depState === 1) {
        const idx = stack.indexOf(dep);
        return [...stack.slice(idx), dep];
      }
      if (!depState) {
        const cycle = dfs(dep);
        if (cycle) return cycle;
      }
    }
    stack.pop();
    stateByNode.set(node, 2);
    return null;
  }

  for (const node of importsByFile.keys()) {
    if (!stateByNode.get(node)) {
      const cycle = dfs(node);
      if (cycle) return cycle;
    }
  }
  return null;
}

test("I18N.zh and I18N.en have identical key sets, matching value shapes, and cover all static t() calls", () => {
  const zhKeys = Object.keys(I18N.zh);
  const enKeys = Object.keys(I18N.en);
  const zhSet = new Set(zhKeys);
  const enSet = new Set(enKeys);

  assert.deepEqual(
    zhKeys.filter((k) => !enSet.has(k)),
    [],
    "keys present in I18N.zh but missing from I18N.en",
  );
  assert.deepEqual(
    enKeys.filter((k) => !zhSet.has(k)),
    [],
    "keys present in I18N.en but missing from I18N.zh",
  );

  for (const key of zhKeys) {
    const zhVal = I18N.zh[key];
    const enVal = I18N.en[key];
    if (Array.isArray(zhVal) || Array.isArray(enVal)) {
      assert.ok(Array.isArray(zhVal) && Array.isArray(enVal), `I18N.${key} must be an array in both locales`);
      assert.equal(zhVal.length, enVal.length, `I18N.${key} array length mismatch between zh and en`);
      assert.ok(
        zhVal.every((item) => typeof item === "string") && enVal.every((item) => typeof item === "string"),
        `I18N.${key} array items must all be strings`,
      );
    } else {
      assert.equal(typeof zhVal, "string", `I18N.zh.${key} must be a string`);
      assert.equal(typeof enVal, "string", `I18N.en.${key} must be a string`);
      assert.ok(zhVal.length > 0, `I18N.zh.${key} must be non-empty`);
      assert.ok(enVal.length > 0, `I18N.en.${key} must be non-empty`);
    }
  }

  const uiJsFiles = collectUiJsFiles();
  assert.ok(uiJsFiles.length >= 4, `expected >= 4 UI JS modules, found ${uiJsFiles.length}`);
  const uiSources = uiJsFiles.map(({ abs, rel }) => ({ rel, src: readFileSync(abs, "utf8") }));

  assert.deepEqual(
    findMissingTranslationKeys(uiSources, zhSet, enSet),
    [],
    "static t(...) call references missing I18N key(s)",
  );

  // Negative cases: unknown keys in double quotes, single quotes, and static template literals
  // must all be flagged, while valid keys and dynamic interpolated templates are ignored.
  const interpolationToken = ["$", "{suffix}"].join("");
  const syntheticMissing = findMissingTranslationKeys(
    [
      {
        rel: "ui/synthetic.js",
        src: [
          'const a = t("navPlane");',
          "const b = t('missingSingleKey');",
          'const c = t("missingDoubleKey");',
          "const d = t(`missingTemplateKey`);",
          "const e = t(`navPlane`);",
          `const f = t(\`dynamic_${interpolationToken}\`);`,
        ].join("\n"),
      },
    ],
    zhSet,
    enSet,
  );
  assert.deepEqual(syntheticMissing, [
    "ui/synthetic.js:missingSingleKey",
    "ui/synthetic.js:missingDoubleKey",
    "ui/synthetic.js:missingTemplateKey",
  ]);
});

test("leaf UI modules (`band.js`, `mock.js`, `i18n.js`) work with zero DOM/fetch and form a strict 1-way DAG", () => {
  // 1. Verify strict 1-way unversioned ESM import graph across every ui/*.js file.
  const uiJsFiles = collectUiJsFiles();
  const uiSources = uiJsFiles.map(({ abs, rel }) => ({ rel, src: readFileSync(abs, "utf8") }));
  const importsByFile = buildUiImportGraph(uiSources);

  assert.equal(
    findImportCycle(importsByFile),
    null,
    "ui/*.js module graph must be acyclic",
  );

  for (const leaf of ["ui/band.js", "ui/mock.js", "ui/i18n.js"]) {
    assert.deepEqual(
      importsByFile.get(leaf),
      [],
      `${leaf} must remain a pure zero-import leaf module`,
    );
  }
  assert.deepEqual(
    [...(importsByFile.get("ui/app.js") || [])].sort(),
    ["ui/band.js", "ui/i18n.js", "ui/mock.js"],
    "ui/app.js must form a strict 1-way DAG to {band.js, i18n.js, mock.js}",
  );

  // Negative case: side-effect imports and circular imports must be detected.
  const cyclicGraph = buildUiImportGraph([
    { rel: "ui/a.js", src: 'import { x } from "./b.js";\nexport const a = x;' },
    { rel: "ui/b.js", src: 'import "./a.js";\nexport const x = 1;' },
  ]);
  assert.deepEqual(cyclicGraph.get("ui/b.js"), ["ui/a.js"]);
  assert.deepEqual(findImportCycle(cyclicGraph), ["ui/a.js", "ui/b.js", "ui/a.js"]);

  // 2. Direct unit checks for ui/band.js
  assert.equal(bandModule.BAND_JOIN_PX, 2.5);
  assert.equal(bandModule.BAND_EASE_PX, 5.0);
  assert.deepEqual(bandModule.bandProfile([]), []);

  // Same session with overlapping bursts must not double-count itself.
  const singleOverlap = bandModule.bandProfile([
    [
      [10, 40],
      [20, 60],
    ],
  ]);
  assert.equal(singleOverlap.length, 1);
  assert.equal(singleOverlap[0].length, 1);
  assert.equal(singleOverlap[0][0][2], 2.5);

  // Two distinct concurrent sessions swell from 2.5 -> 3.5 -> 2.5 and render a smooth closed path.
  const concurrent = bandModule.bandProfile([[[10, 100]], [[35, 75]]]);
  assert.equal(concurrent.length, 1);
  assert.deepEqual(
    concurrent[0].map((seg) => seg[2]),
    [2.5, 3.5, 2.5],
  );
  const smoothPath = bandModule.bandPath(concurrent[0], 14);
  assert.ok(smoothPath.startsWith("M") && smoothPath.endsWith("Z"));
  assert.ok(smoothPath.includes("A"), "bandPath must include semicircular end caps");

  // Sub-lane stays at uniform 2.0 half-thickness even with overlap.
  const subLane = bandModule.bandProfile([[[10, 80]], [[20, 60]]], true);
  assert.equal(subLane.length, 1);
  assert.equal(subLane[0].length, 1);
  assert.equal(subLane[0][0][2], 2.0);
  const capsule = bandModule.bandPath(subLane[0], 10);
  assert.ok(capsule.startsWith("M") && capsule.endsWith("Z"));

  // Narrow sub-BAND_JOIN_PX gap between two distinct sessions is bridged without a zero waist.
  const bridged = bandModule.bandProfile([[[10, 30]], [[31.5, 50]]]);
  assert.equal(bridged.length, 1);
  assert.equal(bridged[0].length, 1);

  // 3. Direct unit checks for ui/mock.js
  assert.ok(mockModule.DEFAULT_MOCK_CONTRIBUTIONS.length >= 140);
  assert.equal(mockModule.DEFAULT_MOCK_CONTRIBUTIONS[0].weekday, 0);
  assert.equal(mockModule.DEFAULT_MOCK_CONTRIBUTIONS.at(-1).date, "2026-11-14");
  assert.deepEqual(mockModule.buildMockContributions(), mockModule.DEFAULT_MOCK_CONTRIBUTIONS);
  assert.deepEqual(
    mockModule.DEFAULT_MOCK_HARNESSES.map((h) => h.id),
    ["antigravity", "claude", "codex", "dsh"],
  );

  const customPalette = ["#111111", "#222222", "#333333", "#444444", "#555555", "#666666"];
  const planeA = mockModule.createMockPlane(customPalette);
  const planeB = mockModule.createMockPlane(customPalette);
  assert.equal(planeA.days.length, 7);
  assert.equal(planeA.archivedCount, 11);
  assert.equal(planeA.lines[0].color, "#111111");
  assert.equal(planeA.lines[4].color, "#555555");
  assert.equal(planeA.unsummarized.length, 2);
  assert.equal(planeA.shortSessions.items.length, 4);

  // Mutating planeA must never leak into planeB or exported defaults.
  planeA.days[0].full = "MUTATED";
  planeA.lines[0].name = "MUTATED";
  planeA.lines[0].sessions.pop();
  planeA.unsummarized.pop();
  planeA.shortSessions.items.pop();
  planeA.contributions[0].totalMinutes = 999999;
  planeA.harnesses[0].enabled = false;

  assert.notEqual(planeB.days[0].full, "MUTATED");
  assert.equal(planeB.lines[0].name, "sqlite-sync");
  assert.equal(planeB.lines[0].sessions.length, 3);
  assert.equal(planeB.unsummarized.length, 2);
  assert.equal(planeB.shortSessions.items.length, 4);
  assert.notEqual(planeB.contributions[0].totalMinutes, 999999);
  assert.notEqual(mockModule.DEFAULT_MOCK_CONTRIBUTIONS[0].totalMinutes, 999999);
  assert.equal(planeB.harnesses[0].enabled, true);
  assert.equal(mockModule.DEFAULT_MOCK_HARNESSES[0].enabled, true);

  // 4. Direct unit checks for ui/i18n.js
  assert.deepEqual(
    i18nModule.LANG_OPTIONS.map((o) => o.id),
    ["zh", "en"],
  );
  const dict = i18nModule.createI18n({
    cliAutoOrderText: "agy → codex → dsh → claude",
    apiModelExamples: ["model-a", "model-b", "model-c"],
  });
  assert.equal(dict.zh.cliAutoOrderMeta, "agy → codex → dsh → claude");
  assert.equal(dict.en.cliAutoOrderMeta, "agy → codex → dsh → claude");
  assert.ok(dict.zh.descCliHarness.includes("agy → codex → dsh → claude"));
  assert.ok(dict.en.descCliHarness.includes("agy → codex → dsh → claude"));
  assert.ok(dict.zh.descApiModel.includes("model-a、model-b、model-c"));
  assert.ok(dict.en.descApiModel.includes("model-a, model-b, model-c"));
  assert.deepEqual(Object.keys(dict.zh).sort(), Object.keys(dict.en).sort());
});

test("all ui/*.js modules contain no CJK characters outside LANG_OPTIONS and I18N.zh in ui/i18n.js", () => {
  const uiJsFiles = collectUiJsFiles();
  const cjkRe = /[\u3000-\u303f\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uff00-\uffef]/;
  const violations = [];
  let checkedI18n = false;

  for (const { abs, rel } of uiJsFiles) {
    const src = readFileSync(abs, "utf8");
    let scannable = src;

    if (rel === "ui/i18n.js") {
      checkedI18n = true;
      const langMatch = /\bexport\s+const\s+LANG_OPTIONS\s*=\s*\[/.exec(src);
      assert.ok(langMatch, "could not locate LANG_OPTIONS in ui/i18n.js");
      const langSpan = findBalancedBlock(src, langMatch.index + langMatch[0].length - 1, "[", "]");
      assert.ok(src.slice(langSpan.start, langSpan.end).includes("简体中文"));

      const fnMatch = /\bexport\s+function\s+createI18n\b[\s\S]*?\breturn\s*\{/.exec(src);
      assert.ok(fnMatch, "could not locate createI18n return dictionary in ui/i18n.js");
      const dictSpan = findBalancedBlock(src, fnMatch.index + fnMatch[0].length - 1, "{", "}");
      const dictBody = src.slice(dictSpan.start, dictSpan.end);

      const zhMatch = /\bzh\s*:\s*\{/.exec(dictBody);
      assert.ok(zhMatch, "could not locate zh dictionary block in ui/i18n.js");
      const zhOpenAbs = dictSpan.start + zhMatch.index + zhMatch[0].length - 1;
      const zhSpan = findBalancedBlock(src, zhOpenAbs, "{", "}");
      assert.ok(src.slice(zhSpan.start, zhSpan.end).includes("navPlane"));

      // Confirm en is strictly outside the masked zh span so CJK leaked into en is caught too.
      const enMatch = /\ben\s*:\s*\{/.exec(dictBody);
      assert.ok(enMatch, "could not locate en dictionary block in ui/i18n.js");
      const enOpenAbs = dictSpan.start + enMatch.index + enMatch[0].length - 1;
      const enSpan = findBalancedBlock(src, enOpenAbs, "{", "}");
      assert.ok(enOpenAbs >= zhSpan.end, "en block must not fall inside the zh exemption span");
      assert.ok(src.slice(enSpan.start, enSpan.end).includes("navPlane"));

      scannable = maskRangePreservingNewlines(scannable, langSpan.start, langSpan.end);
      scannable = maskRangePreservingNewlines(scannable, zhSpan.start, zhSpan.end);
    }

    const lines = scannable.split("\n");
    for (let idx = 0; idx < lines.length; idx += 1) {
      if (cjkRe.test(lines[idx])) {
        violations.push(`${rel}:${idx + 1}: ${lines[idx].trim()}`);
      }
    }
  }

  assert.equal(checkedI18n, true, "expected ui/i18n.js to be present during CJK scan");
  assert.deepEqual(
    violations,
    [],
    `Found CJK character(s) outside LANG_OPTIONS and zh dictionary in ui/i18n.js:\n${violations.join("\n")}`,
  );
});

test("consolidated I18N formatters and popovers render exact zh and en copy", () => {
  const prevPopover = state.popover;
  const prevDataSource = state.dataSource;
  const prevDays = state.days;
  const prevSelectedDayIdx = state.selectedDayIdx;
  const prevContributions = state.contributions;
  const prevLines = state.lines;
  const prevUnsummarized = state.unsummarized;
  const prevLang = state.settings.language;

  try {
    const sampleLine = {
      id: "line-1",
      name: "planar",
      description: "",
      color: "#A43D74",
      pinned: true,
      isSystemBucket: false,
      priority: 1,
      status: "active",
      sessions: [],
    };
    const sampleContrib = [
      {
        date: "2026-10-05",
        weekday: 0,
        totalMinutes: 45,
        totalTokens: 12000,
        costUsd: 0.02,
        sessionCount: 2,
        byHarness: { codex: 45 },
      },
      {
        date: "2026-11-02",
        weekday: 0,
        totalMinutes: 90,
        totalTokens: 24000,
        costUsd: 0.04,
        sessionCount: 3,
        byHarness: { codex: 90 },
      },
    ];
    const sampleUnsummarized = {
      harness: "codex",
      userInputCount: 5,
      steps: 12,
      cwd: "/tmp/demo",
    };
    const sampleLane = {
      kind: "parent",
      label: "planar",
      stats: {
        activeSessions: [{ title: "First" }, { title: "Second" }],
        agentMins: 40,
        wallMins: 30,
        tokens: 12000,
        costUsd: 0.03,
      },
    };

    // Chinese locale checks
    state.settings.language = "zh";
    assert.equal(localizeRecency("running"), "运行中");
    assert.equal(localizeRecency("just now"), "刚刚");
    assert.equal(localizeRecency("12m ago"), "12分钟前");
    assert.equal(localizeRecency("3h ago"), "3小时前");
    assert.equal(localizeRecency("2d ago"), "2天前");
    assert.equal(localizeRecency("custom"), "custom");

    assert.equal(fmtSyncBatchSummary(1, 1), "1 个会话 · 1 批");
    assert.equal(fmtSyncBatchSummary(4, 2), "4 个会话 · 2 批");

    assert.equal(JSON.parse(buildLabelTooltip(sampleLane)).footer, "共 2 个会话，点击查看明细");
    assert.equal(
      buildUnsummarizedSummary(sampleUnsummarized),
      "[Codex] · 5 条提问 · 12 步 · /tmp/demo。暂未生成摘要 —— 可分配到工作线或点击同步。",
    );
    assert.equal(
      buildUnsummarizedSummary({ ...sampleUnsummarized, cwd: "/tmp/price$&notes" }),
      "[Codex] · 5 条提问 · 12 步 · /tmp/price$&notes。暂未生成摘要 —— 可分配到工作线或点击同步。",
    );

    state.dataSource = "mock";
    state.days = [];
    state.lines = [sampleLine];
    state.unsummarized = [];
    ensureMockWindowDays(14);
    assert.equal(state.days.at(-1).full, "今天");
    assert.equal(state.days[0].full, "Nov 1 · 周日");

    state.contributions = sampleContrib;
    const contribZh = renderContributionPopoverBody();
    assert.ok(contribZh.includes("10月"), "zh contribution calendar missing month label");
    assert.ok(contribZh.includes("<div>一</div>") && contribZh.includes("<div>五</div>"));

    const popoverLayer = document.getElementById("popover-layer");
    state.popover = { type: "line-menu", lineId: "line-1", top: 40, left: 40 };
    renderPopoverLayer();
    for (const label of ["紧急", "高", "中", "低", "无优先级", "进行中", "暂停", "已完成"]) {
      assert.ok(popoverLayer.innerHTML.includes(label), `zh line-menu missing ${label}`);
    }

    state.popover = { type: "range-menu", top: 40, right: 320 };
    renderPopoverLayer();
    assert.ok(popoverLayer.innerHTML.includes("时间窗口"), "zh range-menu missing window label");

    // English locale checks
    state.settings.language = "en";
    assert.equal(localizeRecency("running"), "running");
    assert.equal(localizeRecency("just now"), "just now");
    assert.equal(localizeRecency("12m ago"), "12m ago");
    assert.equal(localizeRecency("3h ago"), "3h ago");
    assert.equal(localizeRecency("2d ago"), "2d ago");

    assert.equal(fmtSyncBatchSummary(1, 1), "1 session · 1 batch");
    assert.equal(fmtSyncBatchSummary(4, 2), "4 sessions · 2 batches");

    assert.equal(JSON.parse(buildLabelTooltip(sampleLane)).footer, "2 sessions. Click to see each one.");
    assert.equal(
      buildUnsummarizedSummary(sampleUnsummarized),
      "[Codex] · 5 user prompts · 12 steps · /tmp/demo. Not summarized yet — assign to a Line or run Sync.",
    );
    assert.equal(
      buildUnsummarizedSummary({ ...sampleUnsummarized, cwd: "/tmp/price$&notes" }),
      "[Codex] · 5 user prompts · 12 steps · /tmp/price$&notes. Not summarized yet — assign to a Line or run Sync.",
    );

    state.days = [];
    ensureMockWindowDays(14);
    assert.equal(state.days.at(-1).full, "Today");
    assert.equal(state.days[0].full, "Sun, Nov 1");

    const contribEn = renderContributionPopoverBody();
    assert.ok(contribEn.includes("Oct"), "en contribution calendar missing month label");
    assert.ok(contribEn.includes("<div>Mon</div>") && contribEn.includes("<div>Fri</div>"));

    state.popover = { type: "line-menu", lineId: "line-1", top: 40, left: 40 };
    renderPopoverLayer();
    for (const label of ["Urgent", "High", "Medium", "Low", "No priority", "Active", "Paused", "Done"]) {
      assert.ok(popoverLayer.innerHTML.includes(label), `en line-menu missing ${label}`);
    }

    state.popover = { type: "range-menu", top: 40, right: 320 };
    renderPopoverLayer();
    assert.ok(popoverLayer.innerHTML.includes("Window"), "en range-menu missing window label");
  } finally {
    state.popover = prevPopover;
    state.dataSource = prevDataSource;
    state.days = prevDays;
    state.selectedDayIdx = prevSelectedDayIdx;
    state.contributions = prevContributions;
    state.lines = prevLines;
    state.unsummarized = prevUnsummarized;
    state.settings.language = prevLang;
  }
});
