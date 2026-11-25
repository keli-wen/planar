// Clean-room synthetic mock datasets for `?mock=1` preview and initial offline paint.
// Pure zero-DOM leaf module shared by `ui/app.js` and offline unit tests.

const MOCK_DAYS = [
  { idx: 0, isoDate: "2026-11-08", short: "Sun 8", tiny: "Sun", full: "Sun, Nov 8" },
  { idx: 1, isoDate: "2026-11-09", short: "Mon 9", tiny: "Mon", full: "Mon, Nov 9" },
  { idx: 2, isoDate: "2026-11-10", short: "Tue 10", tiny: "Tue", full: "Tue, Nov 10" },
  { idx: 3, isoDate: "2026-11-11", short: "Wed 11", tiny: "Wed", full: "Wed, Nov 11" },
  { idx: 4, isoDate: "2026-11-12", short: "Thu 12", tiny: "Thu", full: "Thu, Nov 12" },
  { idx: 5, isoDate: "2026-11-13", short: "Fri 13", tiny: "Fri", full: "Fri, Nov 13" },
  { idx: 6, isoDate: "2026-11-14", short: "Sat 14", tiny: "Sat", full: "Today", isToday: true },
];

export function buildMockContributions() {
  const out = [];
  const end = new Date("2026-11-14T12:00:00Z");
  const start = new Date(end.getTime() - 139 * 86400000);
  // Align first column cleanly to Monday
  const dow = (start.getUTCDay() + 6) % 7;
  start.setUTCDate(start.getUTCDate() - dow);
  for (let d = new Date(start); d <= end; d.setUTCDate(d.getUTCDate() + 1)) {
    const idx = out.length;
    const iso = d.toISOString().slice(0, 10);
    const wd = (d.getUTCDay() + 6) % 7;
    const active = (idx * 17 + wd * 5) % 10 < 7;
    const claude = active ? (idx * 13 + 7) % 55 : 0;
    const codex = active ? (idx * 19 + 11) % 70 : 0;
    const agy = active && idx % 3 !== 0 ? (idx * 23 + 3) % 48 : 0;
    const dsh = active && idx % 4 === 0 ? (idx * 11 + 9) % 36 : 0;
    const totalMinutes = claude + codex + agy + dsh;
    const byHarness = {};
    const tokensByHarness = {};
    const costByHarness = {};
    if (agy > 0) {
      byHarness.antigravity = agy;
      tokensByHarness.antigravity = agy * 86000;
    }
    if (claude > 0) {
      byHarness.claude = claude;
      tokensByHarness.claude = claude * 54000;
      costByHarness.claude = Number((claude * 0.042).toFixed(3));
    }
    if (codex > 0) {
      byHarness.codex = codex;
      tokensByHarness.codex = codex * 68000;
      costByHarness.codex = Number((codex * 0.031).toFixed(3));
    }
    if (dsh > 0) {
      byHarness.dsh = dsh;
      tokensByHarness.dsh = dsh * 62000;
      costByHarness.dsh = Number((dsh * 0.008).toFixed(3));
    }
    const totalTokens = Object.values(tokensByHarness).reduce((a, b) => a + b, 0);
    const costUsd = Number(
      Object.values(costByHarness)
        .reduce((a, b) => a + b, 0)
        .toFixed(3)
    );
    out.push({
      date: iso,
      weekday: wd,
      totalMinutes,
      sessionCount: totalMinutes > 0 ? Math.max(1, Math.min(8, Math.round(totalMinutes / 26))) : 0,
      byHarness,
      totalTokens,
      costUsd,
      tokensByHarness,
      costByHarness,
    });
  }
  return out;
}

export const DEFAULT_MOCK_CONTRIBUTIONS = buildMockContributions();

export const DEFAULT_MOCK_HARNESSES = [
  { id: "antigravity", enabled: true, cliAvailable: true, cliPath: "~/.local/bin/agy", logRoot: "~/.gemini/antigravity/brain", logRootExists: true, sessionCount: 5 },
  { id: "claude", enabled: true, cliAvailable: true, cliPath: "/opt/homebrew/bin/claude", logRoot: "~/.claude/projects", logRootExists: true, sessionCount: 6 },
  { id: "codex", enabled: true, cliAvailable: true, cliPath: "/opt/homebrew/bin/codex", logRoot: "~/.codex/sessions", logRootExists: true, sessionCount: 6 },
  { id: "dsh", enabled: true, cliAvailable: false, cliPath: null, logRoot: "~/.dsh/sessions", logRootExists: false, sessionCount: 4 },
];

export function createMockPlane(palette) {
  if (!Array.isArray(palette) || palette.length < 5) {
    throw new TypeError("createMockPlane requires a palette array with at least 5 colors");
  }

  return {
    days: MOCK_DAYS.map((d) => ({ ...d })),
    archivedCount: 11,
    contributions: buildMockContributions(),
    harnesses: DEFAULT_MOCK_HARNESSES.map((h) => ({ ...h })),
    // bursts: [dayIdx, startHour, endHour, promptHours[], activeMinutes]
    lines: [
      {
        id: "sqlite-sync",
        name: "sqlite-sync",
        description: "WAL incremental log ingestion & idempotent receipt cursors",
        color: palette[0],
        pinned: true,
        priority: 1,
        status: "active",
        sessions: [
          {
            id: "ss1",
            harness: "codex",
            model: "gpt-5.4",
            totalTokens: 14820000,
            costUsd: 4.86,
            title: "SQLite WAL Cursor Sync and Idempotent Append Engine",
            rawTitle: "Finalize byte-offset cursor schema and WAL busy_timeout stress tests",
            steps: 395,
            recency: "running",
            running: true,
            stoppedAt: "Running multi-process SQLite WAL read/write stress tests; byte-offset resume and crash-recovery unit tests all pass.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [0, 3.2, 6.8, [3.6, 6.1], 18],
              [2, 5.4, 18.6, [6.2, 11.4, 16.8], 42],
              [3, 10.2, 18.8, [10.8, 15.6], 34],
              [4, 2.4, 8.6, [2.5, 4.9, 7.8], 38],
              [4, 9.15, 16.9, [9.4, 13.1, 14.4, 16.4], 48],
              [5, 9.6, 18.4, [10.2, 15.8], 36],
            ],
          },
          {
            id: "ss2",
            harness: "claude",
            model: "claude-sonnet-4-6",
            totalTokens: 2940000,
            costUsd: 1.62,
            title: "Cross-Harness JSONL Schema & Token Normalization Audit",
            rawTitle: "Compare streaming token accounting across Claude Code, Codex, and DSH logs",
            steps: 162,
            recency: "1d ago",
            running: false,
            stoppedAt: "Verified streaming chunk deduplication for Claude usage blocks and cumulative token_count handling for Codex rollouts.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [2, 9.0, 14.8, [9.5, 13.8], 26],
              [4, 12.2, 15.2, [12.4, 14.8], 24],
              [5, 12.4, 16.2, [12.9, 15.6], 22],
            ],
          },
          {
            id: "ss3",
            harness: "antigravity",
            model: null,
            totalTokens: 6410000,
            costUsd: null,
            title: "FTS5 BM25 Ranking and CJK Substring Recall Study",
            rawTitle: "Benchmark SQLite FTS5 unicode61 tokenization with mixed English and CJK queries",
            steps: 218,
            recency: "2d ago",
            running: false,
            stoppedAt: "Benchmarked FTS5 unicode61 AND/OR fallback combined with case-insensitive substring matching for CJK titles and file paths.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [2, 15.2, 18.2, [15.5, 17.6], 22],
              [3, 13.2, 17.0, [13.6, 16.4], 20],
            ],
          },
        ],
      },
      {
        id: "planar",
        name: "Planar",
        description: "2D work plane core, zero-fork UI, and read-only MCP server",
        color: palette[1],
        pinned: true,
        priority: 2,
        status: "active",
        sessions: [
          {
            id: "p1",
            harness: "antigravity",
            model: null,
            totalTokens: 5180000,
            costUsd: null,
            title: "Planar: Morphing Live Indicator & Continuous Band Swells",
            rawTitle: "Implement C1 triangular-window bandPath smoothing and MD3 running dot",
            steps: 326,
            recency: "running",
            running: true,
            stoppedAt: "Landed closed-form triangular-window thickness smoothing and MD3 morphing indicator; verifying narrow sidebar breakpoints.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [4, 9.8, 12.1, [10.0, 10.6, 11.2, 11.8], 34],
              [4, 12.62, 16.85, [12.74, 13.15, 13.95, 15.4, 16.6], 46],
              [5, 9.4, 18.2, [10.2, 14.6, 17.4], 48],
            ],
          },
          {
            id: "p2",
            harness: "claude",
            model: "claude-sonnet-4-6",
            totalTokens: 3420000,
            costUsd: 1.94,
            title: "Head–Middle–Tail Slicer & Sync / Regroup Boundaries",
            rawTitle: "Design 3-zone trajectory slicer and slice_config_hash cache invalidation",
            steps: 261,
            recency: "1d ago",
            running: false,
            stoppedAt: "Finalized Opening 20 + Middle User Prompts + Closing 40 slicing and locked incremental Sync so pinned Lines stay immutable.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [4, 14.3, 16.72, [14.5, 15.12, 15.67, 16.2, 16.62], 38],
              [5, 11.8, 15.8, [12.1, 13.8, 15.2], 36],
            ],
          },
          {
            id: "p3",
            harness: "codex",
            model: "gpt-5.4",
            totalTokens: 4860000,
            costUsd: 1.58,
            title: "Planar: Read-Only MCP Server & Batch L1/L2 Disclosure",
            rawTitle: "Implement MCP 2025-03-26 over Stdio and Streamable HTTP /mcp",
            steps: 359,
            recency: "1d ago",
            running: false,
            stoppedAt: "Wired planar_status, batch planar_line, bounded planar_session, and FTS5 planar_search with 1:1 CLI Markdown parity; all 33 core tests pass.",
            leftOpen: "Verify Streamable HTTP /mcp reconnection after Planar.app restarts on a fallback port.",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [3, 10.2, 18.8, [10.8, 15.2, 18.1], 58],
              [4, 12.9, 15.8, [13.05, 13.3, 14.75, 15.55], 44],
            ],
          },
          {
            id: "p4",
            harness: "codex",
            model: "gpt-5.4",
            totalTokens: 1640000,
            costUsd: 0.52,
            title: "Active Agent Time vs Wall Clock Burst Union",
            rawTitle: "Compute active agent duration by clamping idle gaps at 12 minutes",
            steps: 144,
            recency: "2d ago",
            running: false,
            stoppedAt: "Implemented clamped burst union (12m idle cap, 60s minimum floor) and dual agent time vs wall clock tooltips on parent lanes.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [3, 12.8, 16.4, [13.2, 15.9], 32],
            ],
          },
          {
            id: "p5",
            harness: "antigravity",
            model: null,
            totalTokens: 920000,
            costUsd: null,
            title: "24x24 Sixths Geometric Icon & OKLab Palette Validation",
            rawTitle: "Design 24x24 rounded-square vector mark and CVD-safe categorical hues",
            steps: 98,
            recency: "2d ago",
            running: false,
            stoppedAt: "Finalized rx=4 tangent-aligned 3-plane SVG icon and verified >= 3:1 light/dark contrast across all 6 Line hues.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [[5, 16.0, 18.0, [16.3, 17.6], 24]],
          },
        ],
      },
      {
        id: "web-kit",
        name: "web-kit",
        color: palette[2],
        pinned: false,
        priority: 0,
        status: "active",
        sessions: [
          {
            id: "w1",
            harness: "dsh",
            model: "deepseek-chat",
            totalTokens: 3850000,
            costUsd: 0.34,
            title: "Container-Query Sidebar Layout & Scroll Stability",
            rawTitle: "Eliminate layout jitter during expand/collapse with width-only ResizeObserver",
            steps: 284,
            recency: "1d ago",
            running: false,
            stoppedAt: "Added scrollbar-gutter reservation and @container breakpoints down to 320px; zero horizontal jump on card toggle.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [4, 9.8, 12.0, [10.1, 11.4], 42],
              [4, 12.52, 16.5, [12.8, 14.9, 16.1], 52],
              [5, 8.8, 20.6, [9.4, 14.6, 19.8], 74],
            ],
          },
          {
            id: "w2",
            harness: "codex",
            model: "gpt-5.4",
            totalTokens: 2290000,
            costUsd: 0.78,
            title: "Keyboard Shortcuts & Accessible Popover Focus Ring",
            rawTitle: "Wire 1-4/0 priority hotkeys and Escape dismissal for line menus",
            steps: 176,
            recency: "2d ago",
            running: false,
            stoppedAt: "Implemented keyboard priority shortcuts and click-outside popover cleanup across Week and Day views.",
            leftOpen: "Add arrow-key navigation inside the upstream session search picker.",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [2, 10.6, 19.0, [11.2, 16.8], 56],
              [5, 11.6, 16.8, [12.1, 16.0], 38],
            ],
          },
          {
            id: "w3",
            harness: "antigravity",
            model: null,
            totalTokens: 4120000,
            costUsd: null,
            title: "Dark Theme Surface Tokens & Heatmap Contrast Sweep",
            rawTitle: "Calibrate dark canvas elevation and 5-step contribution cell luminance",
            steps: 310,
            recency: "3d ago",
            running: false,
            stoppedAt: "Tuned #121418 canvas contrast and automatic system theme switching without flash.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [[2, 12.8, 16.8, [13.2, 16.2], 34]],
          },
          {
            id: "w4",
            harness: "claude",
            model: "claude-sonnet-4-6",
            totalTokens: 1460000,
            costUsd: 0.84,
            title: "Zero-Build ESM Module Hygiene & Biome Lint Gate",
            rawTitle: "Configure Biome strict rules for vanilla JS frontend and preview scripts",
            steps: 128,
            recency: "4d ago",
            running: false,
            sourceExists: false,
            stoppedAt: "Enabled Biome noUnusedVariables and node --check syntax gate in npm run lint.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [[4, 13.4, 15.8, [13.8, 15.2], 22]],
          },
        ],
      },
      {
        id: "search-eval",
        name: "search-eval",
        color: palette[3],
        pinned: false,
        priority: 0,
        status: "active",
        sessions: [
          {
            id: "e1",
            harness: "dsh",
            model: "deepseek-chat",
            totalTokens: 5620000,
            costUsd: 0.49,
            title: "Hybrid BM25 + Vector Reciprocal Rank Fusion Benchmark",
            rawTitle: "Run controlled RRF fusion ablations across public BEIR retrieval subsets",
            steps: 312,
            recency: "2d ago",
            running: false,
            stoppedAt: "Completed 6 controlled RRF weight sweeps on held-out dev sets; nDCG@10 improved by +3.4 pts with sub-18ms p95 latency.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [2, 10.2, 19.2, [10.8, 15.8, 18.4], 54],
              [2, 22.0, 24.0, [22.4, 23.6], 18],
              [3, 0.0, 1.8, [0.4, 1.4], 12],
              [3, 5.4, 21.8, [6.2, 12.8, 19.6], 82],
              [4, 0.35, 0.42, [0.38], 3],
              [4, 2.45, 2.52, [2.48], 3],
              [4, 8.2, 11.8, [8.6, 11.2], 18],
            ],
          },
          {
            id: "e2",
            harness: "claude",
            model: "claude-sonnet-4-6",
            totalTokens: 2780000,
            costUsd: 1.56,
            title: "Evaluation Fixture Drift Detector & Coverage Report",
            rawTitle: "Filter stale fixture URLs and automate weekly recall baseline diffs",
            steps: 189,
            recency: "2d ago",
            running: false,
            stoppedAt: "Added schema validation for golden eval sets and automated markdown diff summaries in CI.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [2, 13.0, 17.8, [13.5, 17.2], 26],
              [3, 7.8, 13.8, [8.4, 13.1], 32],
            ],
          },
        ],
      },
      {
        id: "one-off",
        name: "One-off",
        color: palette[4],
        pinned: true,
        isSystemBucket: true,
        priority: 0,
        status: "active",
        sessions: [
          {
            id: "o1",
            harness: "claude",
            model: "claude-haiku-4-5",
            totalTokens: 420000,
            costUsd: 0.14,
            title: "Fast Summarizer Cost and Throughput Comparison",
            rawTitle: "Compare Flash Lite, Haiku, and Mini costs on 2.5k-token sliced trajectories",
            steps: 46,
            recency: "1d ago",
            running: false,
            stoppedAt: "Verified 100 batched Head–Middle–Tail digests stay under $0.03 across lightweight model tiers.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [
              [4, 12.8, 16.4, [13.0, 15.8], 13],
            ],
          },
          {
            id: "o2",
            harness: "codex",
            model: "gpt-5-mini",
            totalTokens: 510000,
            costUsd: 0.09,
            title: "Local Loopback Port Fallback & CORS Diagnostics",
            rawTitle: "Verify Axum fallback to ephemeral port when 4310 is already bound",
            steps: 58,
            recency: "2d ago",
            running: false,
            stoppedAt: "Confirmed Tauri loopback binding cleanly falls back to 127.0.0.1:0 when port 4310 is occupied.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [[3, 12.4, 16.2, [12.9, 15.8], 9]],
          },
          {
            id: "o3",
            harness: "dsh",
            model: "deepseek-chat",
            totalTokens: 290000,
            costUsd: 0.03,
            title: "Markdown Broken-Link & Anchor Checker Script",
            rawTitle: "Write a quick Node script to validate relative doc links before release",
            steps: 34,
            recency: "4d ago",
            running: false,
            stoppedAt: "Verified zero broken relative links across README.md, AGENTS.md, and contexts/.",
            leftOpen: "",
            dependsOnId: null,
            hidden: false,
            bursts: [[2, 10.4, 14.2, [10.8, 13.6], 6]],
          },
        ],
      },
    ],
    unsummarized: [
      {
        id: "u1",
        harness: "antigravity",
        model: null,
        totalTokens: 1840000,
        costUsd: null,
        firstPrompt: "Polish Agent-Native docs, CLI sync/open commands, and release hygiene for Planar v0.1",
        resolvedTitle: "Planar v0.1 Agent-Native Docs & CLI Sync/Open Polish",
        targetLineId: "planar",
        steps: 84,
        recency: "running",
        running: true,
        bursts: [
          [5, 11.2, 23.95, [11.8, 17.4, 22.8], 34],
          [6, 0.0, 1.75, [0.4, 1.5], 10],
        ],
      },
      {
        id: "u2",
        harness: "dsh",
        model: "deepseek-chat",
        totalTokens: 640000,
        costUsd: 0.06,
        firstPrompt: "Draft release checklist for Cargo workspace metadata and Homebrew formula",
        resolvedTitle: "Release Checklist: Cargo Metadata & Packaging",
        targetLineId: "one-off",
        steps: 52,
        recency: "5h ago",
        running: false,
        bursts: [[5, 19.6, 22.8, [19.9, 22.1], 12]],
      },
    ],
    shortSessions: {
      totalCount: 12,
      items: [
        {
          id: "s1",
          harness: "codex",
          model: "gpt-5-mini",
          totalTokens: 28000,
          costUsd: 0.01,
          prompt: "How to inspect video stream codecs quickly with ffprobe?",
          recency: "4m ago",
          dayIdx: 6,
          mins: 1,
          bursts: [[6, 0.05, 0.09, [0.05], 1]],
        },
        {
          id: "s2",
          harness: "claude",
          model: "claude-haiku-4-5",
          totalTokens: 46000,
          costUsd: 0.02,
          prompt: "macOS sips command to resize PNG app icons cleanly",
          recency: "1h ago",
          dayIdx: 5,
          mins: 6,
          bursts: [[5, 20.8, 20.92, [20.8], 6]],
        },
        {
          id: "s3",
          harness: "dsh",
          model: "deepseek-chat",
          totalTokens: 61000,
          costUsd: 0.01,
          prompt: "Recommended SQLite WAL busy_timeout for concurrent CLI + daemon reads",
          recency: "1d ago",
          dayIdx: 4,
          mins: 9,
          bursts: [[4, 15.1, 15.26, [15.1], 9]],
        },
        {
          id: "s4",
          harness: "antigravity",
          model: null,
          totalTokens: 54000,
          costUsd: null,
          prompt: "GitHub Flavored Markdown alert callout nesting rules",
          recency: "2d ago",
          dayIdx: 3,
          mins: 8,
          bursts: [[3, 11.3, 11.44, [11.3], 8]],
        },
      ],
      weekMinutes: 67,
      weekTokens: 485000,
      weekCostUsd: 0.14,
    },
  };
}
