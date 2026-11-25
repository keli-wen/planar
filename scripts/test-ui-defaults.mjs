import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const DEFAULTS_PATH = fileURLToPath(new URL("../ui/defaults.json", import.meta.url));
const DEFAULTS_RAW = readFileSync(DEFAULTS_PATH, "utf8");
const DEFAULTS_JSON = JSON.parse(DEFAULTS_RAW);

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
  buildSyncButtonTooltip,
  DEFAULT_SETTINGS,
  fmtSyncUsageCompact,
  isProcessorConfigured,
  loadSyncEstimate,
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
