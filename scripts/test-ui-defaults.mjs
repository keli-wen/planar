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

const { applyPlaneResponse, DEFAULT_SETTINGS, isProcessorConfigured, state } = await import(
  "../ui/app.js"
);

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
  const deepseekSpec = DEFAULTS_JSON.apiProviders.find((p) => p.id === "deepseek");

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
