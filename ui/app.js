// Planar — 1:1 Interactive UI Reproduction
// Implements:
// 1. Continuous thickness-encoded timeline bands (`bandProfile` + `bandPath`):
//    - Concurrency half-thickness: 0 / 2.5 / 3.5 / 4.5 px for 0 / 1 / 2 / 3+ overlapping sessions
//    - Sub-2.5px gap bridging (`BAND_JOIN_PX = 2.5`) to prevent pinched waists
//    - Closed-form triangular-window smoothing (`BAND_EASE_PX = 5`) via double integral G(x)
//    - Adaptive 1px transition sampling + semicircular SVG `A` end caps in uniform solid `--hue`
// 2. Material Design 3 Expressive morphing inner dot for live running sessions
// 3. Flush vertical alignment between Line swatch and Session indicator (only dependent children indent)
// 4. Ascending 3-step staircase Priority icons (including white staircase inside Urgent badge)
// 5. Zero-jitter scrollbar reservation + synchronous plot measurement + full responsive layout down to 320px

async function loadSharedDefaults() {
  const url = new URL("./defaults.json", import.meta.url);
  const res = await fetch(url, { cache: "no-store" });
  if (!res.ok) {
    throw new Error(`Failed to load ${url.pathname}: HTTP ${res.status}`);
  }
  return res.json();
}

const SHARED_DEFAULTS = await loadSharedDefaults();
const PALETTE = SHARED_DEFAULTS.palette;
const DARK_PALETTE = SHARED_DEFAULTS.darkPalette;
const HARNESS_ORDER = SHARED_DEFAULTS.harnessOrder;
const CLI_ENGINE_ORDER = SHARED_DEFAULTS.cliEngineOrder;
const API_PROVIDERS = SHARED_DEFAULTS.apiProviders;
const SHORT_THRESHOLD_OPTIONS = SHARED_DEFAULTS.shortThresholdOptions;

function defaultApiModelForProvider(providerId) {
  const pid = typeof providerId === "string" ? providerId.trim() : "";
  if (!pid) return "";
  return API_PROVIDERS.find((p) => p.id.toLowerCase() === pid.toLowerCase())?.defaultModel || "";
}

const DEFAULT_SETTINGS = {
  ...SHARED_DEFAULTS.defaultSettings,
  apiModel: defaultApiModelForProvider(SHARED_DEFAULTS.defaultSettings.apiProvider),
};

const LANG_OPTIONS = [
  { id: "zh", label: "简体中文", code: "zh-CN" },
  { id: "en", label: "English", code: "en-US" },
];

const HARNESS_META = {
  antigravity: {
    id: "antigravity",
    labelEn: "Antigravity",
    labelZh: "Antigravity",
    fullLabelEn: "Antigravity",
    fullLabelZh: "Antigravity",
    cli: "agy",
    path: "M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z",
  },
  claude: {
    id: "claude",
    labelEn: "Claude Code",
    labelZh: "Claude Code",
    fullLabelEn: "Claude Code",
    fullLabelZh: "Claude Code",
    cli: "claude",
    path: "M20.998 10.949H24v3.102h-3v3.028h-1.487V20H18v-2.921h-1.487V20H15v-2.921H9V20H7.488v-2.921H6V20H4.487v-2.921H3V14.05H0V10.95h3V5h17.998v5.949zM6 10.949h1.488V8.102H6v2.847zm10.51 0H18V8.102h-1.49v2.847z",
  },
  codex: {
    id: "codex",
    labelEn: "Codex",
    labelZh: "Codex",
    fullLabelEn: "Codex",
    fullLabelZh: "Codex",
    cli: "codex",
    path: "M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z",
  },
  dsh: {
    id: "dsh",
    labelEn: "DSH",
    labelZh: "DSH",
    fullLabelEn: "DeepSeek Harness",
    fullLabelZh: "DeepSeek Harness",
    cli: "dsh",
    path: "M23.748 4.482c-.254-.124-.364.113-.512.234-.051.039-.094.09-.137.136-.372.397-.806.657-1.373.626-.829-.046-1.537.214-2.163.848-.133-.782-.575-1.248-1.247-1.548-.352-.156-.708-.311-.955-.65-.172-.241-.219-.51-.305-.774-.055-.16-.11-.323-.293-.35-.2-.031-.278.136-.356.276-.313.572-.434 1.202-.422 1.84.027 1.436.633 2.58 1.838 3.393.137.093.172.187.129.323-.082.28-.18.552-.266.833-.055.179-.137.217-.329.14a5.526 5.526 0 01-1.736-1.18c-.857-.828-1.631-1.742-2.597-2.458a11.365 11.365 0 00-.689-.471c-.985-.957.13-1.743.388-1.836.27-.098.093-.432-.779-.428-.872.004-1.67.295-2.687.684a3.055 3.055 0 01-.465.137 9.597 9.597 0 00-2.883-.102c-1.885.21-3.39 1.102-4.497 2.623C.082 8.606-.231 10.684.152 12.85c.403 2.284 1.569 4.175 3.36 5.653 1.858 1.533 3.997 2.284 6.438 2.14 1.482-.085 3.133-.284 4.994-1.86.47.234.962.327 1.78.397.63.059 1.236-.03 1.705-.128.735-.156.684-.837.419-.961-2.155-1.004-1.682-.595-2.113-.926 1.096-1.296 2.746-2.642 3.392-7.003.05-.347.007-.565 0-.845-.004-.17.035-.237.23-.256a4.173 4.173 0 001.545-.475c1.396-.763 1.96-2.015 2.093-3.517.02-.23-.004-.467-.247-.588zM11.581 18c-2.089-1.642-3.102-2.183-3.52-2.16-.392.024-.321.471-.235.763.09.288.207.486.371.739.114.167.192.416-.113.603-.673.416-1.842-.14-1.897-.167-1.361-.802-2.5-1.86-3.301-3.307-.774-1.393-1.224-2.887-1.298-4.482-.02-.386.093-.522.477-.592a4.696 4.696 0 011.529-.039c2.132.312 3.946 1.265 5.468 2.774.868.86 1.525 1.887 2.202 2.891.72 1.066 1.494 2.082 2.48 2.914.348.292.625.514.891.677-.802.09-2.14.11-3.054-.614zm1-6.44a.306.306 0 01.415-.287.302.302 0 01.2.288.306.306 0 01-.31.307.303.303 0 01-.304-.308zm3.11 1.596c-.2.081-.399.151-.59.16a1.245 1.245 0 01-.798-.254c-.274-.23-.47-.358-.552-.758a1.73 1.73 0 01.016-.588c.07-.327-.008-.537-.239-.727-.187-.156-.426-.199-.688-.199a.559.559 0 01-.254-.078c-.11-.054-.2-.19-.114-.358.028-.054.16-.186.192-.21.356-.202.767-.136 1.146.016.352.144.618.408 1.001.782.391.451.462.576.685.914.176.265.336.537.445.848.067.195-.019.354-.25.452z",
  },
};

const CLI_AUTO_ORDER_TEXT = CLI_ENGINE_ORDER.map((id) => HARNESS_META[id]?.cli || id).join(" → ");
const API_MODEL_EXAMPLES = ["gemini", "anthropic", "deepseek"]
  .map((id) => API_PROVIDERS.find((p) => p.id === id)?.defaultModel)
  .filter(Boolean);

const I18N = {
  zh: {
    navPlane: "工作平面",
    navSettings: "设置",
    liveAuto: "自动更新",
    sync: "归纳",
    syncIdle: "同步",
    syncing: "归纳中...",
    syncTip: "对新轨迹执行首中尾切片摘要并归入工作线",
    syncBannerRunning: "正在执行首-中-尾切片归线 · 已完成 {done} / {total}",
    syncBannerBatched: "批量切片归线 · 第 {batch} / {batches} 批 · 已完成 {done} / {total}",
    syncStopAfterBatch: "本批后停止",
    syncStopping: "本批后停止（再点立即终止）",
    syncBatchActiveTag: "本批 {n} 条",
    syncBatchMore: "+{n} 条同批",
    syncFiledInto: "已归入 {line}",
    syncFiledMore: "+{n} 条",
    syncFailedBadge: "{n} 异常",
    allHarnesses: "全部来源",
    allShort: "全部",
    prefsTheme: "外观主题",
    prefsLang: "语言",
    prefsShortLane: "短会话泳道",
    prefsSource: "轨迹来源",
    themeAutoShort: "自动",
    autoSaveReady: "自动保存",
    savedJustNow: "已保存",
    closeBtn: "返回",
    modeDay: "日",
    modeWeek: "周",
    modeMonth: "月",
    range7: "近 7 天",
    range14: "近 14 天",
    range30: "近 30 天",
    today: "今天",
    contribTitle: "20 周活跃热力图",
    contribHide: "收起",
    contribShow: "展开",
    contribBackToday: "回到今天",
    contribLess: "少",
    contribMore: "多",
    contribNoActivity: "无记录",
    contribSessions: "个会话",
    contribDaysActive: "个活跃日",
    agentTimeLast: "近 {n} 天 Agent 活跃时长",
    agentTimeToday: "今日 Agent 活跃时长",
    agentTimeOn: "{date} Agent 活跃时长",
    agentTimeTip: "有效工作片段并集（自动剔除 > 12 分钟空闲等待）",
    unsummarized: "待摘要会话",
    shortSessions: "短会话",
    noShort: "当前时间窗无短会话",
    recap: "摘要 RECAP",
    trajPreview: "轨迹预览",
    summary: "摘要",
    stoppedAt: "推进至",
    leftOpen: "待跟进",
    lineLabel: "工作线 Line",
    unassignedLine: "未归线 · 指定工作线",
    dependsOn: "上游依赖",
    linkUpstream: "+ 关联上游会话...",
    unlinkHint: "点击解除上游依赖",
    hideBtn: "隐藏",
    openBtn: "打开",
    stepsWord: "步",
    lineMenuHint: "优先级、状态、重命名、合并",
    moreIdle: "另有 {n} 个会话在{when}无活动",
    showFewer: "收起空闲会话",
    idleGroupsToggle: "另有 {n} 条工作线在{when}无活动",
    hideIdleGroupsToggle: "收起在{when}无活动的工作线",
    todayWhen: "今天",
    onWhen: "{date}",
    lineSingle: "条线",
    linePlural: "条线",
    sessionSingle: "个会话",
    sessionPlural: "个会话",
    linesTitle: "工作线",
    priority: "优先级",
    status: "状态",
    renameLine: "重命名...",
    renamePrompt: "重命名工作线：",
    searchSessions: "搜索会话...",
    otherLines: "其他工作线",
    moveSessionTitle: "移动到工作线",
    tabGeneral: "外观与偏好",
    tabProcessor: "摘要引擎",
    tabHarnesses: "轨迹源",
    tabLines: "工作线与切片",
    secAppearance: "外观与语言",
    lblLang: "界面语言",
    descLang: "切换中文或英文界面；新生成的会话摘要与归线名称也会同步匹配该语言。",
    lblTheme: "外观主题",
    descTheme: "支持跟随 macOS 系统、浅色与深色模式，泳道色带自动切换高对比度调色盘。",
    themeSystem: "跟随系统",
    themeLight: "浅色",
    themeDark: "深色",
    secAutoDashboard: "工作平面与时间轴",
    lblContrib: "在窗口菜单显示 20 周活跃日历",
    descContrib: "在右上角时间窗口弹层中展示 20 周每日活跃度分布，支持点击任意日期回看历史。",
    lblAutoSync: "自动归纳周期",
    descAutoSync: "后台增量扫描并归纳新输入会话，仅归入既有工作线而不擅自改名或合并。",
    optEveryDay: "每天",
    optEveryHour: "每小时",
    optManual: "仅手动",
    lblShowShort: "在时间轴显示短会话",
    descShowShort: "将少于 {n} 次用户输入的一次性问答作为独立泳道显示。",
    lblIncludeArchived: "包含已归档会话",
    descIncludeArchived: "当前区间共有 {n} 个已归档会话未计入。",
    lblPerLine: "每条工作线默认展示会话数",
    descPerLine: "超出部分折叠展示。",
    resetDefaults: "恢复默认设置",
    resetConfirm: "再点一次确认恢复",
    resetKept: "不会删除你已固定或建立的工作线。",
    procTitle: "轨迹摘要引擎",
    procDesc: "决定由谁执行「首-中-尾切片 → 摘要 + 归线」。可直接复用本机已登录的 CLI，或配置直连模型 API。",
    procCliTitle: "复用本机 CLI",
    procCliDesc: "调用本机已登录的 Claude / Codex / Antigravity / DSH 单轮无痕命令，无需配置 API Key。",
    procApiTitle: "直连模型 API",
    procApiDesc: "通过 HTTPS 直接调用 Gemini / Anthropic / OpenAI / DeepSeek 或本地兼容网关（如 Ollama）。",
    cliAutoChip: "自动挑选",
    cliMissingHint: "本机 PATH 未检测到 {cli}，可安装登录或在下方指定绝对路径。",
    cliAdvToggle: "高级：指定可执行文件路径",
    keyShow: "显示",
    keyHide: "隐藏",
    keyLocalNote: "密钥仅保存在本机 ~/.planar，不会上传到任何第三方服务。",
    lblCliHarness: "首选 CLI 引擎",
    descCliHarness: `自动模式将按已就绪顺序挑选 ${CLI_AUTO_ORDER_TEXT}。`,
    lblCliModel: "模型名称",
    descCliModel: "留空使用该 CLI 默认推荐轻量模型。",
    lblCliBin: "可执行文件路径",
    descCliBin: "仅当工具不在标准 PATH 目录时填写。",
    lblApiProvider: "接口协议",
    descApiProvider: "选择服务商协议与默认端点。",
    apiCompatShort: "本地 / 兼容网关",
    lblApiUrl: "Base URL",
    descApiUrl: "本地或兼容网关必填；官方端点留空即可。",
    lblApiModel: "模型名称",
    descApiModel: `如 ${API_MODEL_EXAMPLES.join("、")}。`,
    descApiModelGemini: "默认 {defaultModel}；追求极速与最低成本可选 {fastModel}。",
    lblApiKey: "API Key",
    descApiKey: "留空时自动尝试读取对应环境变量（如 DEEPSEEK_API_KEY）。",
    lblBatchLimit: "单批归纳会话上限",
    descBatchLimit: "控制每批处理会话数（1–50），大批量待归纳时将逐批更新平面。",
    testProcBtn: "测试连接",
    testingProcBtn: "正在连接测试…",
    syncNowBtn: "立即归纳",
    harnessesTitle: "多端轨迹源与日志目录",
    harnessesDesc: "Planar 对以下本地目录严格采用只读流式增量读取，绝不修改、锁定或包裹源转录文件。",
    cliReady: "CLI 就绪",
    cliMissing: "未安装 CLI",
    dirMissing: "目录未创建",
    editPathBtn: "自定义目录…",
    collapsePathBtn: "收起",
    resetPathBtn: "恢复默认",
    pathPlaceholder: "输入自定义日志根目录路径（留空使用默认）",
    rescanBtn: "重新扫描目录",
    rescanningBtn: "扫描中…",
    pinnedLinesTitle: "固定工作线",
    pinnedLinesDesc: "固定的工作线排在最前，且即使当日暂无会话也在时间轴保留专属泳道。点击色块可更换配色。",
    pickLineColor: "选择工作线颜色",
    unpinBtn: "取消固定",
    emptyPinnedHint: "尚无固定工作线，可在下方输入项目名创建，或将自动发现的工作线固定下来。",
    linePlaceholder: "输入项目或长期工作线名称…",
    addLineBtn: "添加工作线",
    discoveredTitle: "Planar 自动发现的工作线",
    discoveredDesc: "按下「重新归类」时会重新聚类合并未固定的工作线；点击「固定」即可锁定为你自己的长期工作线。",
    emptyDiscoveredHint: "当前暂无未固定的自动工作线；运行「归纳」后新聚类的工作线会显示在这里。",
    regroupBtn: "重新归类",
    regroupingBtn: "正在归类…",
    keepBtn: "固定",
    slicerHeading: "首–中–尾轨迹切片窗口（Head–Middle–Tail）",
    slicerDragHint: "拖动分界滑块调节保留步数",
    slicerFirst: "开头 {n} 步（完整对话）",
    slicerLast: "结尾 {n} 步（完整对话）",
    slicerMidCaption: "中段跳过冗长回复 — 仅保留你的提示词节点 (●)，工具输出始终剔除",
    slicerOpenLbl: "开头保留步数",
    slicerCloseLbl: "结尾保留步数",
    slicerDesc: "首尾保留完整上下文；中间区段仅保留用户输入以防中途跑题丢失；工具长输出始终剔除。",
    lblFewestInputs: "最少用户输入阈值",
    descFewestInputs: "低于该次数的会话直接用首句提示展示，达到后再触发摘要。",
    lblMaxUserChars: "单条用户输入最大字符数",
    descMaxUserChars: "超过则截断；填 0 表示不限制。",
    lblMaxAgentChars: "单条 Agent 回复最大字符数",
    descMaxAgentChars: "超过则截断；填 0 表示不限制。",
    charsUnit: "字符",
    slicerFoot: "修改切片阈值后，下次同步会自动重算受影响的旧摘要。",
    toastSynced: "已同步：处理 {n} 个会话（引擎 {backend}）",
    toastSyncedWithFail: "已同步 {n} 个会话（{failed} 个异常跳过，引擎 {backend}）",
    toastSyncStopped: "已停止归纳：已完成 {n} 个会话",
    toastScanDone: "增量扫描完成，暂无待摘要会话",
    toastRegrouped: "重新归类完成：更新 {n} 个会话（引擎 {backend}）",
    toastSavedPaths: "已保存目录配置并重新扫描",
    sourcePruned: "源日志已清理",
    sourcePrunedTip: "源轨迹日志已被对应工具清理，已保留摘要与时间跨度",
    selectCliPlaceholder: "选择 CLI 引擎…",
    cliAutoOption: "自动按序回退",
    cliAutoOrderMeta: CLI_AUTO_ORDER_TEXT,
    cliModelPlaceholder: "默认轻量模型（如 flash / gpt-5-mini）",
    cliBinPlaceholder: "自动从 PATH 解析（可选绝对路径）",
    apiKeyEnvPlaceholder: "{env} 或粘贴密钥",
    apiKeyLocalOptPlaceholder: "本地免密（可选）",
    shortThresholdBtn: "少于 {n} 次输入",
    shortThresholdTitle: "短会话判定阈值",
    setupPromptTitle: "尚未设置摘要引擎",
    setupPromptBody: "首次同步前，请先选择要使用的本机 CLI 或模型 API。",
    setupPromptGo: "前往设置 →",
    cancelBtn: "取消",
    procNeedSelectHint: "请先选择引擎并测试连接",
    procModeCliShort: "本机 CLI",
    procModeApiShort: "直连 API",
    agentTimeLbl: "Agent 耗时",
    wallClockLbl: "墙上跨度",
    activeInLast: "近 {n} 天活跃",
    lensTime: "活跃时长",
    lensTokens: "Token",
    lensCost: "费用估算",
    tokensTip: "窗口内累计 Token 用量（含子代理汇总）",
    costTip: "按各模型公开目录价估算的 API 等价费用（自动计入缓存折扣）",
    costPartialTip: "仅覆盖 {priced}/{total} 个带模型元数据的会话（其余缺少模型名称）",
    costUnavailableTip: "当前区间会话缺少模型定价元数据",
    tokensLbl: "Token 用量",
    windowLastShort: "近 {n} 天",
    showMoreLines: "展开其余 {n} 条工作线",
    showFewerLines: "收起至前 {n} 条工作线",
    archivedFold: "{n} 个已归档会话（数据已计入统计）",
    archivedHide: "收起 {n} 个已归档会话",
    archivedBadge: "已归档",
    mergeLine: "合并到…",
    mergeBtn: "合并",
    mergeIntoTitle: "将「{name}」合并到…",
    mergeHint: "会话将迁入目标线，并自动固定目标线、补充 AI 归类范围",
    mergeDragBadge: "↳ 合并「{src}」到「{tgt}」",
    toastMerged: "已将「{src}」合并到「{tgt}」，并更新 AI 归类范围",
    lineScopePlaceholder: "补充 AI 归类范围（如：包含 rank-worker、离线评测…）",
    lineScopeAddPlaceholder: "AI 归类范围说明（可选）",
    lineGuideText: "自动发现了 {n} 条工作线 — 可通过行内菜单或拖放合并相关线，或在设置中固定主线并补充范围说明，后续 Sync 会自动按此归类。",
    lineGuideAction: "管理工作线 →",
  },
  en: {
    navPlane: "Plane",
    navSettings: "Settings",
    liveAuto: "Live dashboard",
    sync: "Sync",
    syncIdle: "Sync",
    syncing: "Syncing...",
    syncTip: "Slice & summarize new trajectories into Lines",
    syncBannerRunning: "Slicing & filing trajectories · {done} / {total} completed",
    syncBannerBatched: "Batch slicing · Batch {batch} / {batches} · {done} / {total} completed",
    syncStopAfterBatch: "Stop after batch",
    syncStopping: "Stopping after batch (click to abort)…",
    syncBatchActiveTag: "Batch ({n})",
    syncBatchMore: "+{n} in batch",
    syncFiledInto: "Filed → {line}",
    syncFiledMore: "+{n}",
    syncFailedBadge: "{n} failed",
    allHarnesses: "All Harnesses",
    allShort: "All",
    prefsTheme: "Theme",
    prefsLang: "Language",
    prefsShortLane: "Short lane",
    prefsSource: "Harnesses",
    themeAutoShort: "Auto",
    autoSaveReady: "Auto-save",
    savedJustNow: "Saved",
    closeBtn: "Back",
    modeDay: "Day",
    modeWeek: "Week",
    modeMonth: "Month",
    range7: "Last 7 days",
    range14: "Last 14 days",
    range30: "Last 30 days",
    today: "Today",
    contribTitle: "20-week activity calendar",
    contribHide: "Hide",
    contribShow: "Show",
    contribBackToday: "Back to today",
    contribLess: "Less",
    contribMore: "More",
    contribNoActivity: "No activity",
    contribSessions: "sessions",
    contribDaysActive: "active days",
    agentTimeLast: "Agent time in the last {n} days",
    agentTimeToday: "Agent time today",
    agentTimeOn: "Agent time on {date}",
    agentTimeTip: "Active trajectory intervals (idle pauses > 12m excluded)",
    unsummarized: "Not summarized yet",
    shortSessions: "Short sessions",
    noShort: "No short sessions in this view",
    recap: "RECAP",
    trajPreview: "TRAJECTORY PREVIEW",
    summary: "Summary",
    stoppedAt: "Stopped at",
    leftOpen: "Left open",
    lineLabel: "Line",
    unassignedLine: "Unassigned · Assign line",
    dependsOn: "Depends on",
    linkUpstream: "+ Link upstream session...",
    unlinkHint: "Click to unlink upstream session",
    hideBtn: "Hide",
    openBtn: "Open",
    stepsWord: "steps",
    lineMenuHint: "Priority, status, rename, merge",
    moreIdle: "{n} more, idle {when}",
    showFewer: "Show fewer",
    idleGroupsToggle: "{n} idle groups {when}",
    hideIdleGroupsToggle: "Hide groups idle {when}",
    todayWhen: "today",
    onWhen: "on {date}",
    lineSingle: "line",
    linePlural: "lines",
    sessionSingle: "session",
    sessionPlural: "sessions",
    linesTitle: "Lines of work",
    priority: "Priority",
    status: "Status",
    renameLine: "Rename...",
    renamePrompt: "Rename line:",
    searchSessions: "Search sessions...",
    otherLines: "Other lines",
    moveSessionTitle: "Move session to line",
    tabGeneral: "Appearance",
    tabProcessor: "Engine",
    tabHarnesses: "Sources",
    tabLines: "Lines & Slicer",
    secAppearance: "Appearance & Language",
    lblLang: "Interface language",
    descLang: "Switches between Chinese and English UI, and guides new AI session recaps.",
    lblTheme: "Theme",
    descTheme: "Follows macOS appearance or locks Light / Dark mode with contrast-validated palettes.",
    themeSystem: "System",
    themeLight: "Light",
    themeDark: "Dark",
    secAutoDashboard: "Plane & Timeline",
    lblContrib: "Show 20-week calendar in window menu",
    descContrib: "Displays daily active agent time inside the window menu for historical drilldown.",
    lblAutoSync: "Automatic sync cadence",
    descAutoSync: "Incrementally summarizes new input and places sessions into existing Lines without renaming.",
    optEveryDay: "Daily",
    optEveryHour: "Hourly",
    optManual: "Manual",
    lblShowShort: "Show short sessions on timeline",
    descShowShort: "Include 1–{n} message sessions as a separate lane in the timeline.",
    lblIncludeArchived: "Include archived sessions",
    descIncludeArchived: "{n} archived sessions in this range are left out.",
    lblPerLine: "Sessions shown per line",
    descPerLine: "The rest of a line's sessions stay collapsed behind expand.",
    resetDefaults: "Reset to defaults",
    resetConfirm: "Click again to confirm",
    resetKept: "Your pinned lines are preserved.",
    procTitle: "Trajectory Summarization Engine",
    procDesc: "Choose who runs Head–Middle–Tail summarization and line assignment: reuse your signed-in local CLI or call a model API directly.",
    procCliTitle: "Reuse local CLI",
    procCliDesc: "Runs a single non-interactive turn via your signed-in Claude, Codex, Antigravity, or DSH CLI with zero API keys.",
    procApiTitle: "Direct model API",
    procApiDesc: "Calls Gemini, Anthropic, OpenAI, DeepSeek, or a local OpenAI-compatible gateway directly.",
    cliAutoChip: "Auto",
    cliMissingHint: "`{cli}` was not found on PATH. Install & sign in, or set a custom path below.",
    cliAdvToggle: "Advanced: custom binary path",
    keyShow: "Show",
    keyHide: "Hide",
    keyLocalNote: "Stored locally in ~/.planar and never uploaded elsewhere.",
    lblCliHarness: "Preferred CLI engine",
    descCliHarness: `Auto picks the first ready CLI in order: ${CLI_AUTO_ORDER_TEXT}.`,
    lblCliModel: "Model",
    descCliModel: "Leave empty to use the CLI's default fast model.",
    lblCliBin: "Executable path",
    descCliBin: "Set only if the binary lives outside standard PATH directories.",
    lblApiProvider: "API protocol",
    descApiProvider: "Selects request format and default endpoint.",
    apiCompatShort: "Local / Compatible",
    lblApiUrl: "Base URL",
    descApiUrl: "Required for local/compatible gateways; leave empty for official endpoints.",
    lblApiModel: "Model ID",
    descApiModel: `e.g. ${API_MODEL_EXAMPLES.join(", ")}.`,
    descApiModelGemini: "Defaults to {defaultModel}; use {fastModel} for ultra-fast, lowest-cost runs.",
    lblApiKey: "API key",
    descApiKey: "Leave empty to read standard environment variables (e.g. DEEPSEEK_API_KEY).",
    lblBatchLimit: "Max sessions per Sync batch",
    descBatchLimit: "Caps batch size (1–50); large backlogs update the plane progressively batch by batch.",
    testProcBtn: "Test connection",
    testingProcBtn: "Testing connection…",
    syncNowBtn: "Sync now",
    harnessesTitle: "Trajectory Sources & Log Folders",
    harnessesDesc: "Planar reads local harness logs strictly read-only — never mutating, locking, or wrapping transcripts.",
    cliReady: "CLI ready",
    cliMissing: "No CLI",
    dirMissing: "Folder absent",
    editPathBtn: "Custom folder…",
    collapsePathBtn: "Done",
    resetPathBtn: "Reset",
    pathPlaceholder: "Custom log root folder (leave empty for default)",
    rescanBtn: "Rescan folders",
    rescanningBtn: "Scanning…",
    pinnedLinesTitle: "Pinned Lines of work",
    pinnedLinesDesc: "Pinned lines sort first and keep dedicated swimlanes even before sessions run today. Click a swatch to change its color.",
    pickLineColor: "Line color",
    unpinBtn: "Unpin",
    emptyPinnedHint: "No pinned lines yet. Create one below or keep a discovered line.",
    linePlaceholder: "A project or product line name…",
    addLineBtn: "Add line",
    discoveredTitle: "Discovered by Planar",
    discoveredDesc: "Re-clustered when you press Regroup; click Keep to pin a line permanently.",
    emptyDiscoveredHint: "No unpinned discovered lines right now. New clusters from Sync will appear here.",
    regroupBtn: "Regroup",
    regroupingBtn: "Regrouping…",
    keepBtn: "Keep",
    slicerHeading: "Head–Middle–Tail Trajectory Slicer",
    slicerDragHint: "Drag handles to adjust step bounds",
    slicerFirst: "First {n} steps (full turns)",
    slicerLast: "Last {n} steps (full turns)",
    slicerMidCaption: "Middle skipped — only your prompts (●) kept; tool outputs stripped",
    slicerOpenLbl: "Opening steps",
    slicerCloseLbl: "Closing steps",
    slicerDesc: "Colored ends retain both prompts and replies; the middle retains only user prompts; tool outputs are always stripped.",
    lblFewestInputs: "Fewest user inputs",
    descFewestInputs: "Sessions with fewer user prompts stay in Short sessions until they grow.",
    lblMaxUserChars: "Longest user input kept",
    descMaxUserChars: "A longer input is trimmed here. 0 keeps every prompt whole.",
    lblMaxAgentChars: "Longest agent message kept",
    descMaxAgentChars: "A longer reply is trimmed here. 0 keeps every reply whole.",
    charsUnit: "chars",
    slicerFoot: "Changing slicer bounds invalidates affected cached digests on the next Sync.",
    toastSynced: "Synced {n} sessions ({backend})",
    toastSyncedWithFail: "Synced {n} sessions ({failed} failed, {backend})",
    toastSyncStopped: "Stopped sync after {n} sessions",
    toastScanDone: "Scan finished — no unsummarized sessions needed updates",
    toastRegrouped: "Regrouped {n} sessions ({backend})",
    toastSavedPaths: "Saved harness roots & rescanned logs",
    sourcePruned: "Source log pruned",
    sourcePrunedTip: "Source transcript was cleaned up by its harness; summary and timeline spans are preserved",
    selectCliPlaceholder: "Select CLI engine…",
    cliAutoOption: "Auto fallback",
    cliAutoOrderMeta: CLI_AUTO_ORDER_TEXT,
    cliModelPlaceholder: "Default fast model (e.g. flash / gpt-5-mini)",
    cliBinPlaceholder: "Resolved from PATH (optional full path)",
    apiKeyEnvPlaceholder: "{env} or paste key",
    apiKeyLocalOptPlaceholder: "Optional for local endpoint",
    shortThresholdBtn: "< {n} inputs",
    shortThresholdTitle: "Short-session threshold",
    setupPromptTitle: "Set up summarizer first",
    setupPromptBody: "Choose a local CLI or model API before running your first Sync.",
    setupPromptGo: "Configure →",
    cancelBtn: "Cancel",
    procNeedSelectHint: "Select an engine & test connection",
    procModeCliShort: "Local CLI",
    procModeApiShort: "Direct API",
    agentTimeLbl: "agent time",
    wallClockLbl: "wall clock",
    activeInLast: "active in the last {n} days",
    lensTime: "Agent time",
    lensTokens: "Tokens",
    lensCost: "Cost",
    tokensTip: "Cumulative window tokens (including subagents)",
    costTip: "Estimated API list-price USD spend (includes cached input discounts)",
    costPartialTip: "Covers {priced}/{total} priced sessions (others omitted model metadata)",
    costUnavailableTip: "No model pricing metadata available in this range",
    tokensLbl: "tokens",
    windowLastShort: "Last {n}d",
    showMoreLines: "Show {n} more lines",
    showFewerLines: "Show top {n} lines",
    archivedFold: "{n} archived (included in totals)",
    archivedHide: "Hide {n} archived",
    archivedBadge: "archived",
    mergeLine: "Merge into…",
    mergeBtn: "Merge",
    mergeIntoTitle: "Merge \"{name}\" into…",
    mergeHint: "Moves sessions, pins target, and appends \"{name}\" to its AI scope",
    mergeDragBadge: "↳ Merge \"{src}\" into \"{tgt}\"",
    toastMerged: "Merged \"{src}\" into \"{tgt}\" & updated AI scope",
    lineScopePlaceholder: "AI scope note (e.g. includes rank-worker, offline evals…)",
    lineScopeAddPlaceholder: "AI scope note (optional)",
    lineGuideText: "Found {n} lines — merge related ones via the line menu or drag-and-drop, or pin & add scope notes in Settings so future Syncs follow your structure.",
    lineGuideAction: "Manage lines →",
  },
};

const DAYS = [
  { idx: 0, isoDate: "2026-11-08", short: "Sun 8", tiny: "Sun", full: "Sun, Nov 8" },
  { idx: 1, isoDate: "2026-11-09", short: "Mon 9", tiny: "Mon", full: "Mon, Nov 9" },
  { idx: 2, isoDate: "2026-11-10", short: "Tue 10", tiny: "Tue", full: "Tue, Nov 10" },
  { idx: 3, isoDate: "2026-11-11", short: "Wed 11", tiny: "Wed", full: "Wed, Nov 11" },
  { idx: 4, isoDate: "2026-11-12", short: "Thu 12", tiny: "Thu", full: "Thu, Nov 12" },
  { idx: 5, isoDate: "2026-11-13", short: "Fri 13", tiny: "Fri", full: "Fri, Nov 13" },
  { idx: 6, isoDate: "2026-11-14", short: "Sat 14", tiny: "Sat", full: "Today", isToday: true },
];

const BAND_JOIN_PX = 2.5;
const BAND_EASE_PX = 5.0;

// Exact C1-continuous 8-knot cubic Bezier shapes (50 coords each) centered at (7, 7) for MD3 morphing indicator
const MD3_SHAPES = {
  cookie:
    "M8.68 7.00C8.68 7.93 9.56 8.85 9.21 9.21C8.85 9.56 7.93 8.68 7.00 8.68C6.07 8.68 5.15 9.56 4.79 9.21C4.44 8.85 5.32 7.93 5.32 7.00C5.32 6.07 4.44 5.15 4.79 4.79C5.15 4.44 6.07 5.32 7.00 5.32C7.93 5.32 8.85 4.44 9.21 4.79C9.56 5.15 8.68 6.07 8.68 7.00Z",
  pill:
    "M10.22 7.00C10.22 7.67 9.34 8.39 8.71 8.71C8.08 9.03 7.67 8.65 7.00 8.65C6.33 8.65 5.92 9.03 5.29 8.71C4.66 8.39 3.78 7.67 3.78 7.00C3.78 6.33 4.66 5.61 5.29 5.29C5.92 4.97 6.33 5.35 7.00 5.35C7.67 5.35 8.08 4.97 8.71 5.29C9.34 5.61 10.22 6.33 10.22 7.00Z",
  clover:
    "M10.12 7.00C10.12 7.50 8.84 7.53 8.19 8.19C7.53 8.84 7.50 10.12 7.00 10.12C6.50 10.12 6.47 8.84 5.81 8.19C5.16 7.53 3.88 7.50 3.88 7.00C3.88 6.50 5.16 6.47 5.81 5.81C6.47 5.16 6.50 3.88 7.00 3.88C7.50 3.88 7.53 5.16 8.19 5.81C8.84 6.47 10.12 6.50 10.12 7.00Z",
  squircle:
    "M9.55 7.00C9.55 7.82 9.60 8.61 9.11 9.11C8.61 9.60 7.82 9.55 7.00 9.55C6.18 9.55 5.39 9.60 4.89 9.11C4.40 8.61 4.45 7.82 4.45 7.00C4.45 6.18 4.40 5.39 4.89 4.89C5.39 4.40 6.18 4.45 7.00 4.45C7.82 4.45 8.61 4.40 9.11 4.89C9.60 5.39 9.55 6.18 9.55 7.00Z",
};

function buildMockContributions() {
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
    const claude = active ? ((idx * 13 + 7) % 55) : 0;
    const codex = active ? ((idx * 19 + 11) % 70) : 0;
    const agy = active && idx % 3 !== 0 ? ((idx * 23 + 3) % 48) : 0;
    const dsh = active && idx % 4 === 0 ? ((idx * 11 + 9) % 36) : 0;
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

const DEFAULT_MOCK_CONTRIBUTIONS = buildMockContributions();

const DEFAULT_MOCK_HARNESSES = [
  { id: "antigravity", enabled: true, cliAvailable: true, cliPath: "~/.local/bin/agy", logRoot: "~/.gemini/antigravity/brain", logRootExists: true, sessionCount: 5 },
  { id: "claude", enabled: true, cliAvailable: true, cliPath: "/opt/homebrew/bin/claude", logRoot: "~/.claude/projects", logRootExists: true, sessionCount: 6 },
  { id: "codex", enabled: true, cliAvailable: true, cliPath: "/opt/homebrew/bin/codex", logRoot: "~/.codex/sessions", logRootExists: true, sessionCount: 6 },
  { id: "dsh", enabled: true, cliAvailable: false, cliPath: null, logRoot: "~/.dsh/sessions", logRootExists: false, sessionCount: 4 },
];

const state = {
  route: "main", // "main" | "settings" (dedicated full-page views)
  routeMotion: "", // "" | "enter-settings" | "enter-main"
  savedMainScrollY: 0,
  resetSettingsScroll: false,
  settingsTab: "lines", // "lines" | "processor" | "harnesses" | "general"
  settingsSavedPulse: false,
  editingHarnessPaths: new Set(),
  showCliBinAdvanced: false,
  showApiKeyPlain: false,
  confirmResetDefaults: false,
  dataSource: "mock", // "mock" | "api"
  mode: "week", // "day" | "week" | "month"
  selectedHarness: "all", // "all" | "antigravity" | "claude" | "codex" | "dsh"
  anchorDate: null,
  rangeLabel: "Last 7 days",
  rangeShort: "7d",
  days: DAYS.map((d) => ({ ...d })),
  nowHour: null,
  tzLabel: "UTC+8",
  selectedDayIdx: 4,
  plotWidth: 644,
  hoveredLaneId: null,
  hoveredSessionKey: null,
  expandedTimelineLines: new Set(),
  expandedListLines: new Set(["sqlite-sync"]),
  expandedSessionIds: new Set(),
  expandedArchivedInLine: new Set(),
  showAllIdleInLine: new Set(),
  showAllLines: false,
  dismissedLineGuide: (() => {
    try {
      return window.localStorage?.getItem("planar.dismissedLineGuide") === "1";
    } catch {
      return false;
    }
  })(),
  hideIdleGroups: true,
  contribCollapsed: false,
  isSyncing: false,
  syncStopRequested: false,
  syncProcessedTotal: 0,
  syncCompletedCount: 0,
  syncFailedCount: 0,
  syncInitialPending: 0,
  syncBatchIndex: 0,
  syncTotalBatches: 0,
  syncBatchItems: [],
  syncLastFiled: null,
  syncAbortController: null,
  isTestingProcessor: false,
  isRegrouping: false,
  isRescanning: false,
  processorTestResult: null,
  popover: null,
  pickerQuery: "",
  dragState: null,
  settings: {
    ...DEFAULT_SETTINGS,
    enabledHarnesses: { ...DEFAULT_SETTINGS.enabledHarnesses },
    harnessPaths: { ...DEFAULT_SETTINGS.harnessPaths },
  },
  archivedCount: 11,
  contributions: DEFAULT_MOCK_CONTRIBUTIONS,
  harnesses: DEFAULT_MOCK_HARNESSES,
  lastSync: null,
  // bursts: [dayIdx, startHour, endHour, promptHours[], activeMinutes]
  lines: [
    {
      id: "sqlite-sync",
      name: "sqlite-sync",
      description: "WAL incremental log ingestion & idempotent receipt cursors",
      color: PALETTE[0],
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
      color: PALETTE[1],
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
      color: PALETTE[2],
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
      color: PALETTE[3],
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
      color: PALETTE[4],
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

// Helpers: Locale, Theme, Harnesses & Metrics
function lang() {
  const raw = state.settings.language || DEFAULT_SETTINGS.language;
  return raw === "en" ? "en" : "zh";
}

function t(key, vars = undefined) {
  const dict = I18N[lang()] || I18N.en;
  let str = dict[key] ?? I18N.en[key] ?? key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      str = str.replaceAll(`{${k}}`, String(v));
    }
  }
  return str;
}

function localizeRecency(recency) {
  if (!recency) return "";
  if (lang() !== "zh") return recency;
  if (recency === "running") return "运行中";
  if (recency === "just now") return "刚刚";
  const m = /^(\d+)([mhd])\s+ago$/.exec(recency);
  if (m) {
    const unit = m[2] === "m" ? "分钟前" : m[2] === "h" ? "小时前" : "天前";
    return `${m[1]}${unit}`;
  }
  return recency;
}

function effectiveTheme() {
  const mode = state.settings.theme || DEFAULT_SETTINGS.theme;
  if (mode === "light" || mode === "dark") return mode;
  return window.matchMedia?.("(prefers-color-scheme: dark)")?.matches ? "dark" : "light";
}

function applyThemeAndLang() {
  document.documentElement.dataset.theme = effectiveTheme();
  document.documentElement.lang = lang();
}

function resolveLineColor(color) {
  if (!color) return "#7E828A";
  const idx = PALETTE.findIndex((c) => c.toLowerCase() === color.toLowerCase());
  if (idx >= 0 && effectiveTheme() === "dark") {
    return DARK_PALETTE[idx];
  }
  return color;
}

function matchesHarness(item) {
  if (!item) return false;
  if (state.selectedHarness === "all") return true;
  return (item.harness || "codex") === state.selectedHarness;
}

function harnessLabel(id) {
  const meta = HARNESS_META[id];
  if (!meta) return id;
  return lang() === "zh" ? meta.labelZh : meta.labelEn;
}

function harnessFullLabel(id) {
  const meta = HARNESS_META[id];
  if (!meta) return id;
  return lang() === "zh" ? meta.fullLabelZh || meta.labelZh : meta.fullLabelEn || meta.labelEn;
}

function isProcessorConfigured(s = state.settings) {
  if (!s?.processorConfigured) return false;
  if ((s.processorMode || DEFAULT_SETTINGS.processorMode) === "api") {
    return Boolean((s.apiProvider || "").trim());
  }
  return Boolean((s.cliHarness || "").trim());
}

function getApiProviderMeta(id) {
  return API_PROVIDERS.find((p) => p.id === id) || API_PROVIDERS[0];
}

function apiProviderLabel(id) {
  const meta = getApiProviderMeta(id);
  return lang() === "zh" ? meta.labelZh : meta.labelEn;
}

function langOptionLabel(code) {
  const opt = LANG_OPTIONS.find((l) => l.id === code) || LANG_OPTIONS[0];
  return opt.label;
}

let harnessSvgSeq = 0;

function harnessIconSvg(harness, size = 13, { colored = false } = {}) {
  const meta = HARNESS_META[harness];
  if (!meta) return "";
  const label = escapeHtml(harnessFullLabel(harness));
  const p = meta.path;

  if (!colored) {
    return `<span class="harness-svg" title="${label}"><svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" clip-rule="evenodd" d="${p}"/></svg></span>`;
  }

  harnessSvgSeq = harnessSvgSeq > 100000 ? 1 : harnessSvgSeq + 1;
  const uid = harnessSvgSeq;

  if (meta.id === "antigravity") {
    return `<span class="harness-svg is-colored" data-harness="antigravity" title="${label}"><svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" aria-hidden="true"><defs><radialGradient id="hx-ag1-${uid}" gradientUnits="userSpaceOnUse" cx="3.2" cy="13.5" r="9.2"><stop offset="0%" stop-color="#00B95C"/><stop offset="48%" stop-color="#00B95C" stop-opacity="0.94"/><stop offset="100%" stop-color="#00B95C" stop-opacity="0"/></radialGradient><radialGradient id="hx-ag2-${uid}" gradientUnits="userSpaceOnUse" cx="8.4" cy="4.0" r="8.4"><stop offset="0%" stop-color="#FFE033"/><stop offset="50%" stop-color="#FBBC04" stop-opacity="0.95"/><stop offset="100%" stop-color="#FBBC04" stop-opacity="0"/></radialGradient><radialGradient id="hx-ag3-${uid}" gradientUnits="userSpaceOnUse" cx="15.8" cy="6.2" r="9.4"><stop offset="0%" stop-color="#FC413D"/><stop offset="48%" stop-color="#FC413D" stop-opacity="0.92"/><stop offset="100%" stop-color="#FC413D" stop-opacity="0"/></radialGradient></defs><path d="${p}" fill="#3186FF"/><path d="${p}" fill="url(#hx-ag1-${uid})"/><path d="${p}" fill="url(#hx-ag2-${uid})"/><path d="${p}" fill="url(#hx-ag3-${uid})"/></svg></span>`;
  }

  if (meta.id === "codex") {
    const bottomStop = effectiveTheme() === "dark" ? "#525EFF" : "#3941FF";
    return `<span class="harness-svg is-colored" data-harness="codex" title="${label}"><svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" aria-hidden="true"><defs><linearGradient id="hx-cx-${uid}" gradientUnits="userSpaceOnUse" x1="12" y1="0.4" x2="12" y2="23.6"><stop offset="0%" stop-color="#B1A7FF"/><stop offset="50%" stop-color="#7A9DFF"/><stop offset="100%" stop-color="${bottomStop}"/></linearGradient></defs><path fill-rule="evenodd" clip-rule="evenodd" d="${p}" fill="url(#hx-cx-${uid})"/></svg></span>`;
  }

  return `<span class="harness-svg is-colored" data-harness="${meta.id}" title="${label}"><svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" clip-rule="evenodd" d="${p}"/></svg></span>`;
}

function themeIconSvg(themePref) {
  if (themePref === "dark") {
    return `<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M13.2 10.1A5.5 5.5 0 0 1 5.9 2.8a5.5 5.5 0 1 0 7.3 7.3z" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round"/></svg>`;
  }
  if (themePref === "light") {
    return `<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="3" stroke="currentColor" stroke-width="1.35"/><path d="M8 1.6v1.4M8 13v1.4M1.6 8H3M13 8h1.4M3.5 3.5l1 1M11.5 11.5l1 1M12.5 3.5l-1 1M4.5 11.5l-1 1" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>`;
  }
  return `<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="5.5" stroke="currentColor" stroke-width="1.3"/><path d="M8 2.5a5.5 5.5 0 0 1 0 11V2.5z" fill="currentColor"/></svg>`;
}

function fmtMinutesCompact(mins) {
  const rounded = Math.round(mins);
  if (rounded <= 0) return "0m";
  const h = Math.floor(rounded / 60);
  const m = rounded % 60;
  if (h === 0) return `${m}m`;
  return `${h}h ${String(m).padStart(2, "0")}m`;
}

function fmtTokensCompact(tokens) {
  const n = Math.round(Number(tokens) || 0);
  if (n <= 0) return "0";
  if (n < 1000) return String(n);
  if (n < 99950) {
    const k = (n / 1000).toFixed(1).replace(/\.0$/, "");
    return `${k}K`;
  }
  if (n < 999500) {
    return `${Math.round(n / 1000)}K`;
  }
  if (n < 99950000) {
    const m = (n / 1000000).toFixed(1).replace(/\.0$/, "");
    return `${m}M`;
  }
  if (n < 999500000) {
    return `${Math.round(n / 1000000)}M`;
  }
  const b = (n / 1000000000).toFixed(1).replace(/\.0$/, "");
  return `${b}B`;
}

function fmtCostCompact(usd) {
  const c = Number(usd);
  if (!Number.isFinite(c) || c <= 0) return "";
  if (c < 0.005) return "≈ <$0.01";
  if (c >= 100) return `≈ $${Math.round(c)}`;
  return `≈ $${c.toFixed(2)}`;
}

const DEFAULT_VISIBLE_LINES = 8;

function activeMetricLens() {
  const lens = state.settings.metricLens || DEFAULT_SETTINGS.metricLens;
  if (lens === "tokens") return "tokens";
  if (lens === "cost") return "cost";
  return "time";
}

function metricLensIconSvg(lens) {
  if (lens === "tokens") {
    return `<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <rect x="2.25" y="3.25" width="6.5" height="3.75" rx="1.25" fill="currentColor"/>
      <rect x="10.75" y="3.25" width="3" height="3.75" rx="1.15" stroke="currentColor" stroke-width="1.25"/>
      <rect x="2.25" y="9" width="3.25" height="3.75" rx="1.15" stroke="currentColor" stroke-width="1.25"/>
      <rect x="7.5" y="9" width="6.25" height="3.75" rx="1.25" stroke="currentColor" stroke-width="1.25"/>
    </svg>`;
  }
  if (lens === "cost") {
    return `<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path d="M3.5 2.35H12.5V13.55L10.7 12.25L8.95 13.55L7.05 12.25L5.3 13.55L3.5 12.25V2.35Z" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/>
      <path d="M5.75 5.45H8.5M5.75 8.2H7.75" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
      <circle cx="10.3" cy="5.45" r="0.85" fill="currentColor"/>
      <circle cx="10.3" cy="8.2" r="0.85" fill="currentColor"/>
    </svg>`;
  }
  return `<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path d="M4 2.25H12M4 13.75H12" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/>
    <path d="M5 2.25V4.45C5 5.75 6.45 7 8 8C9.55 7 11 5.75 11 4.45V2.25" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/>
    <path d="M5 13.75V11.55C5 10.25 6.45 9 8 8C9.55 9 11 10.25 11 11.55V13.75" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/>
    <path d="M6.4 12.15L8 10.55L9.6 12.15H6.4Z" fill="currentColor"/>
  </svg>`;
}

function mergeIconSvg() {
  return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
    <path d="M3.2 3.2V5.3C3.2 6.6 4.25 7.6 5.55 7.6H8.45C9.75 7.6 10.8 6.6 10.8 5.3V3.2" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="M7 7.6V11.6M7 11.6L5.1 9.8M7 11.6L8.9 9.8" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/>
  </svg>`;
}

function sessionUsageInView(session, isDay, dayIdx) {
  const totTok = Number(session.totalTokens || 0);
  const totCost = Number(session.costUsd || 0);
  if (totTok <= 0 && totCost <= 0) return { tokens: 0, costUsd: 0 };
  if (!isDay) {
    return { tokens: totTok, costUsd: totCost };
  }
  const dayMins = Array.isArray(session.bursts) ? sessionMinutesOnDay(session, dayIdx) : Number(session.mins || 0);
  if (dayMins <= 0) return { tokens: 0, costUsd: 0 };
  const weekMins = Array.isArray(session.bursts)
    ? Math.max(1, sessionMinutesInWeek(session))
    : Math.max(1, dayMins);
  const share = Math.min(1, dayMins / weekMins);
  return {
    tokens: Math.round(totTok * share),
    costUsd: totCost * share,
  };
}

function renderMetricRightCell({
  mins,
  maxMins,
  tokens,
  maxTokens,
  costUsd,
  maxCost = 0,
  pricedSessions = 0,
  totalSessions = 0,
  barColor,
  minBarPct = 10,
  forceShowZeroTime = false,
}) {
  const lens = activeMetricLens();
  if (lens === "tokens") {
    const hasTok = tokens > 0;
    const barPct = hasTok && maxTokens > 0 ? Math.min(100, Math.round((tokens / maxTokens) * 100)) : 0;
    return `<div class="mini-bar-track"><div class="mini-bar-fill" style="width:${
      hasTok ? Math.max(minBarPct, barPct) : 0
    }%;background:${barColor}"></div></div>
      <span class="line-duration ${hasTok ? "" : "is-empty-metric"}">${
      hasTok ? escapeHtml(fmtTokensCompact(tokens)) : "—"
    }</span>`;
  }
  if (lens === "cost") {
    const hasCost = Number.isFinite(costUsd) && costUsd > 0;
    const barPct = hasCost && maxCost > 0 ? Math.min(100, Math.round((costUsd / maxCost) * 100)) : 0;
    const isPartial = hasCost && pricedSessions > 0 && totalSessions > pricedSessions;
    const tip = isPartial
      ? t("costPartialTip", { priced: pricedSessions, total: totalSessions })
      : hasCost
      ? t("costTip")
      : t("costUnavailableTip");
    return `<div class="mini-bar-track"><div class="mini-bar-fill" style="width:${
      hasCost ? Math.max(minBarPct, barPct) : 0
    }%;background:${barColor}"></div></div>
      <span class="line-duration ${hasCost ? "" : "is-empty-metric"}" title="${escapeHtml(tip)}">${
      hasCost
        ? `${escapeHtml(fmtCostCompact(costUsd))}${isPartial ? `<span class="cost-partial-mark">*</span>` : ""}`
        : "—"
    }</span>`;
  }
  if (mins <= 0 && !forceShowZeroTime) {
    return `<div class="mini-bar-track"><div class="mini-bar-fill" style="width:0%;background:${barColor}"></div></div>
      <span class="line-duration is-empty-metric">—</span>`;
  }
  const barPct = maxMins > 0 && mins > 0 ? Math.min(100, Math.round((mins / maxMins) * 100)) : 0;
  return `<div class="mini-bar-track"><div class="mini-bar-fill" style="width:${
    mins > 0 ? Math.max(minBarPct, barPct) : 0
  }%;background:${barColor}"></div></div>
    <span class="line-duration">${fmtMinutesCompact(mins)}</span>`;
}

function fmtClock(hr) {
  const h = Math.floor(hr);
  const m = Math.round((hr - h) * 60);
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function burstMinutes(burst) {
  const [, s, e, , explicitMins] = burst;
  if (typeof explicitMins === "number") return explicitMins;
  return Math.max(2, Math.round((e - s) * 60));
}

function sessionMinutesOnDay(session, dayIdx) {
  let mins = 0;
  for (const b of session.bursts) {
    if (b[0] === dayIdx) mins += burstMinutes(b);
  }
  return Math.round(mins);
}

function sessionMinutesInWeek(session) {
  let mins = 0;
  for (let d = 0; d < state.days.length; d++) mins += sessionMinutesOnDay(session, d);
  return mins;
}

function unionSpanMinutes(intervals) {
  if (!intervals.length) return 0;
  const sorted = [...intervals].sort((a, b) => a[0] - b[0]);
  const merged = [sorted[0].slice()];
  for (let i = 1; i < sorted.length; i++) {
    const [a, b] = sorted[i];
    const last = merged[merged.length - 1];
    if (a <= last[1]) last[1] = Math.max(last[1], b);
    else merged.push([a, b]);
  }
  return Math.round(merged.reduce((acc, [a, b]) => acc + Math.max(0.03, b - a) * 60, 0));
}

function lineStats(line, mode, dayIdx) {
  const visible = line.sessions.filter((s) => !s.hidden && matchesHarness(s));
  if (mode !== "day") {
    let agentMins = 0;
    let tokens = 0;
    let costUsd = 0;
    let pricedSessions = 0;
    const activeSessions = [];
    let wallMins = 0;
    for (const s of visible) {
      const m = sessionMinutesInWeek(s);
      if (m > 0) {
        agentMins += m;
        tokens += Number(s.totalTokens || 0);
        const sc = Number(s.costUsd || 0);
        if (sc > 0) {
          costUsd += sc;
          pricedSessions += 1;
        }
        activeSessions.push(s);
      }
    }
    for (let d = 0; d < state.days.length; d++) {
      const dayIv = [];
      for (const s of visible) {
        for (const [bd, a, b] of s.bursts) {
          if (bd === d) dayIv.push([a, b]);
        }
      }
      wallMins += unionSpanMinutes(dayIv);
    }
    return {
      agentMins,
      wallMins:
        state.dataSource === "api"
          ? agentMins > 0
            ? Math.min(agentMins, Math.max(1, wallMins))
            : 0
          : Math.min(agentMins, Math.max(Math.round(agentMins * 0.88), 1)),
      tokens,
      costUsd,
      pricedSessions,
      activeSessions,
      totalSessions: visible.length,
    };
  }

  let agentMins = 0;
  let tokens = 0;
  let costUsd = 0;
  let pricedSessions = 0;
  const activeSessions = [];
  const dayIv = [];
  for (const s of visible) {
    const m = sessionMinutesOnDay(s, dayIdx);
    if (m > 0) {
      agentMins += m;
      const usage = sessionUsageInView(s, true, dayIdx);
      tokens += usage.tokens;
      if (usage.costUsd > 0) {
        costUsd += usage.costUsd;
        pricedSessions += 1;
      }
      activeSessions.push(s);
    }
    for (const [bd, a, b] of s.bursts) {
      if (bd === dayIdx) dayIv.push([a, b]);
    }
  }
  const rawUnion = unionSpanMinutes(dayIv);
  return {
    agentMins,
    wallMins: agentMins > 0 ? Math.min(agentMins, Math.max(Math.round(agentMins * 0.86), Math.min(rawUnion, agentMins))) : 0,
    tokens,
    costUsd,
    pricedSessions,
    activeSessions,
    totalSessions: visible.length,
  };
}

// Icons
function planarLogoSvg() {
  return `<svg class="brand-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <defs><clipPath id="p-clip"><rect width="24" height="24" rx="4"/></clipPath></defs>
    <g clip-path="url(#p-clip)">
      <rect width="24" height="24" fill="#E63B7A"/>
      <path d="M0 0H24V4.15H16A8 8 0 0 1 8 12.15H0Z" fill="#F27D16"/>
      <path d="M0 12H8A8 8 0 0 0 16 4H24V12H16L8 20H0Z" fill="#2945F5"/>
    </g>
  </svg>`;
}

function chevronSvg(dir = "down") {
  if (dir === "right") {
    return `<svg width="11" height="11" viewBox="0 0 12 12" fill="none" style="display:block"><path d="M4.5 2.8L7.7 6L4.5 9.2" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
  }
  return `<svg width="11" height="11" viewBox="0 0 12 12" fill="none" style="display:block"><path d="M2.8 4.5L6 7.7L9.2 4.5" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
}

function checkMarkSvg() {
  return `<span class="check-mark"><svg width="13" height="13" viewBox="0 0 14 14" fill="none"><path d="M2.5 7.2L5.4 10.1L11.5 3.9" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"/></svg></span>`;
}

// Ascending 3-step staircase Priority icons (matching reference UI exactly)
function priorityIconSvg(priority) {
  if (priority === 1) {
    // Urgent: rounded orange-red badge containing 3 white ascending stair treads
    return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none">
      <rect x="0.6" y="0.6" width="12.8" height="12.8" rx="3.2" fill="#DE4E18"/>
      <rect x="2.35" y="9.05" width="3.3" height="1.55" rx="0.78" fill="#FFFFFF"/>
      <rect x="5.35" y="6.22" width="3.3" height="1.55" rx="0.78" fill="#FFFFFF"/>
      <rect x="8.35" y="3.40" width="3.3" height="1.55" rx="0.78" fill="#FFFFFF"/>
    </svg>`;
  }
  const activeSteps = priority === 2 ? 3 : priority === 3 ? 2 : priority === 4 ? 1 : 0;
  const on = "var(--prio-on)";
  const off = "var(--prio-off)";
  const c1 = activeSteps >= 1 ? on : off;
  const c2 = activeSteps >= 2 ? on : off;
  const c3 = activeSteps >= 3 ? on : off;
  return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none">
    <rect x="1.5" y="9.5" width="4.0" height="1.75" rx="0.88" fill="${c1}"/>
    <rect x="5.0" y="6.12" width="4.0" height="1.75" rx="0.88" fill="${c2}"/>
    <rect x="8.5" y="2.75" width="4.0" height="1.75" rx="0.88" fill="${c3}"/>
  </svg>`;
}

function statusIconSvg(status) {
  if (status === "paused") {
    return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7" r="5.5" stroke="#C98727" stroke-width="1.4"/><path d="M5.6 5v4M8.4 5v4" stroke="#C98727" stroke-width="1.4" stroke-linecap="round"/></svg>`;
  }
  if (status === "done") {
    return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7" r="6" fill="var(--accent-sync)"/><path d="M4.4 7.1l1.7 1.7 3.5-3.6" stroke="#fff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
  }
  return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7" r="5.5" stroke="var(--ink-secondary)" stroke-width="1.4"/><circle cx="7" cy="7" r="2" fill="var(--ink-secondary)"/></svg>`;
}

// Session Status Indicator:
// - Running session: Material Design 3 Expressive morphing inner shape + gentle rotation inside breathing ring
// - Completed/idle summarized session: static ring + centered dot
// - Short session: hollow ring
function sessionIndicatorSvg(color, { isShort = false, isRunning = false } = {}) {
  if (isShort) {
    return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none">
      <circle cx="7" cy="7" r="5.0" stroke="${color}" stroke-opacity="0.58" stroke-width="1.25"/>
    </svg>`;
  }
  if (isRunning) {
    const morphValues = `${MD3_SHAPES.cookie};${MD3_SHAPES.pill};${MD3_SHAPES.clover};${MD3_SHAPES.squircle};${MD3_SHAPES.cookie}`;
    return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-label="Running session">
      <circle class="md3-ring-live" cx="7" cy="7" r="5.45" stroke="${color}" stroke-opacity="0.65" stroke-width="1.3"/>
      <g class="md3-morph-rotor">
        <path d="${MD3_SHAPES.cookie}" fill="${color}">
          <animate attributeName="d"
            dur="2.6s"
            repeatCount="indefinite"
            calcMode="spline"
            keyTimes="0;0.25;0.5;0.75;1"
            keySplines="0.4 0 0.2 1;0.4 0 0.2 1;0.4 0 0.2 1;0.4 0 0.2 1"
            values="${morphValues}"/>
        </path>
      </g>
    </svg>`;
  }
  return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none">
    <circle cx="7" cy="7" r="5.3" stroke="${color}" stroke-opacity="0.74" stroke-width="1.3"/>
    <circle cx="7" cy="7" r="2.0" fill="${color}" fill-opacity="0.9"/>
  </svg>`;
}

// ============================================================================
// Continuous Thickness-Encoded Timeline Band (`bandProfile` + `bandPath`)
// ============================================================================

function halfThicknessForCount(count, isSubLane) {
  if (count <= 0) return 0;
  if (isSubLane) return 2.0;
  if (count === 1) return 2.5;
  if (count === 2) return 3.5;
  return 4.5;
}

// Merge overlapping or sub-BAND_JOIN_PX intervals within a single session first
// so one session never double-counts itself as 2 concurrent sessions.
function normalizeSessionPixelSpans(rawSpans, minRadius) {
  if (!rawSpans.length) return [];
  const padded = rawSpans
    .map(([x1, x2]) => {
      const mid = (x1 + x2) / 2;
      return x2 - x1 < minRadius * 2 ? [mid - minRadius, mid + minRadius] : [x1, x2];
    })
    .sort((a, b) => a[0] - b[0]);

  const merged = [padded[0].slice()];
  for (let i = 1; i < padded.length; i++) {
    const [a, b] = padded[i];
    const last = merged[merged.length - 1];
    if (a - last[1] <= BAND_JOIN_PX) {
      last[1] = Math.max(last[1], b);
    } else {
      merged.push([a, b]);
    }
  }
  return merged;
}

// Build contiguous active islands of [x0, x1, halfThickness] from per-session pixel spans
function bandProfile(sessionSpanLists, isSubLane = false) {
  const minR = isSubLane ? 2.0 : 2.5;
  const events = [];
  for (const spans of sessionSpanLists) {
    const clean = normalizeSessionPixelSpans(spans, minR);
    for (const [a, b] of clean) {
      events.push([a, 1], [b, -1]);
    }
  }
  if (!events.length) return [];
  events.sort((p, q) => p[0] - q[0] || q[1] - p[1]);

  const rawSegs = [];
  let curCount = 0;
  let prevX = events[0][0];
  let idx = 0;
  while (idx < events.length) {
    const x = events[idx][0];
    if (x > prevX + 1e-4) {
      rawSegs.push([prevX, x, curCount]);
      prevX = x;
    }
    while (idx < events.length && Math.abs(events[idx][0] - x) <= 1e-4) {
      curCount += events[idx][1];
      idx++;
    }
  }

  // Bridge zero-count gaps narrower than BAND_JOIN_PX (`避免出现掐腰`)
  const bridged = [];
  for (let i = 0; i < rawSegs.length; i++) {
    const [a, b, cnt] = rawSegs[i];
    if (cnt === 0 && b - a < BAND_JOIN_PX && bridged.length > 0 && i + 1 < rawSegs.length && rawSegs[i + 1][2] > 0) {
      bridged.push([a, b, Math.min(bridged[bridged.length - 1][2], rawSegs[i + 1][2])]);
    } else {
      bridged.push([a, b, cnt]);
    }
  }

  // Group positive segments into contiguous active islands, coalescing equal adjacent radii
  const islands = [];
  let cur = [];
  for (const [a, b, cnt] of bridged) {
    if (cnt <= 0) {
      if (cur.length) {
        islands.push(cur);
        cur = [];
      }
      continue;
    }
    const r = halfThicknessForCount(cnt, isSubLane);
    if (cur.length && Math.abs(cur[cur.length - 1][2] - r) < 1e-6) {
      cur[cur.length - 1][1] = b;
    } else {
      cur.push([a, b, r]);
    }
  }
  if (cur.length) islands.push(cur);
  return islands;
}

function capsulePath(a, b, cy, r) {
  const mid = (a + b) / 2;
  const xL = b - a <= 2 * r ? mid : a + r;
  const xR = b - a <= 2 * r ? mid : b - r;
  const top = (cy - r).toFixed(2);
  const bot = (cy + r).toFixed(2);
  const lx = xL.toFixed(2);
  const rx = xR.toFixed(2);
  const rf = r.toFixed(2);
  if (Math.abs(xR - xL) < 0.05) {
    return `M${lx} ${top}A${rf} ${rf} 0 0 1 ${lx} ${bot}A${rf} ${rf} 0 0 1 ${lx} ${top}Z`;
  }
  return `M${lx} ${top}L${rx} ${top}A${rf} ${rf} 0 0 1 ${rx} ${bot}L${lx} ${bot}A${rf} ${rf} 0 0 1 ${lx} ${top}Z`;
}

// Convert one island `[[x0, x1, r0], [x1, x2, r1], ...]` into a smooth C1 closed SVG path
// using closed-form triangular-window convolution: h(x) = (G(x+E) - 2*G(x) + G(x-E)) / E^2
function bandPath(island, cy) {
  const m = island.length;
  const a = island[0][0];
  const b = island[m - 1][1];
  const rStart = island[0][2];
  const rEnd = island[m - 1][2];

  // Fast path: uniform thickness or too short to fit internal transitions
  if (m === 1 || b - a <= rStart + rEnd + 2.0) {
    const rUniform = m === 1 ? rStart : Math.max(...island.map((s) => s[2]));
    return capsulePath(a, b, cy, rUniform);
  }

  const xL = a + rStart;
  const xR = b - rEnd;

  // Precompute knots x_k, first integral F_k, and double integral G_k relative to x_0 = a
  const knots = new Float64Array(m + 1);
  const radii = new Float64Array(m);
  const F = new Float64Array(m + 1);
  const G = new Float64Array(m + 1);
  knots[0] = a;
  for (let k = 0; k < m; k++) {
    const [, xNext, rk] = island[k];
    const dx = xNext - knots[k];
    knots[k + 1] = xNext;
    radii[k] = rk;
    F[k + 1] = F[k] + rk * dx;
    G[k + 1] = G[k] + F[k] * dx + 0.5 * rk * dx * dx;
  }

  function evalG(x) {
    if (x <= a) {
      const d = x - a;
      return 0.5 * rStart * d * d;
    }
    if (x >= b) {
      const d = x - b;
      return G[m] + F[m] * d + 0.5 * rEnd * d * d;
    }
    let lo = 0;
    let hi = m - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (knots[mid] <= x) lo = mid;
      else hi = mid - 1;
    }
    const d = x - knots[lo];
    return G[lo] + F[lo] * d + 0.5 * radii[lo] * d * d;
  }

  const E = BAND_EASE_PX;
  const invE2 = 1.0 / (E * E);
  const smoothH = (x) => (evalG(x + E) - 2.0 * evalG(x) + evalG(x - E)) * invE2;

  // Merge transition windows [x_k - E, x_k + E] clamped to [xL, xR]; sample 1px inside, skip flat spans
  const windows = [];
  for (let k = 1; k < m; k++) {
    const w0 = Math.max(xL, knots[k] - E);
    const w1 = Math.min(xR, knots[k] + E);
    if (w1 > w0) {
      if (windows.length && w0 <= windows[windows.length - 1][1] + 0.5) {
        windows[windows.length - 1][1] = Math.max(windows[windows.length - 1][1], w1);
      } else {
        windows.push([w0, w1]);
      }
    }
  }

  const xs = [xL];
  for (const [w0, w1] of windows) {
    if (w0 > xs[xs.length - 1] + 0.25) xs.push(w0);
    const startPx = Math.ceil(w0);
    const endPx = Math.floor(w1);
    for (let px = startPx; px <= endPx; px++) {
      if (px > xs[xs.length - 1] + 0.2 && px < xR - 0.2) xs.push(px);
    }
    if (w1 > xs[xs.length - 1] + 0.25 && w1 < xR - 0.2) xs.push(w1);
  }
  if (xR > xs[xs.length - 1] + 0.05) xs.push(xR);

  const hs = xs.map(smoothH);
  const n = xs.length;
  const hStart = hs[0];
  const hEnd = hs[n - 1];

  let d = `M${xs[0].toFixed(2)} ${(cy - hStart).toFixed(2)}`;
  for (let i = 1; i < n; i++) {
    d += `L${xs[i].toFixed(2)} ${(cy - hs[i]).toFixed(2)}`;
  }
  d += `A${hEnd.toFixed(2)} ${hEnd.toFixed(2)} 0 0 1 ${xs[n - 1].toFixed(2)} ${(cy + hEnd).toFixed(2)}`;
  for (let i = n - 2; i >= 0; i--) {
    d += `L${xs[i].toFixed(2)} ${(cy + hs[i]).toFixed(2)}`;
  }
  d += `A${hStart.toFixed(2)} ${hStart.toFixed(2)} 0 0 1 ${xs[0].toFixed(2)} ${(cy - hStart).toFixed(2)}Z`;
  return d;
}

// ============================================================================
// Build Timeline Lanes & Render Swimlane SVG
// ============================================================================

function selectVisibleLines(lines) {
  if (state.showAllLines || lines.length <= DEFAULT_VISIBLE_LINES) {
    return lines;
  }
  const picked = [];
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    if (
      i < DEFAULT_VISIBLE_LINES ||
      state.expandedListLines.has(l.id) ||
      state.expandedTimelineLines.has(l.id)
    ) {
      picked.push(l);
    }
  }
  return picked;
}

function buildTimelineLanes() {
  const isDay = state.mode === "day";
  const dIdx = state.selectedDayIdx;
  const lanes = [];

  const eligibleLines = [];
  for (const line of state.lines) {
    const st = lineStats(line, state.mode, dIdx);
    const windowSt = lineStats(line, "week", dIdx);
    // Pinned user lines keep their timeline lane even before any session runs today (unless filtered to another harness with 0 sessions)
    if (!line.pinned && st.agentMins === 0 && (isDay || windowSt.agentMins === 0)) continue;
    if (line.pinned && state.selectedHarness !== "all" && st.agentMins === 0 && windowSt.totalSessions === 0) continue;
    if (line.isSystemBucket && st.agentMins === 0) continue;
    eligibleLines.push({ line, st, windowSt });
  }

  const visibleEligible = state.showAllLines || eligibleLines.length <= DEFAULT_VISIBLE_LINES
    ? eligibleLines
    : eligibleLines.filter(
        ({ line }, idx) =>
          idx < DEFAULT_VISIBLE_LINES ||
          state.expandedListLines.has(line.id) ||
          state.expandedTimelineLines.has(line.id)
      );

  for (const { line, st, windowSt } of visibleEligible) {
    const displayColor = resolveLineColor(line.color);
    const expanded = state.expandedTimelineLines.has(line.id);
    lanes.push({
      kind: "parent",
      laneKey: `line:${line.id}`,
      lineId: line.id,
      label: line.name,
      color: displayColor,
      expanded,
      stats: st,
      sessions: line.sessions.filter((s) => !s.hidden && matchesHarness(s)),
    });

    if (expanded) {
      const subSessions = isDay ? st.activeSessions : windowSt.activeSessions;
      for (const s of subSessions) {
        lanes.push({
          kind: "sub",
          laneKey: `sub:${s.id}`,
          lineId: line.id,
          sessionId: s.id,
          parentName: line.name,
          label: s.title,
          color: displayColor,
          session: s,
        });
      }
    }
  }

  const unsummarizedActive = state.unsummarized.filter(
    (u) => matchesHarness(u) && (isDay ? u.bursts.some(([d]) => d === dIdx) : u.bursts.length > 0)
  );
  if (unsummarizedActive.length > 0) {
    lanes.push({
      kind: "unsummarized",
      laneKey: "line:__unsummarized__",
      lineId: "__unsummarized__",
      label: t("unsummarized"),
      color: "#7E828A",
      items: unsummarizedActive,
    });
  }

  if (state.settings.showShortOnTimeline) {
    const shortItems = (isDay
      ? state.shortSessions.items.filter((i) => i.dayIdx === dIdx)
      : state.shortSessions.items
    ).filter(matchesHarness);
    if ((state.dataSource === "mock" && state.selectedHarness === "all") || shortItems.length > 0) {
      lanes.push({
        kind: "short",
        laneKey: "line:__short__",
        lineId: "__short__",
        label: t("shortSessions"),
        color: "#9A9186",
        items: shortItems,
      });
    }
  }

  return lanes;
}

function getDayAxisRange(dayIdx, lanes, plotWidth = 640) {
  const day = state.days[dayIdx] || { isToday: false };
  let rawTicks;
  let tMin;
  let tMax;
  let nowHr = null;

  if (state.dataSource === "api") {
    let earliest = Infinity;
    let latest = -Infinity;
    for (const lane of lanes) {
      const sessList = lane.sessions || (lane.session ? [lane.session] : lane.items || []);
      for (const s of sessList) {
        for (const [d, a, b] of s.bursts || []) {
          if (d === dayIdx) {
            earliest = Math.min(earliest, a);
            latest = Math.max(latest, b);
          }
        }
      }
    }
    if (day.isToday && typeof state.nowHour === "number") {
      nowHr = state.nowHour;
      earliest = Number.isFinite(earliest) ? Math.min(earliest, nowHr - 0.5) : Math.max(0, nowHr - 4);
      latest = Math.max(latest, nowHr + 0.15);
    }
    if (!Number.isFinite(earliest) || !Number.isFinite(latest)) {
      tMin = 8;
      tMax = 20;
    } else {
      tMin = Math.max(0, Math.floor((earliest - 0.25) * 2) / 2);
      tMax = Math.min(24, Math.ceil((latest + 0.25) * 2) / 2);
      if (tMax - tMin < 2) {
        tMin = Math.max(0, Math.floor(tMin));
        tMax = Math.min(24, tMin + 2);
        if (tMax - tMin < 2) tMin = Math.max(0, tMax - 2);
      }
    }
    const span = tMax - tMin;
    const tickStep = span <= 3 ? 0.5 : span <= 8 ? 1 : span <= 14 ? 2 : 4;
    rawTicks = [];
    const firstTick = Math.ceil(tMin / tickStep) * tickStep;
    for (let t = firstTick; t <= tMax + 1e-6; t += tickStep) {
      rawTicks.push(Number(t.toFixed(2)));
    }
  } else if (day.isToday) {
    tMin = 0;
    tMax = 2.0;
    rawTicks = [0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75];
    nowHr = 0.14;
  } else {
    let earliest = 9.5;
    let latest = 16.5;
    for (const lane of lanes) {
      const sessList = lane.sessions || (lane.session ? [lane.session] : lane.items || []);
      for (const s of sessList) {
        for (const [d, a, b] of s.bursts || []) {
          if (d === dayIdx) {
            earliest = Math.min(earliest, a);
            latest = Math.max(latest, b);
          }
        }
      }
    }
    if (earliest < 4) {
      tMin = -0.35;
      tMax = 17.2;
      rawTicks = [0, 2, 4, 6, 8, 10, 12, 14, 16];
    } else {
      tMin = 9.6;
      tMax = 20.4;
      rawTicks = [10, 12, 14, 16, 18, 20];
    }
  }

  // Responsive tick thinning so labels never collide in narrow sidebars
  const step = plotWidth < 250 ? 3 : plotWidth < 390 ? 2 : 1;
  const ticks = rawTicks.filter((_, idx) => idx % step === 0);
  return { tMin, tMax, ticks, nowHr };
}

let lastRenderedLaneGeo = new Map();

function slicerTriggerIconSvg() {
  return `<svg class="slicer-glyph" width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
    <circle cx="2.3" cy="3.5" r="1.25" fill="currentColor"/>
    <circle cx="2.3" cy="10.5" r="1.25" fill="currentColor"/>
    <path d="M3.4 3.5C6.2 3.5 6.4 7 9.5 7M3.4 10.5C6.2 10.5 6.4 7 9.5 7" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
    <circle cx="11.3" cy="7" r="1.6" fill="currentColor"/>
  </svg>`;
}

function renderSwimlaneSvg(lanes) {
  const W = Math.max(160, Math.round(state.plotWidth || 644));
  let y = 0;
  const positioned = lanes.map((lane) => {
    const h = lane.kind === "sub" ? 18 : 22;
    const cy = y + h / 2;
    y += h;
    return { lane, cy, h };
  });
  const H = Math.max(44, y);
  const isDay = state.mode === "day";
  const dIdx = state.selectedDayIdx;
  const numDays = Math.max(1, state.days.length);
  const colW = W / numDays;

  let svg = "";

  if (!isDay) {
    const step = numDays > 14 ? 5 : 1;
    for (let d = step; d < numDays; d += step) {
      const gx = (d * colW).toFixed(1);
      svg += `<line x1="${gx}" y1="2" x2="${gx}" y2="${H}" stroke="var(--grid-hairline)" stroke-width="1"/>`;
    }
  }

  const dayRange = isDay ? getDayAxisRange(dIdx, lanes, W) : null;
  const xOfDay = isDay ? (t) => ((t - dayRange.tMin) / (dayRange.tMax - dayRange.tMin)) * W : null;
  const xOfWindow = (d, hr) => d * colW + (hr / 24) * colW;

  const nextLaneGeo = new Map();

  for (const { lane, cy } of positioned) {
    const isHovered = state.hoveredLaneId === lane.laneKey;
    const isSub = lane.kind === "sub";
    nextLaneGeo.set(lane.laneKey, {
      cy,
      color: lane.color,
      isSub,
      isDay,
      dIdx,
      numDays,
      xOfDay,
      xOfWindow,
    });
    svg += `<g class="lane-group ${isHovered ? "is-hovered" : ""}" style="--hue:${lane.color}" data-lane-key="${
      lane.laneKey
    }" data-line-id="${lane.lineId || ""}">`;
    svg += `<line x1="0" y1="${cy}" x2="${W}" y2="${cy}" stroke="var(--grid-hairline)" stroke-width="1"/>`;
    svg += `<line class="lane-hover-rule" x1="0" y1="${cy}" x2="${W}" y2="${cy}" stroke="var(--lane-hover-stroke)" stroke-width="1"/>`;
    svg += `<rect x="0" y="${cy - 10}" width="${W}" height="20" fill="transparent" style="cursor:pointer"/>`;

    const sessSource =
      lane.kind === "parent"
        ? lane.sessions
        : lane.kind === "sub"
        ? [lane.session]
        : lane.kind === "unsummarized" || lane.kind === "short"
        ? lane.items || []
        : [];

    const sessionPixelLists = [];
    const hoverTargets = [];
    const promptDots = [];

    if (isDay) {
      for (const s of sessSource) {
        const spans = [];
        for (const b of s.bursts) {
          const [bd, t0, t1, prompts] = b;
          if (bd !== dIdx) continue;
          const x1 = xOfDay(t0);
          const x2 = xOfDay(t1);
          spans.push([x1, x2]);
          hoverTargets.push({
            x1: Math.min(x1, x2) - 3,
            x2: Math.max(x1, x2) + 3,
            tip: {
              kind: "day-bar",
              title: lane.label,
              sub: `${fmtClock(t0)}–${fmtClock(t1)} · ${burstMinutes(b)} min`,
            },
          });
          for (const p of prompts || []) promptDots.push(p);
        }
        if (spans.length) sessionPixelLists.push(spans);
      }
    } else {
      for (const s of sessSource) {
        const spans = [];
        for (const [bd, t0, t1] of s.bursts || []) {
          if (bd < 0 || bd >= numDays) continue;
          const x1 = xOfWindow(bd, t0);
          const x2 = xOfWindow(bd, t1);
          spans.push([x1, x2]);
          hoverTargets.push({
            x1: Math.min(x1, x2) - 3,
            x2: Math.max(x1, x2) + 3,
            tip: {
              kind: "week-bar",
              main: `${state.days[bd]?.short || ""}, ${fmtClock(Math.floor(t0))}–${fmtClock(Math.ceil(t1))}`,
            },
          });
        }
        if (spans.length) sessionPixelLists.push(spans);
      }
    }

    const islands = bandProfile(sessionPixelLists, isSub);
    for (const island of islands) {
      const pathD = bandPath(island, cy);
      svg += `<path class="tl-band ${isSub ? "is-sub" : ""}" d="${pathD}"/>`;
    }

    for (const ht of hoverTargets) {
      const hw = Math.max(7, ht.x2 - ht.x1);
      svg += `<rect class="tl-hover" data-tip="${escapeHtml(JSON.stringify(ht.tip))}" x="${ht.x1.toFixed(
        1
      )}" y="${(cy - 6).toFixed(1)}" width="${hw.toFixed(1)}" height="12" fill="transparent" style="cursor:pointer"/>`;
    }

    // Hover-only user prompt dots above Day lanes (with 2-tier anti-collision stacking)
    if (isDay && promptDots.length > 0) {
      promptDots.sort((a, b) => a - b);
      svg += `<g class="lane-prompt-dots">`;
      let lastTier0X = -999;
      for (const p of promptDots) {
        const px = xOfDay(p);
        let py = cy - 5.8;
        if (px - lastTier0X < 4.4) {
          py = cy - 9.2;
        } else {
          lastTier0X = px;
        }
        svg += `<circle cx="${px.toFixed(1)}" cy="${py.toFixed(1)}" r="1.55" fill="${lane.color}" fill-opacity="0.9"/>`;
      }
      svg += `</g>`;
    }

    svg += `</g>`;
  }

  lastRenderedLaneGeo = nextLaneGeo;
  svg += `<g id="tl-session-spotlight" class="tl-session-spotlight"></g>`;

  if (isDay && dayRange.nowHr !== null) {
    const nx = xOfDay(dayRange.nowHr).toFixed(1);
    svg += `<line x1="${nx}" y1="3" x2="${nx}" y2="${H + 3}" stroke="var(--accent-cursor)" stroke-width="1.15"/>`;
    svg += `<circle cx="${nx}" cy="3" r="2.2" fill="var(--accent-cursor)"/>`;
  } else if (!isDay) {
    const todayIdx = state.dataSource === "api" ? state.days.findIndex((d) => d.isToday) : numDays - 1;
    const weekNowHr = state.dataSource === "api" ? state.nowHour : 1.8;
    if (todayIdx >= 0 && typeof weekNowHr === "number") {
      const nx = xOfWindow(todayIdx, weekNowHr).toFixed(1);
      svg += `<line x1="${nx}" y1="3" x2="${nx}" y2="${H + 3}" stroke="var(--accent-cursor)" stroke-width="1.15"/>`;
      svg += `<circle cx="${nx}" cy="3" r="2.2" fill="var(--accent-cursor)"/>`;
    }
  }

  return `<svg class="swimlane-svg" viewBox="0 0 ${W} ${H}" style="height:${H}px">${svg}</svg>`;
}

function buildLabelTooltip(lane) {
  if (lane.kind === "parent") {
    const st = lane.stats;
    const footer =
      st.activeSessions.length === 1
        ? st.activeSessions[0].title
        : lang() === "zh"
        ? `共 ${st.activeSessions.length} 个会话，点击查看明细`
        : `${st.activeSessions.length} sessions. Click to see each one.`;
    return JSON.stringify({
      kind: "line-summary",
      title: lane.label,
      agentTime: fmtMinutesCompact(st.agentMins),
      wallClock: fmtMinutesCompact(st.wallMins),
      tokensText: st.tokens > 0 ? `${fmtTokensCompact(st.tokens)} tok` : "",
      costText: st.costUsd > 0 ? fmtCostCompact(st.costUsd) : "",
      footer,
    });
  }
  if (lane.kind === "sub") {
    const isDay = state.mode === "day";
    const mins = isDay
      ? sessionMinutesOnDay(lane.session, state.selectedDayIdx)
      : sessionMinutesInWeek(lane.session);
    const usage = sessionUsageInView(lane.session, isDay, state.selectedDayIdx);
    return JSON.stringify({
      kind: "sub-summary",
      title: lane.label,
      activeText: `${fmtMinutesCompact(mins)}`,
      tokensText: usage.tokens > 0 ? `${fmtTokensCompact(usage.tokens)} tok` : "",
      costText: usage.costUsd > 0 ? fmtCostCompact(usage.costUsd) : "",
    });
  }
  if (lane.kind === "unsummarized" || lane.kind === "short") {
    const isDay = state.mode === "day";
    const dIdx = state.selectedDayIdx;
    const items = lane.items || [];
    let mins = 0;
    let tokens = 0;
    let costUsd = 0;
    for (const it of items) {
      if (Array.isArray(it.bursts)) {
        for (const b of it.bursts) {
          if (!isDay || b[0] === dIdx) mins += burstMinutes(b);
        }
      } else {
        mins += Number(it.mins || 0);
      }
      const usage = sessionUsageInView(it, isDay, dIdx);
      tokens += usage.tokens;
      costUsd += usage.costUsd;
    }
    if (lane.kind === "short" && !isDay && state.selectedHarness === "all") {
      mins = Number(state.shortSessions.weekMinutes || mins);
      tokens = Number(state.shortSessions.weekTokens || tokens);
      costUsd = Number(state.shortSessions.weekCostUsd || costUsd);
    }
    return JSON.stringify({
      kind: "sub-summary",
      title: lane.label,
      activeText: fmtMinutesCompact(mins),
      tokensText: tokens > 0 ? `${fmtTokensCompact(tokens)} tok` : "",
      costText: costUsd > 0 ? fmtCostCompact(costUsd) : "",
    });
  }
  return "";
}

// Organize sessions into top-level + indented children (`dependsOnId` within same line)
function orderSessionsWithDependencies(sessions) {
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const childrenMap = new Map();
  const roots = [];
  for (const s of sessions) {
    if (s.dependsOnId && byId.has(s.id) && byId.has(s.dependsOnId) && s.dependsOnId !== s.id) {
      if (!childrenMap.has(s.dependsOnId)) childrenMap.set(s.dependsOnId, []);
      childrenMap.get(s.dependsOnId).push(s);
    } else {
      roots.push(s);
    }
  }
  const ordered = [];
  const visit = (node, depth) => {
    ordered.push({ session: node, isChild: depth > 0 });
    for (const child of childrenMap.get(node.id) || []) {
      visit(child, depth + 1);
    }
  };
  for (const r of roots) visit(r, 0);
  return ordered;
}

function findSessionEverywhere(sessionId) {
  for (const line of state.lines) {
    const found = line.sessions.find((s) => s.id === sessionId);
    if (found) return { session: found, line };
  }
  const unsum = state.unsummarized.find((u) => u.id === sessionId);
  if (unsum) return { session: unsum, line: null };
  return null;
}

function buildUnsummarizedSummary(u) {
  const parts = [];
  if (u.harness) parts.push(`[${harnessLabel(u.harness)}]`);
  if (typeof u.userInputCount === "number") {
    parts.push(`${u.userInputCount} ${lang() === "zh" ? "条提问" : "user prompts"}`);
  }
  parts.push(`${u.steps || 0} ${t("stepsWord")}`);
  if (u.cwd) parts.push(u.cwd);
  return lang() === "zh"
    ? `${parts.join(" · ")}。暂未生成摘要 —— 可分配到工作线或点击同步。`
    : `${parts.join(" · ")}. Not summarized yet — assign to a Line or run Sync.`;
}

function resolveUpstreamBadge(depId) {
  const inLine = findSessionEverywhere(depId);
  if (inLine) {
    return {
      title: inLine.session.title || inLine.session.firstPrompt || "",
      color: resolveLineColor(inLine.line ? inLine.line.color : "#7E828A"),
    };
  }
  const sh = state.shortSessions.items.find((s) => s.id === depId);
  if (sh) return { title: sh.prompt, color: "#8A8F98" };
  return null;
}

function localizedRangeLabel() {
  const daysCount = state.mode === "month" ? 30 : rangeDays();
  if (daysCount === 30) return t("range30");
  if (daysCount === 14) return t("range14");
  return t("range7");
}

function localizedRangeShort() {
  const daysCount = state.mode === "month" ? 30 : rangeDays();
  if (daysCount === 30) return "30d";
  if (daysCount === 14) return "14d";
  return "7d";
}

function ensureMockWindowDays(targetDays) {
  if (state.dataSource !== "mock") return;
  const prevLen = state.days.length || 7;
  if (prevLen === targetDays) return;
  const end = new Date("2026-11-14T12:00:00Z");
  const wdEn = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const wdZh = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  const moEn = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const nextDays = [];
  for (let i = 0; i < targetDays; i++) {
    const offsetFromEnd = targetDays - 1 - i;
    const d = new Date(end.getTime() - offsetFromEnd * 86400000);
    const iso = d.toISOString().slice(0, 10);
    const wd = d.getUTCDay();
    const dom = d.getUTCDate();
    const mo = moEn[d.getUTCMonth()];
    const isToday = offsetFromEnd === 0;
    const isZh = lang() === "zh";
    nextDays.push({
      idx: i,
      isoDate: iso,
      short: isZh ? `${mo} ${dom}` : `${wdEn[wd]} ${dom}`,
      tiny: isZh ? `${dom}` : wdEn[wd],
      full: isToday ? (isZh ? "今天" : "Today") : isZh ? `${mo} ${dom} · ${wdZh[wd]}` : `${wdEn[wd]}, ${mo} ${dom}`,
      isToday,
    });
  }
  const delta = targetDays - prevLen;
  state.days = nextDays;
  state.selectedDayIdx = Math.max(0, Math.min(targetDays - 1, state.selectedDayIdx + delta));
  for (const line of state.lines) {
    for (const s of line.sessions) {
      if (Array.isArray(s.bursts)) {
        s.bursts = s.bursts.map((b) => [b[0] + delta, b[1], b[2], b[3], b[4]]);
      }
    }
  }
  for (const u of state.unsummarized) {
    if (Array.isArray(u.bursts)) {
      u.bursts = u.bursts.map((b) => [b[0] + delta, b[1], b[2], b[3], b[4]]);
    }
  }
  for (const sh of state.shortSessions.items) {
    sh.dayIdx = (sh.dayIdx || 0) + delta;
    if (Array.isArray(sh.bursts)) {
      sh.bursts = sh.bursts.map((b) => [b[0] + delta, b[1], b[2], b[3], b[4]]);
    }
  }
}

function harnessSessionCounts() {
  const counts = { total: 0, antigravity: 0, claude: 0, codex: 0, dsh: 0 };
  for (const line of state.lines) {
    if (line.isSystemBucket) continue;
    for (const s of line.sessions) {
      if (s.hidden) continue;
      const h = s.harness || "codex";
      counts.total += 1;
      if (Object.hasOwn(counts, h)) counts[h] += 1;
    }
  }
  for (const u of state.unsummarized) {
    const h = u.harness || "codex";
    counts.total += 1;
    if (Object.hasOwn(counts, h)) counts[h] += 1;
  }
  return counts;
}

function syncRefreshIconSvg() {
  return `<svg class="slicer-glyph" width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path d="M13.2 8a5.2 5.2 0 1 1-1.45-3.6M13.4 2.7v2.4H11" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/>
  </svg>`;
}

function settingsSlidersIconSvg() {
  return `<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path d="M2.2 4.8h11.6M2.2 11.2h11.6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
    <circle cx="5.6" cy="4.8" r="1.8" fill="var(--bg-canvas)" stroke="currentColor" stroke-width="1.3"/>
    <circle cx="10.4" cy="11.2" r="1.8" fill="var(--bg-canvas)" stroke="currentColor" stroke-width="1.3"/>
  </svg>`;
}

function backArrowSvg() {
  return `<svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
    <path d="M8.6 2.8L4.4 7l4.2 4.2" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"/>
  </svg>`;
}

let savedPulseTimer = 0;

function markSettingsSaved() {
  state.settingsSavedPulse = true;
  const badge = document.querySelector(".settings-save-status");
  if (badge) {
    badge.classList.add("is-pulsing");
    const txt = badge.querySelector(".save-label");
    if (txt) txt.textContent = t("savedJustNow");
  }
  if (savedPulseTimer) clearTimeout(savedPulseTimer);
  savedPulseTimer = setTimeout(() => {
    state.settingsSavedPulse = false;
    const curBadge = document.querySelector(".settings-save-status");
    if (curBadge) {
      curBadge.classList.remove("is-pulsing");
      const txt = curBadge.querySelector(".save-label");
      if (txt) txt.textContent = t("autoSaveReady");
    }
  }, 1600);
}

function persistSettings() {
  markSettingsSaved();
  if (state.dataSource !== "api") return;
  void (async () => {
    try {
      const res = await fetch(`/api/settings?${apiQuerySuffix()}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(state.settings),
      });
      if (!res.ok) return;
      const data = await res.json();
      lastApiRefreshMs = Date.now();
      const activeTag = document.activeElement?.tagName;
      if (activeTag === "INPUT" || activeTag === "TEXTAREA") {
        if (Array.isArray(data.lines)) state.lines = data.lines;
        if (Array.isArray(data.unsummarized)) state.unsummarized = data.unsummarized;
        if (data.shortSessions) state.shortSessions = data.shortSessions;
        if (Array.isArray(data.contributions)) state.contributions = data.contributions;
        if (Array.isArray(data.harnesses)) state.harnesses = data.harnesses;
        if (typeof data.archivedCount === "number") state.archivedCount = data.archivedCount;
        return;
      }
      applyPlaneResponse(data);
      render();
    } catch {
      // Ignore transient offline save errors
    }
  })();
}

function openSettingsPage(tab) {
  if (tab && tab !== state.settingsTab) {
    state.settingsTab = tab;
    state.resetSettingsScroll = true;
  }
  state.popover = null;
  state.confirmResetDefaults = false;
  if (tooltipEl) tooltipEl.hidden = true;
  if (state.route !== "settings") {
    state.savedMainScrollY = window.scrollY || 0;
    state.route = "settings";
    state.routeMotion = "enter-settings";
    state.resetSettingsScroll = true;
    render();
  } else {
    render();
  }
}

function closeSettingsPage() {
  state.popover = null;
  state.confirmResetDefaults = false;
  if (tooltipEl) tooltipEl.hidden = true;
  if (state.route === "settings") {
    state.route = "main";
    state.routeMotion = "enter-main";
    render();
    window.scrollTo(0, state.savedMainScrollY || 0);
  }
}

function renderInlineHarnessLens() {
  const counts = harnessSessionCounts();
  const statusById = new Map((state.harnesses || []).map((h) => [h.id, h]));
  const visibleIds = HARNESS_ORDER.filter((id) => {
    const meta = statusById.get(id);
    if (meta && meta.enabled === false && state.selectedHarness !== id) return false;
    return (counts[id] || 0) > 0 || state.selectedHarness === id;
  });

  if (visibleIds.length === 0 && state.selectedHarness === "all") return "";

  const hasSolo = state.selectedHarness !== "all";
  return `
    <span class="topbar-sep" aria-hidden="true"></span>
    <div class="harness-inline" role="group" aria-label="Harness filter">
      ${visibleIds
        .map((id) => {
          const cnt = counts[id] || 0;
          const active = state.selectedHarness === id;
          const dimmed = hasSolo && !active;
          const tip = JSON.stringify({
            kind: "simple",
            main: `${harnessLabel(id)} · ${cnt} ${t("contribSessions")}`,
          });
          return `<button class="harness-chip tl-hover ${active ? "is-active is-filtered-source" : ""} ${
            dimmed ? "is-dimmed" : ""
          }"
            data-action="filter-harness" data-harness="${id}" data-tip="${escapeHtml(tip)}" aria-pressed="${
            active ? "true" : "false"
          }">
            ${harnessIconSvg(id, 13.5)}
            ${active ? `<span class="harness-chip-name">${escapeHtml(harnessLabel(id))}</span>` : ""}
            <span class="harness-chip-count">${cnt}</span>
          </button>`;
        })
        .join("")}
    </div>
  `;
}

function renderTopbar() {
  const rangeTxt = state.anchorDate
    ? `${localizedRangeShort()} · ${state.anchorDate.slice(5)}`
    : localizedRangeLabel();
  const rangeShortTxt = state.anchorDate ? state.anchorDate.slice(5) : localizedRangeShort();
  const isRangeOpen = state.popover?.type === "range-menu";
  const isThemeOpen = state.popover?.type === "theme-menu";
  const pendingCount = state.unsummarized.length;
  const curTheme = state.settings.theme || DEFAULT_SETTINGS.theme;

  const rangeControlHtml = state.anchorDate
    ? `<div class="range-anchor-group">
        <button class="btn-outline has-anchor is-grouped-main ${isRangeOpen ? "is-open" : ""}" data-action="open-range-menu">
          <span class="anchor-dot" aria-hidden="true"></span>
          <span class="range-full">${escapeHtml(rangeTxt)}</span>
          <span class="range-short">${escapeHtml(rangeShortTxt)}</span>
          <span style="color:var(--ink-muted)">${chevronSvg("down")}</span>
        </button>
        <button class="btn-outline is-grouped-clear" data-action="reset-anchor-date" title="${escapeHtml(
          t("contribBackToday")
        )} (T)" aria-label="${escapeHtml(t("contribBackToday"))}">×</button>
      </div>`
    : `<button class="btn-outline ${isRangeOpen ? "is-open" : ""}" data-action="open-range-menu">
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true"><rect x="2" y="3" width="12" height="11" rx="2" stroke="currentColor" stroke-width="1.3"/><path d="M5 1.8v2.4M11 1.8v2.4M2 6.5h12" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
        <span class="range-full">${escapeHtml(rangeTxt)}</span>
        <span class="range-short">${escapeHtml(rangeShortTxt)}</span>
        <span style="color:var(--ink-muted)">${chevronSvg("down")}</span>
      </button>`;

  return `
    <header class="topbar">
      <div class="topbar-left">
        <div class="brand">
          ${planarLogoSvg()}
          <span>Planar</span>
        </div>
        ${renderInlineHarnessLens()}
      </div>
      <div class="topbar-actions">
        ${rangeControlHtml}
        <button class="btn-sync ${pendingCount > 0 ? "has-pending" : ""} ${state.isSyncing ? "is-syncing" : ""}"
          data-action="sync-now" title="${escapeHtml(t("syncTip"))}">
          ${pendingCount > 0 || state.isSyncing ? slicerTriggerIconSvg() : syncRefreshIconSvg()}
          <span>${escapeHtml(state.isSyncing ? t("syncing") : pendingCount > 0 ? t("sync") : t("syncIdle"))}</span>
          ${pendingCount > 0 ? `<span class="sync-badge">${pendingCount}</span>` : ""}
        </button>
        <button class="btn-outline btn-icon ${isThemeOpen ? "is-open" : ""}"
          data-action="open-theme-menu"
          title="${escapeHtml(t("lblTheme"))}"
          aria-label="${escapeHtml(t("lblTheme"))}">
          ${themeIconSvg(curTheme)}
        </button>
        <button class="btn-outline btn-icon"
          data-action="open-settings"
          title="${escapeHtml(t("navSettings"))} (⌘,)"
          aria-label="${escapeHtml(t("navSettings"))}">
          ${settingsSlidersIconSvg()}
        </button>
      </div>
    </header>
  `;
}

function renderSyncProgressStrip() {
  if (!state.isSyncing) return "";
  const done = state.syncCompletedCount || state.syncProcessedTotal || 0;
  const total = Math.max(done, state.syncInitialPending || state.unsummarized.length || 1);
  const pct = Math.min(100, Math.max(0, Math.round((done / Math.max(1, total)) * 100)));
  const batchItems = Array.isArray(state.syncBatchItems) ? state.syncBatchItems : [];
  const leadActive = batchItems[0] || null;
  const extraInBatch = Math.max(0, batchItems.length - 1);
  const bannerLabel =
    state.syncTotalBatches > 0
      ? t("syncBannerBatched", {
          batch: Math.min(state.syncBatchIndex || 1, state.syncTotalBatches),
          batches: state.syncTotalBatches,
          done,
          total,
        })
      : t("syncBannerRunning", { done, total });

  let subRowHtml = "";
  if (leadActive || state.syncLastFiled) {
    const activeHtml = leadActive
      ? `<span class="sync-sub-active">
           <span class="sync-sub-tag">${escapeHtml(
             t("syncBatchActiveTag", { n: batchItems.length })
           )} · ${escapeHtml(harnessLabel(leadActive.harness))}</span>
           <span class="sync-sub-title">${escapeHtml(leadActive.label)}</span>
           ${
             extraInBatch > 0
               ? `<span class="sync-sub-chip">${escapeHtml(
                   t("syncBatchMore", { n: extraInBatch })
                 )}</span>`
               : ""
           }
         </span>`
      : "";
    const filedHtml = state.syncLastFiled
      ? `<span class="sync-sub-filed">
           <span class="sync-sub-line-tag">${escapeHtml(
             t("syncFiledInto", { line: state.syncLastFiled.lineName })
           )}</span>
           <span class="sync-sub-filed-title">${escapeHtml(state.syncLastFiled.title)}</span>
           ${
             state.syncLastFiled.extraCount > 0
               ? `<span class="sync-sub-chip">${escapeHtml(
                   t("syncFiledMore", { n: state.syncLastFiled.extraCount })
                 )}</span>`
               : ""
           }
         </span>`
      : "";
    subRowHtml = `<div class="sync-progress-sub">${activeHtml}${filedHtml}</div>`;
  }

  return `
    <div class="sync-progress-strip">
      <div class="scan-line-bar" aria-hidden="true">
        <span class="scan-line-fill" style="width:${pct}%"></span>
      </div>
      <div class="sync-progress-inner">
        <span class="sync-progress-text">
          ${slicerTriggerIconSvg()}
          <span>${escapeHtml(bannerLabel)}</span>
          ${
            state.syncFailedCount > 0
              ? `<span class="sync-sub-chip is-warn">${escapeHtml(
                  t("syncFailedBadge", { n: state.syncFailedCount })
                )}</span>`
              : ""
          }
        </span>
        <button class="btn-quiet sync-stop-btn" data-action="stop-sync-after-batch">
          ${escapeHtml(state.syncStopRequested ? t("syncStopping") : t("syncStopAfterBatch"))}
        </button>
      </div>
      ${subRowHtml}
    </div>
  `;
}

function contribLevelForMinutes(mins) {
  if (!mins || mins <= 0) return 0;
  if (mins < 30) return 1;
  if (mins < 90) return 2;
  if (mins < 180) return 3;
  return 4;
}

function renderContributionPopoverBody() {
  const rawDays = Array.isArray(state.contributions) ? state.contributions : [];
  if (rawDays.length === 0) return "";

  const isZh = lang() === "zh";
  const monthNames = isZh
    ? ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"]
    : ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const weekdays = isZh ? ["一", "", "三", "", "五", "", ""] : ["Mon", "", "Wed", "", "Fri", "", ""];

  const leadPad = Math.max(0, Number(rawDays[0]?.weekday || 0));
  const padded = [];
  for (let i = 0; i < leadPad; i++) padded.push(null);
  for (const d of rawDays) padded.push(d);

  let totalMins = 0;
  let totalTokens = 0;
  let totalCostUsd = 0;
  let activeDays = 0;
  for (const d of rawDays) {
    const mins =
      state.selectedHarness === "all"
        ? Number(d.totalMinutes || 0)
        : Number(d.byHarness?.[state.selectedHarness] || 0);
    const tok =
      state.selectedHarness === "all"
        ? Number(d.totalTokens || 0)
        : Number(d.tokensByHarness?.[state.selectedHarness] || 0);
    const cost =
      state.selectedHarness === "all"
        ? Number(d.costUsd || 0)
        : Number(d.costByHarness?.[state.selectedHarness] || 0);
    totalMins += mins;
    totalTokens += tok;
    totalCostUsd += cost;
    if (mins > 0) activeDays += 1;
  }
  const contribCostStr = fmtCostCompact(totalCostUsd);
  const contribUsageSuffix =
    totalTokens > 0
      ? ` · ${fmtTokensCompact(totalTokens)} tok${contribCostStr ? ` (${contribCostStr})` : ""}`
      : "";

  const colCount = Math.ceil(padded.length / 7);
  const monthLabels = [];
  let lastMonth = -1;
  let lastPlacedCol = -3;
  for (let col = 0; col < colCount; col++) {
    const firstValid = padded.slice(col * 7, col * 7 + 7).find((x) => x !== null);
    if (!firstValid?.date) continue;
    const monthIdx = Number.parseInt(firstValid.date.slice(5, 7), 10) - 1;
    if (monthIdx !== lastMonth && monthIdx >= 0 && monthIdx < 12) {
      if (col - lastPlacedCol >= 2) {
        monthLabels.push(`<span class="contrib-month-lbl" style="left:${col * 14}px">${monthNames[monthIdx]}</span>`);
        lastPlacedCol = col;
      }
      lastMonth = monthIdx;
    }
  }

  return `
    <div class="horizon-meta-row">
      <span class="contrib-summary">${escapeHtml(fmtMinutesCompact(totalMins))} · ${activeDays} ${escapeHtml(
        t("contribDaysActive")
      )}${escapeHtml(contribUsageSuffix)}</span>
      ${
        state.anchorDate
          ? `<button class="contrib-anchor-pill" data-action="reset-anchor-date">${escapeHtml(
              state.anchorDate
            )} · ${escapeHtml(t("contribBackToday"))} ×</button>`
          : ""
      }
    </div>
    <div class="contrib-scroll">
      <div class="contrib-months" style="width:${colCount * 14}px">${monthLabels.join("")}</div>
      <div class="contrib-body">
        <div class="contrib-weekdays">${weekdays.map((w) => `<div>${w}</div>`).join("")}</div>
        <div class="contrib-grid">
          ${padded
            .map((d) => {
              if (!d) return `<div class="contrib-cell" style="opacity:0;pointer-events:none"></div>`;
              const mins =
                state.selectedHarness === "all"
                  ? Number(d.totalMinutes || 0)
                  : Number(d.byHarness?.[state.selectedHarness] || 0);
              const dayTokens =
                state.selectedHarness === "all"
                  ? Number(d.totalTokens || 0)
                  : Number(d.tokensByHarness?.[state.selectedHarness] || 0);
              const dayCostUsd =
                state.selectedHarness === "all"
                  ? Number(d.costUsd || 0)
                  : Number(d.costByHarness?.[state.selectedHarness] || 0);
              const lvl = contribLevelForMinutes(mins);
              const tipPayload = JSON.stringify({
                kind: "contrib-day",
                date: d.date,
                totalMinutes: mins,
                totalTokens: dayTokens,
                costUsd: dayCostUsd,
                sessionCount: Number(d.sessionCount || 0),
                byHarness: d.byHarness || {},
              });
              return `<button class="contrib-cell tl-hover ${
                state.anchorDate === d.date ? "is-selected" : ""
              }" data-level="${lvl}" data-action="select-contrib-day" data-date="${
                d.date
              }" data-tip="${escapeHtml(tipPayload)}"></button>`;
            })
            .join("")}
        </div>
      </div>
      <div class="contrib-footer">
        <span>${escapeHtml(rawDays[0]?.date || "")} → ${escapeHtml(rawDays[rawDays.length - 1]?.date || "")}</span>
        <span class="contrib-legend">
          <span>${escapeHtml(t("contribLess"))}</span>
          <span class="contrib-legend-box" style="background:var(--contrib-0)"></span>
          <span class="contrib-legend-box" style="background:var(--contrib-1)"></span>
          <span class="contrib-legend-box" style="background:var(--contrib-2)"></span>
          <span class="contrib-legend-box" style="background:var(--contrib-3)"></span>
          <span class="contrib-legend-box" style="background:var(--contrib-4)"></span>
          <span>${escapeHtml(t("contribMore"))}</span>
        </span>
      </div>
    </div>
  `;
}

function renderRecapCard(session, line) {
  const isUnassigned = !line;
  const upstream = session.dependsOnId ? resolveUpstreamBadge(session.dependsOnId) : null;
  const stoppedText = session.stoppedAt || buildUnsummarizedSummary(session);
  const rawTitleText = session.rawTitle || session.firstPrompt || "";
  const lineColor = resolveLineColor(line ? line.color : "#7E828A");
  const lineName = line ? line.name : t("unassignedLine");
  const harnessId = session.harness || "codex";

  const sessTokens = Number(session.totalTokens || 0);
  const sessCostStr = fmtCostCompact(session.costUsd);
  const usageSuffix =
    sessTokens > 0
      ? ` · ${fmtTokensCompact(sessTokens)} tok${sessCostStr ? ` · ${sessCostStr}` : ""}`
      : "";

  return `
    <div class="recap-card">
      <div class="recap-body">
        <div class="recap-eyebrow-row">
          <div class="recap-eyebrow">${escapeHtml(isUnassigned ? t("trajPreview") : t("recap"))}</div>
          <span class="recap-harness-chip">
            ${harnessIconSvg(harnessId, 11)}
            <span>${escapeHtml(harnessLabel(harnessId))}</span>
          </span>
        </div>
        <div class="recap-block">
          <div class="recap-block-label">
            <svg width="13" height="13" viewBox="0 0 14 14" fill="none"><path d="M3 2.2v9.6M3 2.8h6.6l-1.4 2.3 1.4 2.3H3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
            <span>${escapeHtml(isUnassigned ? t("summary") : t("stoppedAt"))}</span>
          </div>
          <div class="recap-block-text">${escapeHtml(stoppedText)}</div>
        </div>
        ${
          session.leftOpen
            ? `<div class="recap-block">
                <div class="recap-block-label">
                  <svg width="13" height="13" viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7" r="4.8" stroke="currentColor" stroke-width="1.2" stroke-dasharray="2.1 2.1"/></svg>
                  <span>${escapeHtml(t("leftOpen"))}</span>
                </div>
                <div class="recap-block-text">${escapeHtml(session.leftOpen)}</div>
              </div>`
            : ""
        }

        <div class="recap-meta-grid">
          <div class="recap-meta-key">
            <svg width="12" height="12" viewBox="0 0 14 14" fill="none"><path d="M2.2 3.8h9.6M2.2 7h6.6M2.2 10.2h8.2" stroke="currentColor" stroke-width="1.25" stroke-linecap="round"/></svg>
            <span>${escapeHtml(t("lineLabel"))}</span>
          </div>
          <div class="recap-meta-val">
            <button class="recap-pill-btn" data-action="open-reassign-line" data-session-id="${session.id}">
              <span class="line-swatch" style="background:${lineColor}"></span>
              <span class="recap-pill-text">${escapeHtml(lineName)}</span>
              <span style="color:var(--ink-muted)">${chevronSvg("down")}</span>
            </button>
          </div>

          ${
            line
              ? `<div class="recap-meta-key">
                  <svg width="12" height="12" viewBox="0 0 14 14" fill="none"><circle cx="4.2" cy="3.8" r="1.7" stroke="currentColor" stroke-width="1.2"/><circle cx="9.8" cy="10.2" r="1.7" stroke="currentColor" stroke-width="1.2"/><path d="M4.2 5.6v2.2c0 1.3 1 2.4 2.3 2.4h1.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
                  <span>${escapeHtml(t("dependsOn"))}</span>
                </div>
                <div class="recap-meta-val">
                  ${
                    upstream
                      ? `<button class="recap-pill-btn" data-action="unlink-upstream" data-session-id="${session.id}" title="${escapeHtml(
                          t("unlinkHint")
                        )}">
                          <span class="line-swatch" style="background:${upstream.color}"></span>
                          <span class="recap-pill-text">${escapeHtml(upstream.title)}</span>
                          <span style="color:var(--ink-muted);margin-left:2px;flex-shrink:0">×</span>
                        </button>`
                      : `<button class="recap-pill-btn is-ghost" data-action="open-upstream-picker" data-session-id="${session.id}" data-line-id="${line.id}">
                          <span class="recap-pill-text">${escapeHtml(t("linkUpstream"))}</span>
                        </button>`
                  }
                </div>`
              : ""
          }
        </div>
      </div>

      <div class="recap-footer">
        <div class="recap-raw-meta">
          <svg width="12" height="12" viewBox="0 0 14 14" fill="none" style="flex-shrink:0"><path d="M2.2 3.2h9.6v6.6H5.4L2.2 12V3.2z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
          <span class="recap-raw-title">${escapeHtml(rawTitleText)}</span>
          <span class="recap-steps">· ${(session.steps || 0).toLocaleString()} ${escapeHtml(t("stepsWord"))}${escapeHtml(usageSuffix)}</span>
        </div>
        <div class="recap-footer-actions">
          <button class="btn-quiet" data-action="hide-session" data-session-id="${session.id}">${escapeHtml(t("hideBtn"))}</button>
          ${
            session.sourceExists === false
              ? `<span class="recap-pruned-tag" title="${escapeHtml(t("sourcePrunedTip"))}">${escapeHtml(
                  t("sourcePruned")
                )}</span>`
              : `<button class="btn-open-session" data-action="open-external-session" data-session-id="${session.id}">
                  <span>${escapeHtml(t("openBtn"))}</span>
                  <span>↗</span>
                </button>`
          }
        </div>
      </div>
    </div>
  `;
}

function renderLineSessionRow(s, line, lineColor, { isChild = false, isArchived = false } = {}) {
  const sExpanded = state.expandedSessionIds.has(s.id);
  const harnessId = s.harness || "codex";
  return `
    <div class="session-item ${isChild ? "is-child-item" : ""} ${isArchived ? "is-archived-item" : ""}">
      <div class="session-row ${isChild ? "is-child" : ""} ${isArchived ? "is-archived-row" : ""} ${
    sExpanded ? "is-expanded" : ""
  }"
        draggable="true"
        data-action="toggle-session-recap"
        data-session-id="${s.id}"
        data-line-id="${line.id}"
        data-is-child="${isChild ? "1" : "0"}"
        data-hover-lane="sub:${s.id}"
        data-spotlight-session="${s.id}"
        data-spotlight-lane="line:${line.id}">
        ${isChild ? `<span class="tree-elbow"></span>` : ""}
        <div class="session-left">
          <span class="icon-slot">${sessionIndicatorSvg(lineColor, {
            isRunning: Boolean(s.running),
          })}</span>
          <span class="harness-badge">${harnessIconSvg(harnessId, 13)}</span>
          <span class="session-title">${escapeHtml(s.title)}</span>
          ${isArchived ? `<span class="session-archived-tag">${escapeHtml(t("archivedBadge"))}</span>` : ""}
        </div>
        <span class="session-age">${escapeHtml(localizeRecency(s.recency))}</span>
      </div>
      ${sExpanded ? renderRecapCard(s, line) : ""}
    </div>
  `;
}

function renderLineSection(line, maxMins, maxTokens, maxCost) {
  const isDay = state.mode === "day";
  const dIdx = state.selectedDayIdx;
  const st = lineStats(line, state.mode, dIdx);
  const weekSt = lineStats(line, isDay ? "week" : state.mode, dIdx);
  const expanded = state.expandedListLines.has(line.id);
  const lineColor = resolveLineColor(line.color);

  let countLabel = `${st.totalSessions}`;
  if (isDay && st.activeSessions.length > 0 && st.activeSessions.length < st.totalSessions) {
    countLabel = `${st.activeSessions.length} / ${st.totalSessions}`;
  } else if (!isDay && weekSt.activeSessions.length < weekSt.totalSessions) {
    countLabel = `${weekSt.activeSessions.length} / ${weekSt.totalSessions}`;
  }

  const showAll = state.showAllIdleInLine.has(line.id);
  const visiblePool = line.sessions.filter((s) => !s.hidden && matchesHarness(s));
  const activePool = visiblePool.filter((s) => !s.archived);
  const archivedPool = visiblePool.filter((s) => s.archived);
  const activeSet = new Set(st.activeSessions.map((s) => s.id));

  const filteredSessions =
    isDay && st.activeSessions.length > 0 && !showAll
      ? activePool.filter((s) => activeSet.has(s.id) || (s.dependsOnId && activeSet.has(s.dependsOnId)))
      : activePool.slice(0, state.settings.sessionsShownPerLine);

  const hiddenIdleCount =
    isDay && st.activeSessions.length > 0 ? activePool.length - filteredSessions.length : 0;

  const ordered = orderSessionsWithDependencies(filteredSessions);
  const archivedExpanded = state.expandedArchivedInLine.has(line.id);
  const currentDay = state.days[dIdx] || { isToday: false, full: "" };
  const whenSuffix = currentDay.isToday
    ? t("todayWhen")
    : t("onWhen", { date: currentDay.full });

  const isMenuOpen =
    state.popover &&
    (state.popover.type === "line-menu" || state.popover.type === "merge-line-picker") &&
    state.popover.lineId === line.id;

  return `
    <div class="line-block">
      <div class="line-row ${isMenuOpen ? "is-menu-open" : ""}"
        ${!line.isSystemBucket ? `draggable="true" data-drag-line-id="${line.id}"` : ""}
        data-drop-line-id="${line.id}"
        data-action="toggle-list-line"
        data-line-id="${line.id}"
        data-hover-lane="line:${line.id}">
        <div class="line-left">
          <span class="icon-slot"><span class="line-swatch" style="background:${lineColor}"></span></span>
          <span class="line-name">${escapeHtml(line.name)}</span>
          ${line.status !== "active" ? `<span class="line-status-tag">${escapeHtml(line.status)}</span>` : ""}
          <span class="line-count">${countLabel}</span>
          ${
            !line.isSystemBucket
              ? `<button class="line-menu-trigger ${line.priority > 0 ? "has-priority" : ""}"
                  data-action="open-line-menu" data-line-id="${line.id}" title="${escapeHtml(t("lineMenuHint"))}">
                  ${priorityIconSvg(line.priority)}
                </button>`
              : ""
          }
        </div>
        <div class="line-right">
          ${renderMetricRightCell({
            mins: st.agentMins,
            maxMins,
            tokens: st.tokens,
            maxTokens,
            costUsd: st.costUsd,
            maxCost,
            pricedSessions: st.pricedSessions,
            totalSessions: st.activeSessions.length,
            barColor: lineColor,
            minBarPct: 10,
          })}
          <span class="line-chevron">${expanded ? chevronSvg("down") : chevronSvg("right")}</span>
        </div>
      </div>

      ${
        expanded
          ? `<div class="session-list">
              ${ordered
                .map(({ session: s, isChild }) => renderLineSessionRow(s, line, lineColor, { isChild }))
                .join("")}

              ${
                hiddenIdleCount > 0 && !showAll
                  ? `<button class="more-idle-btn" data-action="expand-idle-sessions" data-line-id="${line.id}">
                      ${escapeHtml(t("moreIdle", { n: hiddenIdleCount, when: whenSuffix }))}
                    </button>`
                  : isDay && showAll && activePool.length > st.activeSessions.length && st.activeSessions.length > 0
                  ? `<button class="more-idle-btn" data-action="collapse-idle-sessions" data-line-id="${line.id}">
                      ${escapeHtml(t("showFewer"))}
                    </button>`
                  : ""
              }

              ${
                archivedPool.length > 0
                  ? `<button class="more-idle-btn is-archived-toggle" data-action="toggle-archived-sessions" data-line-id="${line.id}">
                      ${archivedExpanded ? "▾" : "▸"} ${escapeHtml(
                        archivedExpanded
                          ? t("archivedHide", { n: archivedPool.length })
                          : t("archivedFold", { n: archivedPool.length })
                      )}
                    </button>
                    ${
                      archivedExpanded
                        ? archivedPool
                            .map((s) => renderLineSessionRow(s, line, lineColor, { isArchived: true }))
                            .join("")
                        : ""
                    }`
                  : ""
              }
            </div>`
          : ""
      }
    </div>
  `;
}

function renderMainView(motionCls = "") {
  const isDay = state.mode === "day";
  const isMonth = state.mode === "month";
  const dIdx = Math.min(state.selectedDayIdx, Math.max(0, state.days.length - 1));
  const dayMeta = state.days[dIdx] || { idx: 0, short: "", tiny: "", full: "Today", isToday: true };
  const lanes = buildTimelineLanes();

  const activeLines = [];
  const idleLines = [];
  for (const l of state.lines) {
    const st = lineStats(l, state.mode, dIdx);
    const windowSt = lineStats(l, isDay ? "week" : state.mode, dIdx);
    if (st.agentMins > 0) activeLines.push(l);
    else if (windowSt.agentMins > 0) idleLines.push(l);
  }

  const unsummarizedAll = state.unsummarized.filter((u) => matchesHarness(u));
  const unsummarizedInView = unsummarizedAll.filter((u) =>
    isDay ? u.bursts.some(([d]) => d === dIdx) : true
  );
  const unsummarizedMins = unsummarizedInView.reduce((acc, u) => {
    for (const b of u.bursts) {
      if (!isDay || b[0] === dIdx) acc += burstMinutes(b);
    }
    return acc;
  }, 0);
  let unsummarizedTokens = 0;
  let unsummarizedCostUsd = 0;
  let unsummarizedPriced = 0;
  for (const u of unsummarizedInView) {
    const usage = sessionUsageInView(u, isDay, dIdx);
    unsummarizedTokens += usage.tokens;
    if (usage.costUsd > 0) {
      unsummarizedCostUsd += usage.costUsd;
      unsummarizedPriced += 1;
    }
  }

  const shortPool = state.shortSessions.items.filter(
    (i) => state.selectedHarness === "all" || !i.harness || i.harness === state.selectedHarness
  );
  const shortItemsInView = isDay ? shortPool.filter((i) => i.dayIdx === dIdx) : shortPool;
  const shortMins = isDay
    ? shortItemsInView.reduce((a, i) => a + i.mins, 0)
    : state.selectedHarness === "all"
    ? state.shortSessions.weekMinutes
    : shortPool.reduce((a, i) => a + i.mins, 0);
  const shortTokens =
    !isDay && state.selectedHarness === "all" && typeof state.shortSessions.weekTokens === "number"
      ? state.shortSessions.weekTokens
      : shortItemsInView.reduce((a, i) => a + Number(i.totalTokens || 0), 0);
  const shortCostUsd =
    !isDay && state.selectedHarness === "all" && typeof state.shortSessions.weekCostUsd === "number"
      ? state.shortSessions.weekCostUsd
      : shortItemsInView.reduce((a, i) => a + Number(i.costUsd || 0), 0);
  const shortPriced = shortItemsInView.filter((i) => Number(i.costUsd || 0) > 0).length;

  const activeSessionTotal =
    activeLines.reduce((acc, l) => acc + lineStats(l, state.mode, dIdx).activeSessions.length, 0) +
    unsummarizedInView.length;
  const activeLineTotal = activeLines.length + (unsummarizedInView.length > 0 ? 1 : 0);

  const maxMins = Math.max(
    60,
    ...activeLines.map((l) => lineStats(l, state.mode, dIdx).agentMins),
    unsummarizedMins,
    shortMins
  );
  const maxTokens = Math.max(
    1000,
    ...activeLines.map((l) => lineStats(l, state.mode, dIdx).tokens),
    unsummarizedTokens,
    shortTokens
  );
  const maxCost = Math.max(
    0.05,
    ...activeLines.map((l) => lineStats(l, state.mode, dIdx).costUsd),
    unsummarizedCostUsd,
    shortCostUsd
  );

  const displayedActiveLines = selectVisibleLines(activeLines);
  const hiddenLinesCount = Math.max(0, activeLines.length - displayedActiveLines.length);

  const W = Math.max(150, Math.round(state.plotWidth || 644));
  const axisInfo = isDay ? getDayAxisRange(dIdx, lanes, W) : null;
  const compactWeekAxis = W < 340;
  const useCompactAxis = isMonth || state.days.length > 10;

  const windowContextText = !isDay
    ? t("windowLastShort", { n: state.days.length || rangeDays() })
    : dayMeta.isToday
    ? t("today")
    : dayMeta.full;
  const curLens = activeMetricLens();

  const unsummarizedExpanded = state.expandedListLines.has("__unsummarized__");
  const unsummarizedColor = "#7E828A";
  const shortExpanded = state.expandedListLines.has("__short__");

  const timelineTitleText = isDay
    ? dayMeta.isToday
      ? t("today")
      : dayMeta.full
    : localizedRangeLabel();
  const lineWord = activeLineTotal === 1 ? t("lineSingle") : t("linePlural");
  const sessWord = activeSessionTotal === 1 ? t("sessionSingle") : t("sessionPlural");

  let axisHtml = "";
  if (isDay && axisInfo) {
    axisHtml = axisInfo.ticks
      .map((tk) => {
        const rawPct = ((tk - axisInfo.tMin) / (axisInfo.tMax - axisInfo.tMin)) * 100;
        const edgeCls = rawPct < 6 ? "is-edge-start" : rawPct > 94 ? "is-edge-end" : "";
        const clampedPct = Math.max(0, Math.min(100, rawPct)).toFixed(2);
        return `<span class="${edgeCls}" style="left:${clampedPct}%">${fmtClock(tk)}</span>`;
      })
      .join("");
  } else if (useCompactAxis) {
    const step = Math.max(2, Math.ceil(state.days.length / (W < 380 ? 5 : 7)));
    const selectedIndices = [];
    for (let i = 0; i < state.days.length; i += step) selectedIndices.push(i);
    const lastIdx = state.days.length - 1;
    if (lastIdx > 0 && lastIdx - selectedIndices[selectedIndices.length - 1] >= Math.max(2, Math.floor(step / 2))) {
      selectedIndices.push(lastIdx);
    } else if (lastIdx > 0) {
      selectedIndices[selectedIndices.length - 1] = lastIdx;
    }
    axisHtml = selectedIndices
      .map((i) => {
        const d = state.days[i];
        const pct = (((d.idx + 0.5) / Math.max(1, state.days.length)) * 100).toFixed(2);
        const edgeCls = i === 0 ? "is-edge-start" : i === lastIdx ? "is-edge-end" : "";
        return `<button class="week-day-btn month-day-btn ${edgeCls} ${d.isToday ? "is-today" : ""}" style="left:${pct}%" data-action="select-day" data-day-idx="${d.idx}" title="${escapeHtml(d.full)}">${escapeHtml(d.short)}</button>`;
      })
      .join("");
  } else {
    axisHtml = state.days
      .map(
        (d) =>
          `<button class="week-day-btn ${d.isToday ? "is-today" : ""}" data-action="select-day" data-day-idx="${
            d.idx
          }" title="${escapeHtml(d.full)}">${escapeHtml(compactWeekAxis ? d.tiny : d.short)}</button>`
      )
      .join("");
  }

  return `
    <div class="plane-page ${motionCls}">
      ${renderTopbar()}

      <section class="timeline-section">
        <div class="timeline-header">
          <div class="timeline-title-group">
            <span class="timeline-title">${escapeHtml(timelineTitleText)}</span>
            <span class="timeline-meta">${activeLineTotal} ${escapeHtml(lineWord)} · ${activeSessionTotal} ${escapeHtml(
    sessWord
  )}<span class="tz-suffix"> · ${escapeHtml(state.tzLabel)}</span></span>
          </div>
          <div class="segmented" role="tablist">
            <button class="segmented-btn ${state.mode === "day" ? "is-active" : ""}" data-action="set-mode" data-mode="day">${escapeHtml(
    t("modeDay")
  )}</button>
            <button class="segmented-btn ${state.mode === "week" ? "is-active" : ""}" data-action="set-mode" data-mode="week">${escapeHtml(
    t("modeWeek")
  )}</button>
            <button class="segmented-btn ${state.mode === "month" ? "is-active" : ""}" data-action="set-mode" data-mode="month">${escapeHtml(
    t("modeMonth")
  )}</button>
          </div>
        </div>

        <div class="swimlane-wrap">
          <div class="swimlane-labels">
            ${lanes
              .map((lane) => {
                const tip = buildLabelTooltip(lane);
                return `<div class="swimlane-label ${lane.kind === "sub" ? "is-sub" : ""} ${
                  state.hoveredLaneId === lane.laneKey ? "is-hovered" : ""
                } ${tip ? "tl-hover" : ""}"
                  data-action="click-timeline-lane"
                  data-line-id="${lane.lineId || ""}"
                  data-lane-key="${lane.laneKey}"
                  ${tip ? `data-tip="${escapeHtml(tip)}"` : ""}>${escapeHtml(lane.label)}</div>`;
              })
              .join("")}
          </div>
          <div class="swimlane-plot-area">
            ${renderSwimlaneSvg(lanes)}
            <div class="swimlane-axis ${isDay ? "is-day" : useCompactAxis ? "is-month" : "is-week"}">
              ${axisHtml}
            </div>
          </div>
        </div>
      </section>

      <section>
        <div class="list-column-header">
          <span class="metric-window-context">${escapeHtml(windowContextText)}</span>
          <div class="metric-lens-switch" role="tablist" aria-label="Metric lens">
            <button type="button" class="metric-lens-btn is-icon-tab ${curLens === "time" ? "is-active" : ""}"
              data-action="set-metric-lens" data-lens="time"
              title="${escapeHtml(`${t("lensTime")} · ${t("agentTimeTip")}`)}"
              aria-label="${escapeHtml(t("lensTime"))}"
              aria-selected="${curLens === "time" ? "true" : "false"}">
              ${metricLensIconSvg("time")}
            </button>
            <button type="button" class="metric-lens-btn is-icon-tab ${curLens === "tokens" ? "is-active" : ""}"
              data-action="set-metric-lens" data-lens="tokens"
              title="${escapeHtml(`${t("lensTokens")} · ${t("tokensTip")}`)}"
              aria-label="${escapeHtml(t("lensTokens"))}"
              aria-selected="${curLens === "tokens" ? "true" : "false"}">
              ${metricLensIconSvg("tokens")}
            </button>
            <button type="button" class="metric-lens-btn is-icon-tab ${curLens === "cost" ? "is-active" : ""}"
              data-action="set-metric-lens" data-lens="cost"
              title="${escapeHtml(`${t("lensCost")} · ${t("costTip")}`)}"
              aria-label="${escapeHtml(t("lensCost"))}"
              aria-selected="${curLens === "cost" ? "true" : "false"}">
              ${metricLensIconSvg("cost")}
            </button>
          </div>
        </div>

        <div class="lines-list is-lens-${curLens}">
          ${displayedActiveLines.map((l) => renderLineSection(l, maxMins, maxTokens, maxCost)).join("")}

          ${
            hiddenLinesCount > 0 || (state.showAllLines && activeLines.length > DEFAULT_VISIBLE_LINES)
              ? `<button type="button" class="lines-overflow-toggle" data-action="toggle-show-all-lines">
                  ${escapeHtml(
                    hiddenLinesCount > 0
                      ? t("showMoreLines", { n: hiddenLinesCount })
                      : t("showFewerLines", { n: DEFAULT_VISIBLE_LINES })
                  )}
                </button>`
              : ""
          }

          ${renderSyncProgressStrip()}

          ${
            unsummarizedInView.length > 0
              ? `<div class="line-block">
                  <div class="line-row" data-action="toggle-list-line" data-line-id="__unsummarized__" data-hover-lane="line:__unsummarized__">
                    <div class="line-left">
                      <span class="icon-slot"><span class="line-swatch" style="background:${unsummarizedColor}"></span></span>
                      <span class="line-name">${escapeHtml(t("unsummarized"))}</span>
                      <span class="line-count">${
                        isDay && unsummarizedInView.length < unsummarizedAll.length
                          ? `${unsummarizedInView.length} / ${unsummarizedAll.length}`
                          : `${unsummarizedAll.length}`
                      }</span>
                    </div>
                    <div class="line-right">
                      ${renderMetricRightCell({
                        mins: unsummarizedMins,
                        maxMins,
                        tokens: unsummarizedTokens,
                        maxTokens,
                        costUsd: unsummarizedCostUsd,
                        maxCost,
                        pricedSessions: unsummarizedPriced,
                        totalSessions: unsummarizedInView.length,
                        barColor: unsummarizedColor,
                        minBarPct: 10,
                        forceShowZeroTime: true,
                      })}
                      <span class="line-chevron">${unsummarizedExpanded ? chevronSvg("down") : chevronSvg("right")}</span>
                    </div>
                  </div>
                  ${
                    unsummarizedExpanded
                      ? `<div class="session-list">
                          ${unsummarizedInView
                            .map((u) => {
                              const uExpanded = state.expandedSessionIds.has(u.id);
                              const harnessId = u.harness || "codex";
                              return `<div class="session-item">
                                <div class="session-row ${uExpanded ? "is-expanded" : ""}"
                                  data-action="toggle-session-recap"
                                  data-session-id="${u.id}"
                                  data-spotlight-session="${u.id}"
                                  data-spotlight-lane="line:__unsummarized__">
                                  <div class="session-left">
                                    <span class="icon-slot">${sessionIndicatorSvg(unsummarizedColor, {
                                      isRunning: Boolean(u.running),
                                    })}</span>
                                    <span class="harness-badge">${harnessIconSvg(harnessId, 13)}</span>
                                    <span class="session-title">${escapeHtml(u.firstPrompt)}</span>
                                  </div>
                                  <span class="session-age">${escapeHtml(localizeRecency(u.recency))}</span>
                                </div>
                                ${uExpanded ? renderRecapCard(u, null) : ""}
                              </div>`;
                            })
                            .join("")}
                        </div>`
                      : ""
                  }
                </div>`
              : ""
          }

          <div class="line-block">
            <div class="line-row" data-action="toggle-list-line" data-line-id="__short__" data-hover-lane="line:__short__">
              <div class="line-left">
                <span class="icon-slot"><span class="line-swatch is-hollow"></span></span>
                <span class="line-name">${escapeHtml(t("shortSessions"))}</span>
                <span class="line-count">${
                  state.dataSource === "api"
                    ? `${isDay ? shortItemsInView.length : shortPool.length} / ${state.shortSessions.totalCount}`
                    : isDay
                    ? `${Math.max(1, shortItemsInView.length)} / ${state.shortSessions.totalCount}`
                    : `${shortPool.length} / ${state.shortSessions.totalCount}`
                }</span>
              </div>
              <div class="line-right">
                ${renderMetricRightCell({
                  mins: state.dataSource === "api" ? shortMins : Math.max(1, shortMins),
                  maxMins,
                  tokens: shortTokens,
                  maxTokens,
                  costUsd: shortCostUsd,
                  maxCost,
                  pricedSessions: shortPriced,
                  totalSessions: shortItemsInView.length,
                  barColor: "#9A9186",
                  minBarPct: 8,
                  forceShowZeroTime: true,
                })}
                <span class="line-chevron">${shortExpanded ? chevronSvg("down") : chevronSvg("right")}</span>
              </div>
            </div>
            ${
              shortExpanded
                ? `<div class="session-list">
                    ${
                      shortItemsInView
                        .map(
                          (it) => `<div class="session-row"
                            data-spotlight-session="${it.id}"
                            data-spotlight-lane="line:__short__">
                            <div class="session-left">
                              <span class="icon-slot">${sessionIndicatorSvg("#9A9186", { isShort: true })}</span>
                              ${it.harness ? `<span class="harness-badge">${harnessIconSvg(it.harness, 13)}</span>` : ""}
                              <span class="session-title">${escapeHtml(it.prompt)}</span>
                            </div>
                            <span class="session-age">${escapeHtml(localizeRecency(it.recency))}</span>
                          </div>`
                        )
                        .join("") ||
                      `<div class="session-row"><span class="session-age">${escapeHtml(t("noShort"))}</span></div>`
                    }
                  </div>`
                : ""
            }
          </div>

          ${
            isDay && idleLines.length > 0
              ? `<button class="idle-groups-toggle" data-action="toggle-idle-groups">
                  ${escapeHtml(
                    state.hideIdleGroups
                      ? t("idleGroupsToggle", {
                          n: idleLines.length,
                          when: dayMeta.isToday ? t("todayWhen") : dayMeta.short,
                        })
                      : t("hideIdleGroupsToggle", {
                          when: dayMeta.isToday ? t("todayWhen") : dayMeta.short,
                        })
                  )}
                </button>
                ${!state.hideIdleGroups ? idleLines.map((l) => renderLineSection(l, maxMins, maxTokens, maxCost)).join("") : ""}`
              : ""
          }
        </div>
      </section>
    </div>
  `;
}

function renderSwitch(field, isOn) {
  return `<button type="button" class="switch ${isOn ? "is-on" : ""}" data-action="toggle-setting" data-field="${field}" aria-pressed="${
    isOn ? "true" : "false"
  }">
    <span class="switch-knob"></span>
  </button>`;
}

function settingsNavIconSvg(tabId) {
  if (tabId === "lines") {
    return slicerTriggerIconSvg();
  }
  if (tabId === "processor") {
    return `<svg width="13.5" height="13.5" viewBox="0 0 14 14" fill="none" aria-hidden="true"><rect x="1.6" y="2.2" width="10.8" height="9.6" rx="2" stroke="currentColor" stroke-width="1.25"/><path d="M4.1 5.3l2 1.7-2 1.7M7.3 8.7h2.5" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
  }
  if (tabId === "harnesses") {
    return `<svg width="13.5" height="13.5" viewBox="0 0 14 14" fill="none" aria-hidden="true"><circle cx="7" cy="7" r="5.2" stroke="currentColor" stroke-width="1.25"/><circle cx="7" cy="7" r="2.1" stroke="currentColor" stroke-width="1.2"/><path d="M7 1.8v1.6M7 10.6v1.6M1.8 7h1.6M10.6 7h1.6" stroke="currentColor" stroke-width="1.15" stroke-linecap="round"/></svg>`;
  }
  return `<svg width="13.5" height="13.5" viewBox="0 0 14 14" fill="none" aria-hidden="true"><circle cx="7" cy="7" r="4.9" stroke="currentColor" stroke-width="1.25"/><path d="M7 2.1a4.9 4.9 0 0 1 0 9.8V2.1z" fill="currentColor"/></svg>`;
}

function renderSettingsView(motionCls = "") {
  const s = state.settings;
  const tab = state.settingsTab || "lines";
  const pinnedLines = state.lines.filter((l) => l.pinned && !l.isSystemBucket);
  const discoveredLines = state.lines.filter((l) => !l.pinned && !l.isSystemBucket);
  const statusById = new Map((state.harnesses || []).map((h) => [h.id, h]));

  const virtualTotal = Math.max(120, s.openingSteps + s.closingSteps + 40);
  const openBasis = Math.max(11, Math.min(68, (s.openingSteps / virtualTotal) * 100));
  const closeBasis = Math.max(11, Math.min(78 - openBasis, (s.closingSteps / virtualTotal) * 100));
  const midBasis = Math.max(14, 100 - openBasis - closeBasis);

  const tabs = [
    { id: "lines", label: t("tabLines") },
    { id: "processor", label: t("tabProcessor") },
    { id: "harnesses", label: t("tabHarnesses") },
    { id: "general", label: t("tabGeneral") },
  ];

  let tabBody = "";
  if (tab === "general") {
    const curLang = lang();
    const curTheme = s.theme || DEFAULT_SETTINGS.theme;
    const curAutoSync = s.autoSync || DEFAULT_SETTINGS.autoSync;
    const isLangOpen = state.popover?.type === "lang-select";
    const isShortOpen = state.popover?.type === "short-threshold-select";

    tabBody = `
      <section class="settings-section">
        <h2 class="settings-section-heading">${escapeHtml(t("secAppearance"))}</h2>
        <div class="settings-card">
          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblLang"))}</div>
            <button type="button" class="ui-select-btn ${isLangOpen ? "is-open" : ""}"
              data-action="open-lang-select" aria-haspopup="listbox">
              <span class="ui-select-left">
                <span class="ui-select-text">${escapeHtml(langOptionLabel(curLang))}</span>
              </span>
              <span class="ui-select-caret">${chevronSvg("down")}</span>
            </button>
          </div>

          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblTheme"))}</div>
            <div class="segmented" role="radiogroup" aria-label="${escapeHtml(t("lblTheme"))}">
              <button type="button" class="segmented-btn is-icon-seg ${curTheme === "system" ? "is-active" : ""}"
                data-action="set-setting-val" data-field="theme" data-val="system">
                ${themeIconSvg("system")}
                <span>${escapeHtml(t("themeSystem"))}</span>
              </button>
              <button type="button" class="segmented-btn is-icon-seg ${curTheme === "light" ? "is-active" : ""}"
                data-action="set-setting-val" data-field="theme" data-val="light">
                ${themeIconSvg("light")}
                <span>${escapeHtml(t("themeLight"))}</span>
              </button>
              <button type="button" class="segmented-btn is-icon-seg ${curTheme === "dark" ? "is-active" : ""}"
                data-action="set-setting-val" data-field="theme" data-val="dark">
                ${themeIconSvg("dark")}
                <span>${escapeHtml(t("themeDark"))}</span>
              </button>
            </div>
          </div>
        </div>
      </section>

      <section class="settings-section">
        <h2 class="settings-section-heading">${escapeHtml(t("secAutoDashboard"))}</h2>
        <div class="settings-card">
          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblAutoSync"))}</div>
            <div class="segmented" role="radiogroup" aria-label="${escapeHtml(t("lblAutoSync"))}">
              <button type="button" class="segmented-btn ${curAutoSync === "Every day" ? "is-active" : ""}"
                data-action="set-setting-val" data-field="autoSync" data-val="Every day">${escapeHtml(
                  t("optEveryDay")
                )}</button>
              <button type="button" class="segmented-btn ${curAutoSync === "Every hour" ? "is-active" : ""}"
                data-action="set-setting-val" data-field="autoSync" data-val="Every hour">${escapeHtml(
                  t("optEveryHour")
                )}</button>
              <button type="button" class="segmented-btn ${curAutoSync === "Manual only" ? "is-active" : ""}"
                data-action="set-setting-val" data-field="autoSync" data-val="Manual only">${escapeHtml(
                  t("optManual")
                )}</button>
            </div>
          </div>

          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblContrib"))}</div>
            ${renderSwitch("showContributionGraph", Boolean(s.showContributionGraph))}
          </div>

          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblShowShort"))}</div>
            <div class="settings-inline-controls">
              <button type="button" class="ui-select-btn is-compact ${isShortOpen ? "is-open" : ""}"
                data-action="open-short-threshold-select" aria-haspopup="listbox">
                <span class="ui-select-left">
                  <span class="ui-select-text">${escapeHtml(
                    t("shortThresholdBtn", { n: s.fewestUserInputs })
                  )}</span>
                </span>
                <span class="ui-select-caret">${chevronSvg("down")}</span>
              </button>
              ${renderSwitch("showShortOnTimeline", Boolean(s.showShortOnTimeline))}
            </div>
          </div>

          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblPerLine"))}</div>
            <div class="stepper">
              <button type="button" data-action="step" data-field="sessionsShownPerLine" data-delta="-1">−</button>
              <span>${s.sessionsShownPerLine}</span>
              <button type="button" data-action="step" data-field="sessionsShownPerLine" data-delta="1">+</button>
            </div>
          </div>
        </div>

        <div class="reset-footer">
          <button type="button" class="btn-outline is-reset-btn ${
            state.confirmResetDefaults ? "is-confirming" : ""
          }" data-action="reset-defaults">
            ${escapeHtml(state.confirmResetDefaults ? t("resetConfirm") : t("resetDefaults"))}
          </button>
        </div>
      </section>
    `;
  } else if (tab === "processor") {
    const isCli = (s.processorMode || DEFAULT_SETTINGS.processorMode) !== "api";
    const curCli = (s.cliHarness || "").trim();
    const curApi = (s.apiProvider || DEFAULT_SETTINGS.apiProvider).trim() || DEFAULT_SETTINGS.apiProvider;
    const provMeta = getApiProviderMeta(curApi);
    const apiModelHint =
      curApi === "gemini" || curApi === "openrouter"
        ? t("descApiModelGemini", {
            defaultModel: provMeta.defaultModel,
            fastModel:
              curApi === "openrouter" ? "google/gemini-3.5-flash-lite" : "gemini-3.5-flash-lite",
          })
        : "";
    const selectedCliMeta = curCli && curCli !== "auto" ? statusById.get(curCli) : null;
    const showMissingCliBanner = Boolean(isCli && selectedCliMeta && !selectedCliMeta.cliAvailable && !s.cliBinOverride);
    const isCliSelectOpen = state.popover?.type === "cli-select";
    const isApiSelectOpen = state.popover?.type === "api-select";

    let cliSelectInnerHtml = `<span class="ui-select-text">${escapeHtml(t("selectCliPlaceholder"))}</span>`;
    if (curCli === "auto") {
      cliSelectInnerHtml = `<span class="ui-select-text">${escapeHtml(t("cliAutoOption"))}</span>`;
    } else if (curCli) {
      const cliReady = Boolean(selectedCliMeta?.cliAvailable);
      cliSelectInnerHtml = `
        <span class="avail-dot ${cliReady ? "is-ready" : "is-missing"}" aria-hidden="true"></span>
        ${harnessIconSvg(curCli, 13, { colored: true })}
        <span class="ui-select-text">${escapeHtml(harnessFullLabel(curCli))}</span>
      `;
    }

    const cliSelectBtnHtml = `
      <button type="button" class="ui-select-btn ${!curCli ? "is-placeholder" : ""} ${
        isCliSelectOpen ? "is-open" : ""
      }" id="cli-engine-select-btn" data-action="open-cli-select" aria-haspopup="listbox">
        <span class="ui-select-left">${cliSelectInnerHtml}</span>
        <span class="ui-select-caret">${chevronSvg("down")}</span>
      </button>
    `;

    let consoleStatusHtml = "";
    if (state.isSyncing) {
      const done = state.syncCompletedCount || state.syncProcessedTotal || 0;
      const total = Math.max(done, state.syncInitialPending || state.unsummarized.length || 1);
      const bannerLabel =
        state.syncTotalBatches > 0
          ? t("syncBannerBatched", {
              batch: Math.min(state.syncBatchIndex || 1, state.syncTotalBatches),
              batches: state.syncTotalBatches,
              done,
              total,
            })
          : t("syncBannerRunning", { done, total });
      const lead = (state.syncBatchItems || [])[0];
      consoleStatusHtml = `
        <span class="proc-readout">
          <span>${escapeHtml(bannerLabel)}</span>
          ${lead ? `<span>·</span><span>${escapeHtml(lead.label)}</span>` : ""}
        </span>
      `;
    } else if (state.processorTestResult) {
      const ok = Boolean(state.processorTestResult.ok);
      consoleStatusHtml = `
        <span class="proc-readout ${ok ? "is-ok" : "is-err"}">
          <span>${ok ? "✓" : "✕"}</span>
          <span>${escapeHtml(state.processorTestResult.backendUsed)}</span>
          ${
            state.processorTestResult.latencyMs > 0
              ? `<span>·</span><span>${state.processorTestResult.latencyMs} ms</span>`
              : ""
          }
          <span>·</span>
          <span>${escapeHtml(state.processorTestResult.message)}</span>
        </span>
      `;
    } else if (state.lastSync) {
      const hasErr = Boolean(state.lastSync.lastError);
      consoleStatusHtml = `
        <span class="proc-readout ${hasErr ? "is-err" : "is-muted"}">
          <span>${escapeHtml(state.lastSync.backendUsed)}</span>
          <span>·</span>
          <span>${state.lastSync.processed} ok / ${state.lastSync.failed} fail</span>
          ${hasErr ? `<span>· ${escapeHtml(state.lastSync.lastError)}</span>` : ""}
        </span>
      `;
    } else {
      consoleStatusHtml = `
        <span class="proc-readout is-muted">${escapeHtml(
          isProcessorConfigured(s)
            ? isCli
              ? `cli:${curCli}`
              : `api:${curApi}`
            : t("procNeedSelectHint")
        )}</span>
      `;
    }

    tabBody = `
      <section class="settings-section">
        <div class="settings-section-bar">
          <h2 class="settings-section-heading">${escapeHtml(t("procTitle"))}</h2>
          <div class="segmented" role="radiogroup" aria-label="${escapeHtml(t("procTitle"))}">
            <button type="button" class="segmented-btn ${isCli ? "is-active" : ""}" data-action="set-processor-mode" data-mode="cli">
              ${escapeHtml(t("procModeCliShort"))}
            </button>
            <button type="button" class="segmented-btn ${!isCli ? "is-active" : ""}" data-action="set-processor-mode" data-mode="api">
              ${escapeHtml(t("procModeApiShort"))}
            </button>
          </div>
        </div>

        <div class="settings-card">
          ${
            isCli
              ? `${
                  showMissingCliBanner
                    ? `<div class="settings-row-block">
                        <div class="settings-row-top">
                          <div class="settings-row-title">${escapeHtml(t("lblCliHarness"))}</div>
                          ${cliSelectBtnHtml}
                        </div>
                        <div class="inline-hint-banner">${escapeHtml(
                          t("cliMissingHint", { cli: HARNESS_META[curCli]?.cli || curCli })
                        )}</div>
                      </div>`
                    : `<div class="settings-row">
                        <div class="settings-row-title">${escapeHtml(t("lblCliHarness"))}</div>
                        ${cliSelectBtnHtml}
                      </div>`
                }
                <div class="settings-row">
                  <div class="settings-row-title">${escapeHtml(t("lblCliModel"))}</div>
                  <input class="settings-input is-wide is-mono" data-action="setting-text" data-field="cliModel" value="${escapeHtml(
                    s.cliModel || ""
                  )}" placeholder="${escapeHtml(HARNESS_META[curCli]?.defaultCliModel || t("cliModelPlaceholder"))}" autocomplete="off" />
                </div>
                <div class="settings-row">
                  <div class="settings-row-title">${escapeHtml(t("lblCliBin"))}</div>
                  <input class="settings-input is-wide is-mono" data-action="setting-text" data-field="cliBinOverride" value="${escapeHtml(
                    s.cliBinOverride || ""
                  )}" placeholder="${escapeHtml(selectedCliMeta?.cliPath || t("cliBinPlaceholder"))}" autocomplete="off" />
                </div>`
              : `<div class="settings-row">
                  <div class="settings-row-title">${escapeHtml(t("lblApiProvider"))}</div>
                  <button type="button" class="ui-select-btn ${isApiSelectOpen ? "is-open" : ""}"
                    data-action="open-api-select" aria-haspopup="listbox">
                    <span class="ui-select-left">
                      <span class="ui-select-text">${escapeHtml(apiProviderLabel(curApi))}</span>
                    </span>
                    <span class="ui-select-caret">${chevronSvg("down")}</span>
                  </button>
                </div>
                <div class="settings-row">
                  <div class="settings-row-title">${escapeHtml(t("lblApiUrl"))}</div>
                  <input class="settings-input is-wide is-mono" data-action="setting-text" data-field="apiBaseUrl" value="${escapeHtml(
                    s.apiBaseUrl || ""
                  )}" placeholder="${escapeHtml(provMeta.defaultBase)}" autocomplete="off" />
                </div>
                <div class="settings-row">
                  <div class="settings-row-copy">
                    <div class="settings-row-title">${escapeHtml(t("lblApiModel"))}</div>
                    ${apiModelHint ? `<div class="settings-row-sub">${escapeHtml(apiModelHint)}</div>` : ""}
                  </div>
                  <input class="settings-input is-wide is-mono" data-action="setting-text" data-field="apiModel" value="${escapeHtml(
                    s.apiModel || ""
                  )}" placeholder="${escapeHtml(provMeta.defaultModel)}" autocomplete="off" />
                </div>
                <div class="settings-row">
                  <div class="settings-row-title">${escapeHtml(t("lblApiKey"))}</div>
                  <div class="key-input-group">
                    <input type="${
                      state.showApiKeyPlain ? "text" : "password"
                    }" class="settings-input is-mono" data-action="setting-text" data-field="apiKey" value="${escapeHtml(
                  s.apiKey || ""
                )}" placeholder="${escapeHtml(
                  provMeta.allowKeyless
                    ? t("apiKeyLocalOptPlaceholder")
                    : t("apiKeyEnvPlaceholder", { env: provMeta.envVars?.[0] || "" })
                )}" autocomplete="off" />
                    <button type="button" class="btn-outline is-key-toggle" data-action="toggle-apikey-vis">
                      ${escapeHtml(state.showApiKeyPlain ? t("keyHide") : t("keyShow"))}
                    </button>
                  </div>
                </div>`
          }
          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblBatchLimit"))}</div>
            <div class="stepper">
              <button type="button" data-action="step" data-field="syncBatchLimit" data-delta="-1">−</button>
              <span>${s.syncBatchLimit || DEFAULT_SETTINGS.syncBatchLimit}</span>
              <button type="button" data-action="step" data-field="syncBatchLimit" data-delta="1">+</button>
            </div>
          </div>

          <div class="proc-card-footer ${state.isTestingProcessor || state.isSyncing ? "is-busy" : ""}">
            ${state.isTestingProcessor || state.isSyncing ? `<div class="scan-line-bar" aria-hidden="true"></div>` : ""}
            ${consoleStatusHtml}
            <div class="proc-console-actions">
              <button type="button" class="btn-outline" data-action="test-processor" ${
                state.isTestingProcessor ? "disabled" : ""
              }>
                ${escapeHtml(state.isTestingProcessor ? t("testingProcBtn") : t("testProcBtn"))}
              </button>
              <button type="button" class="btn-sync ${
                state.unsummarized.length > 0 ? "has-pending" : ""
              } ${state.isSyncing ? "is-syncing" : ""}" data-action="sync-now">
                ${slicerTriggerIconSvg()}
                <span>${escapeHtml(state.isSyncing ? t("syncing") : t("syncNowBtn"))}</span>
                ${
                  state.unsummarized.length > 0
                    ? `<span class="sync-badge">${state.unsummarized.length}</span>`
                    : ""
                }
              </button>
            </div>
          </div>
        </div>
      </section>
    `;
  } else if (tab === "harnesses") {
    tabBody = `
      <section class="settings-section">
        <div class="settings-section-bar">
          <h2 class="settings-section-heading">${escapeHtml(t("harnessesTitle"))}</h2>
          <button type="button" class="btn-outline" data-action="rescan-harnesses" ${
            state.isRescanning ? "disabled" : ""
          }>
            ${syncRefreshIconSvg()}
            <span>${escapeHtml(state.isRescanning ? t("rescanningBtn") : t("rescanBtn"))}</span>
          </button>
        </div>

        <div class="settings-card ${state.isRescanning ? "is-scanning-card" : ""}">
          ${state.isRescanning ? `<div class="scan-line-bar" aria-hidden="true"></div>` : ""}
          ${HARNESS_ORDER.map((id) => {
            const meta = statusById.get(id) || {
              id,
              enabled: true,
              cliAvailable: false,
              cliPath: null,
              logRoot: "",
              logRootExists: false,
              sessionCount: 0,
            };
            const isEnabled = s.enabledHarnesses?.[id] !== false;
            const customPath = s.harnessPaths?.[id] || "";
            const isEditingPath = Boolean(customPath) || state.editingHarnessPaths.has(id);
            const displayPath = customPath || meta.logRoot || "~";

            return `
              <div class="settings-row-stack ${!isEnabled ? "is-disabled-source" : ""}">
                <div class="settings-row-top">
                  <div class="source-main-col">
                    <div class="line-left">
                      <span class="harness-source-icon">${harnessIconSvg(id, 18, { colored: true })}</span>
                      <span class="line-name">${escapeHtml(harnessFullLabel(id))}</span>
                      <code class="harness-cli-slug" data-harness="${id}">${escapeHtml(
                        HARNESS_META[id]?.cli || id
                      )}</code>
                      <span class="line-count">${meta.sessionCount || 0} ${escapeHtml(t("sessionPlural"))}</span>
                    </div>
                    <div class="harness-meta-row">
                      <span class="meta-outline-tag ${meta.cliAvailable ? "is-ready" : ""}" title="${escapeHtml(
              meta.cliPath || ""
            )}">
                        <span class="avail-dot ${meta.cliAvailable ? "is-ready" : "is-missing"}"></span>
                        <span>${escapeHtml(meta.cliAvailable ? t("cliReady") : t("cliMissing"))}</span>
                      </span>
                      <span class="source-path-text ${meta.logRootExists || customPath ? "" : "is-missing-dir"}">
                        ${escapeHtml(meta.logRootExists || customPath ? displayPath : t("dirMissing"))}
                      </span>
                      <button type="button" class="btn-inline-disclose" data-action="toggle-harness-path-edit" data-harness="${id}">
                        ${escapeHtml(isEditingPath ? t("collapsePathBtn") : t("editPathBtn"))}
                      </button>
                    </div>
                  </div>
                  <button type="button" class="switch ${isEnabled ? "is-on" : ""}" data-action="toggle-harness-enabled" data-harness="${id}" aria-pressed="${
              isEnabled ? "true" : "false"
            }">
                    <span class="switch-knob"></span>
                  </button>
                </div>
                ${
                  isEditingPath
                    ? `<div class="harness-path-editor">
                        <input class="settings-input is-wide is-mono"
                          data-action="harness-path"
                          data-harness="${id}"
                          value="${escapeHtml(customPath)}"
                          placeholder="${escapeHtml(meta.logRoot || t("pathPlaceholder"))}"
                          autocomplete="off" />
                        ${
                          customPath
                            ? `<button type="button" class="btn-quiet" data-action="reset-harness-path" data-harness="${id}">
                                ${escapeHtml(t("resetPathBtn"))}
                              </button>`
                            : ""
                        }
                      </div>`
                    : ""
                }
              </div>
            `;
          }).join("")}
        </div>
      </section>
    `;
  } else {
    tabBody = `
      <section class="settings-section">
        <h2 class="settings-section-heading">${escapeHtml(t("slicerHeading"))}</h2>

        <div class="slicer-diagram">
          <div class="slicer-top-labels">
            <span>${escapeHtml(t("slicerFirst", { n: s.openingSteps }))}</span>
            <span>${escapeHtml(t("slicerLast", { n: s.closingSteps }))}</span>
          </div>
          <div class="slicer-bar-wrap" id="slicer-bar-track">
            <div class="slicer-bar">
              <div class="slicer-seg is-open" style="flex-basis:${openBasis.toFixed(2)}%">
                <span class="slicer-dot"></span><span class="slicer-dot"></span>
              </div>
              <div class="slicer-seg is-mid" style="flex-basis:${midBasis.toFixed(2)}%">
                <span class="slicer-dot"></span><span class="slicer-dot"></span><span class="slicer-dot"></span><span class="slicer-dot"></span>
              </div>
              <div class="slicer-seg is-close" style="flex-basis:${closeBasis.toFixed(2)}%">
                <span class="slicer-dot"></span><span class="slicer-dot"></span><span class="slicer-dot"></span><span class="slicer-dot"></span>
              </div>
            </div>
            <button type="button" class="slicer-handle" data-slicer-drag="open" style="left:${openBasis.toFixed(
              2
            )}%" aria-label="${escapeHtml(t("slicerOpenLbl"))}" title="${escapeHtml(t("slicerOpenLbl"))}"></button>
            <button type="button" class="slicer-handle" data-slicer-drag="close" style="left:${(
              100 - closeBasis
            ).toFixed(2)}%" aria-label="${escapeHtml(t("slicerCloseLbl"))}" title="${escapeHtml(
      t("slicerCloseLbl")
    )}"></button>
          </div>
          <div class="slicer-mid-caption">${escapeHtml(t("slicerMidCaption"))}</div>
        </div>

        <div class="slicer-controls-row">
          <div class="slicer-control-block">
            <div class="slicer-legend-label">
              <span class="line-swatch" style="background:var(--slicer-open)"></span>
              <span>${escapeHtml(t("slicerOpenLbl"))}</span>
            </div>
            <div class="stepper">
              <button type="button" data-action="step" data-field="openingSteps" data-delta="-5">−</button>
              <span>${s.openingSteps}</span>
              <button type="button" data-action="step" data-field="openingSteps" data-delta="5">+</button>
            </div>
          </div>
          <div class="slicer-control-block">
            <div class="slicer-legend-label">
              <span class="line-swatch" style="background:var(--slicer-close)"></span>
              <span>${escapeHtml(t("slicerCloseLbl"))}</span>
            </div>
            <div class="stepper">
              <button type="button" data-action="step" data-field="closingSteps" data-delta="-5">−</button>
              <span>${s.closingSteps}</span>
              <button type="button" data-action="step" data-field="closingSteps" data-delta="5">+</button>
            </div>
          </div>
        </div>

        <div class="settings-card">
          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblMaxUserChars"))}</div>
            <label class="char-pill">
              <input type="number" min="0" step="100" value="${s.longestUserChars}" placeholder="0" data-action="char-limit" data-field="longestUserChars" />
              <span style="color:var(--ink-secondary)">${escapeHtml(t("charsUnit"))}</span>
            </label>
          </div>
          <div class="settings-row">
            <div class="settings-row-title">${escapeHtml(t("lblMaxAgentChars"))}</div>
            <label class="char-pill">
              <input type="number" min="0" step="100" value="${s.longestAgentChars}" placeholder="0" data-action="char-limit" data-field="longestAgentChars" />
              <span style="color:var(--ink-secondary)">${escapeHtml(t("charsUnit"))}</span>
            </label>
          </div>
        </div>
      </section>

      <section class="settings-section">
        <h2 class="settings-section-heading">${escapeHtml(t("pinnedLinesTitle"))}</h2>
        <div class="settings-card">
          ${
            pinnedLines.length > 0
              ? pinnedLines
                  .map(
                    (l) => `<div class="settings-row-block is-line-config-block">
                      <div class="settings-line-head">
                        <div class="line-left">
                          <button type="button" class="swatch-picker-btn" data-action="open-line-color-menu" data-line-id="${
                            l.id
                          }" title="${escapeHtml(t("pickLineColor"))}">
                            <span class="line-swatch" style="background:${resolveLineColor(l.color)}"></span>
                          </button>
                          <span class="line-name">${escapeHtml(l.name)}</span>
                          <button type="button" class="btn-icon-inline" data-action="rename-line" data-line-id="${
                            l.id
                          }" title="${escapeHtml(t("renameLine"))}">
                            <svg width="12" height="12" viewBox="0 0 14 14" fill="none"><path d="M9.7 2.3l2 2-7.1 7.1-2.5.5.5-2.5 7.1-7.1z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
                          </button>
                        </div>
                        <div class="line-right">
                          <span class="line-count">${l.sessions.length} ${escapeHtml(t("sessionPlural"))}</span>
                          ${
                            state.lines.length > 1
                              ? `<button type="button" class="btn-quiet" data-action="open-merge-line-picker" data-line-id="${
                                  l.id
                                }" title="${escapeHtml(t("mergeLine"))}">${escapeHtml(t("mergeBtn"))}</button>`
                              : ""
                          }
                          <button type="button" class="btn-quiet" data-action="unpin-line" data-line-id="${
                            l.id
                          }" title="${escapeHtml(t("unpinBtn"))}">${escapeHtml(t("unpinBtn"))}</button>
                        </div>
                      </div>
                      <input class="line-scope-input"
                        type="text"
                        data-action="edit-line-desc"
                        data-line-id="${l.id}"
                        value="${escapeHtml(l.description || "")}"
                        placeholder="${escapeHtml(t("lineScopePlaceholder"))}"
                        autocomplete="off" />
                    </div>`
                  )
                  .join("")
              : `<div class="settings-empty-row">${escapeHtml(t("emptyPinnedHint"))}</div>`
          }
          <form class="settings-row-block is-add-line-block" id="add-line-form">
            <div class="add-line-main-row">
              <input class="settings-line-input" id="new-line-input" placeholder="${escapeHtml(
                t("linePlaceholder")
              )}" autocomplete="off" />
              <button type="submit" class="btn-outline">${escapeHtml(t("addLineBtn"))}</button>
            </div>
            <input class="line-scope-input is-add-scope" id="new-line-desc-input" placeholder="${escapeHtml(
              t("lineScopeAddPlaceholder")
            )}" autocomplete="off" />
          </form>
        </div>
      </section>

      <section class="settings-section">
        <div class="settings-section-bar">
          <h2 class="settings-section-heading">${escapeHtml(t("discoveredTitle"))}</h2>
          <button type="button" class="btn-outline" data-action="regroup-lines" ${
            state.isRegrouping ? "disabled" : ""
          }>
            ${slicerTriggerIconSvg()}
            <span>${escapeHtml(state.isRegrouping ? t("regroupingBtn") : t("regroupBtn"))}</span>
          </button>
        </div>
        <div class="settings-card ${state.isRegrouping ? "is-scanning-card" : ""}">
          ${state.isRegrouping ? `<div class="scan-line-bar" aria-hidden="true"></div>` : ""}
          ${
            discoveredLines.length > 0
              ? discoveredLines
                  .map(
                    (l) => `<div class="settings-row is-compact-line-row">
                      <div class="line-left">
                        <span class="icon-slot"><span class="line-swatch" style="background:${resolveLineColor(
                          l.color
                        )}"></span></span>
                        <span class="line-name">${escapeHtml(l.name)}</span>
                      </div>
                      <div class="line-right">
                        <span class="line-count">${l.sessions.length} ${escapeHtml(t("sessionPlural"))}</span>
                        ${
                          state.lines.length > 1
                            ? `<button type="button" class="btn-quiet" data-action="open-merge-line-picker" data-line-id="${
                                l.id
                              }" title="${escapeHtml(t("mergeLine"))}">${escapeHtml(t("mergeBtn"))}</button>`
                            : ""
                        }
                        <button type="button" class="btn-outline is-small-btn" data-action="keep-line" data-line-id="${
                          l.id
                        }">${escapeHtml(t("keepBtn"))}</button>
                      </div>
                    </div>`
                  )
                  .join("")
              : `<div class="settings-empty-row">${escapeHtml(t("emptyDiscoveredHint"))}</div>`
          }
        </div>
      </section>
    `;
  }

  const isThemeOpen = state.popover?.type === "theme-menu";
  const curTheme = s.theme || DEFAULT_SETTINGS.theme;

  return `
    <div class="settings-page ${motionCls}">
      <header class="settings-topbar">
        <div class="settings-topbar-left">
          <button type="button" class="settings-back-btn" data-action="back-to-main" title="Esc">
            ${backArrowSvg()}
            <span>${escapeHtml(t("navPlane"))}</span>
          </button>
          <span class="topbar-sep" aria-hidden="true"></span>
          <h1 class="settings-page-title">${escapeHtml(t("navSettings"))}</h1>
        </div>
        <div class="topbar-actions">
          <span class="settings-save-status ${state.settingsSavedPulse ? "is-pulsing" : ""}">
            <span class="save-dot" aria-hidden="true"></span>
            <span class="save-label">${escapeHtml(
              state.settingsSavedPulse ? t("savedJustNow") : t("autoSaveReady")
            )}</span>
          </span>
          <button type="button" class="btn-outline btn-icon ${isThemeOpen ? "is-open" : ""}"
            data-action="open-theme-menu"
            title="${escapeHtml(t("lblTheme"))}"
            aria-label="${escapeHtml(t("lblTheme"))}">
            ${themeIconSvg(curTheme)}
          </button>
        </div>
      </header>

      <div class="settings-shell">
        <nav class="settings-nav" role="tablist" aria-label="${escapeHtml(t("navSettings"))}">
          ${tabs
            .map(
              (tb) => `<button type="button" class="settings-nav-item ${tab === tb.id ? "is-active" : ""}"
                role="tab" aria-selected="${tab === tb.id ? "true" : "false"}"
                data-action="set-settings-tab" data-tab="${tb.id}">
                ${settingsNavIconSvg(tb.id)}
                <span>${escapeHtml(tb.label)}</span>
              </button>`
            )
            .join("")}
        </nav>

        <div class="settings-content-stage">
          <main class="settings-content">
            ${tabBody}
          </main>
          <button type="button" class="scroll-more-cue" data-action="scroll-settings-more" aria-label="Scroll down" tabindex="-1">
            <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
              <path d="M3.6 5.4L7 8.8l3.4-3.4" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </button>
        </div>
      </div>
    </div>
  `;
}

function bindAddLineForm() {
  const addForm = document.getElementById("add-line-form");
  if (addForm) {
    addForm.addEventListener("submit", (e) => {
      e.preventDefault();
      const inp = document.getElementById("new-line-input");
      const descInp = document.getElementById("new-line-desc-input");
      const name = inp?.value.trim() || "";
      const description = descInp?.value.trim() || "";
      if (!name) return;
      markSettingsSaved();
      if (state.dataSource === "api") {
        inp.value = "";
        if (descInp) descInp.value = "";
        void apiApply("/api/lines", "POST", { name, description });
        return;
      }
      state.lines.splice(Math.max(0, state.lines.length - 1), 0, {
        id: `line-${Date.now()}`,
        name,
        description,
        color: PALETTE[state.lines.length % PALETTE.length],
        pinned: true,
        priority: 0,
        status: "active",
        sessions: [],
      });
      render();
    });
  }

  for (const descInput of document.querySelectorAll(".line-scope-input[data-action='edit-line-desc']")) {
    const commit = () => {
      const lid = descInput.dataset.lineId;
      const line = state.lines.find((l) => l.id === lid);
      if (!line) return;
      const nextDesc = descInput.value.trim();
      if ((line.description || "") === nextDesc) return;
      line.description = nextDesc;
      markSettingsSaved();
      if (state.dataSource === "api") {
        void apiApply(`/api/lines/${encodeURIComponent(line.id)}`, "PATCH", { description: nextDesc });
      }
    };
    descInput.addEventListener("blur", commit);
    descInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        descInput.blur();
      }
    });
  }
}

// Popovers
function renderPopoverLayer() {
  const layer = document.getElementById("popover-layer");
  if (!state.popover) {
    layer.hidden = true;
    layer.innerHTML = "";
    return;
  }
  layer.hidden = false;
  const p = state.popover;
  const isZh = lang() === "zh";

  if (p.type === "line-menu") {
    const line = state.lines.find((l) => l.id === p.lineId);
    if (!line) return;
    const priorities = isZh
      ? [
          { val: 1, label: "紧急", key: "1" },
          { val: 2, label: "高", key: "2" },
          { val: 3, label: "中", key: "3" },
          { val: 4, label: "低", key: "4" },
          { val: 0, label: "无优先级", key: "0" },
        ]
      : [
          { val: 1, label: "Urgent", key: "1" },
          { val: 2, label: "High", key: "2" },
          { val: 3, label: "Medium", key: "3" },
          { val: 4, label: "Low", key: "4" },
          { val: 0, label: "No priority", key: "0" },
        ];
    const statuses = isZh
      ? [
          { val: "active", label: "进行中" },
          { val: "paused", label: "暂停" },
          { val: "done", label: "已完成" },
        ]
      : [
          { val: "active", label: "Active" },
          { val: "paused", label: "Paused" },
          { val: "done", label: "Done" },
        ];
    const menuW = Math.min(214, window.innerWidth - 20);
    const top = Math.max(10, Math.min(window.innerHeight - 332, p.top));
    const left = Math.max(10, Math.min(window.innerWidth - menuW - 10, p.left));

    layer.innerHTML = `
      <div class="popover-menu" style="top:${top}px;left:${left}px">
        <div class="popover-section-label">${escapeHtml(t("priority"))}</div>
        ${priorities
          .map(
            (pr) => `<button class="popover-item ${line.priority === pr.val ? "is-selected" : ""}"
              data-action="set-priority" data-line-id="${line.id}" data-priority="${pr.val}">
              <span class="popover-item-left">${priorityIconSvg(pr.val)}<span>${escapeHtml(pr.label)}</span></span>
              <span class="popover-item-right">
                ${line.priority === pr.val ? checkMarkSvg() : ""}
                <span class="kbd-hint">${pr.key}</span>
              </span>
            </button>`
          )
          .join("")}
        <div class="popover-divider"></div>
        <div class="popover-section-label">${escapeHtml(t("status"))}</div>
        ${statuses
          .map(
            (st) => `<button class="popover-item ${line.status === st.val ? "is-selected" : ""}"
              data-action="set-status" data-line-id="${line.id}" data-status="${st.val}">
              <span class="popover-item-left">${statusIconSvg(st.val)}<span>${escapeHtml(st.label)}</span></span>
              <span class="popover-item-right">
                ${line.status === st.val ? checkMarkSvg() : ""}
              </span>
            </button>`
          )
          .join("")}
        <div class="popover-divider"></div>
        <button class="popover-item" data-action="rename-line" data-line-id="${line.id}">
          <span class="popover-item-left">
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M9.7 2.3l2 2-7.1 7.1-2.5.5.5-2.5 7.1-7.1z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
            <span>${escapeHtml(t("renameLine"))}</span>
          </span>
          <span class="kbd-hint">R</span>
        </button>
        ${
          !line.isSystemBucket && state.lines.length > 1
            ? `<button class="popover-item" data-action="open-merge-line-picker" data-line-id="${line.id}">
                <span class="popover-item-left">
                  ${mergeIconSvg()}
                  <span>${escapeHtml(t("mergeLine"))}</span>
                </span>
              </button>`
            : ""
        }
      </div>
    `;
    return;
  }

  if (p.type === "merge-line-picker") {
    const sourceLine = state.lines.find((l) => l.id === p.lineId);
    if (!sourceLine) return;
    const targets = state.lines.filter((l) => l.id !== sourceLine.id);
    const menuW = Math.min(256, window.innerWidth - 20);
    const estH = Math.min(340, 88 + targets.length * 32);
    const top = Math.max(10, Math.min(window.innerHeight - estH - 10, p.top));
    const left = Math.max(10, Math.min(window.innerWidth - menuW - 10, p.left));
    layer.innerHTML = `
      <div class="popover-menu is-merge-picker" style="top:${top}px;left:${left}px">
        <div class="popover-section-label">${escapeHtml(t("mergeIntoTitle", { name: sourceLine.name }))}</div>
        <div class="popover-merge-hint">${escapeHtml(t("mergeHint", { name: sourceLine.name }))}</div>
        <div class="picker-list is-merge-list">
          ${targets
            .map(
              (tgt) => `<button class="popover-item" data-action="confirm-merge-line" data-source-id="${
                sourceLine.id
              }" data-target-id="${tgt.id}">
                <span class="popover-item-left">
                  <span class="line-swatch" style="background:${resolveLineColor(tgt.color)}"></span>
                  <span>${escapeHtml(tgt.name)}</span>
                </span>
                <span class="popover-item-right">
                  <span class="line-count">${tgt.sessions.length}</span>
                </span>
              </button>`
            )
            .join("")}
        </div>
      </div>
    `;
    return;
  }

  if (p.type === "upstream-picker") {
    const q = state.pickerQuery.trim().toLowerCase();
    const curLine = p.lineId ? state.lines.find((l) => l.id === p.lineId) : null;
    const sameLineCandidates = (curLine ? curLine.sessions : []).filter(
      (s) => s.id !== p.sessionId && !s.hidden && (!q || s.title.toLowerCase().includes(q))
    );
    const otherCandidates = [];
    for (const u of state.unsummarized) {
      if (u.id === p.sessionId) continue;
      if (!q || u.firstPrompt.toLowerCase().includes(q)) {
        otherCandidates.push({ id: u.id, title: u.firstPrompt, recency: u.recency, color: "#7E828A", hollow: false });
      }
    }
    for (const sh of state.shortSessions.items.slice(0, 2)) {
      if (!q || sh.prompt.toLowerCase().includes(q)) {
        otherCandidates.push({ id: sh.id, title: sh.prompt, recency: sh.recency, color: "#8A8F98", hollow: true });
      }
    }
    for (const l of state.lines) {
      if (l.id === p.lineId) continue;
      for (const s of l.sessions) {
        if (!s.hidden && (!q || s.title.toLowerCase().includes(q))) {
          otherCandidates.push({
            id: s.id,
            title: s.title,
            recency: s.recency,
            color: resolveLineColor(l.color),
            hollow: false,
          });
        }
      }
    }
    const pickerW = Math.min(318, window.innerWidth - 20);
    const top = Math.max(10, Math.min(window.innerHeight - 320, p.top - 290));
    const left = Math.max(10, Math.min(window.innerWidth - pickerW - 10, p.left));

    layer.innerHTML = `
      <div class="session-picker-menu" style="top:${top}px;left:${left}px">
        <input id="picker-search-input" class="picker-search-input" placeholder="${escapeHtml(
          t("searchSessions")
        )}" value="${escapeHtml(state.pickerQuery)}" autocomplete="off" />
        <div class="picker-list">
          ${
            sameLineCandidates.length > 0
              ? `<div class="popover-section-label">${escapeHtml(curLine.name)}</div>
                 ${sameLineCandidates
                   .map(
                     (s) => `<button class="popover-item" data-action="pick-upstream" data-session-id="${
                       p.sessionId
                     }" data-upstream-id="${s.id}">
                      <span class="popover-item-left">
                        <span class="line-swatch" style="background:${resolveLineColor(curLine.color)}"></span>
                        <span class="popover-item-title">${escapeHtml(s.title)}</span>
                      </span>
                      <span class="session-age">${escapeHtml(localizeRecency(s.recency).replace(" ago", ""))}</span>
                    </button>`
                   )
                   .join("")}`
              : ""
          }
          ${
            otherCandidates.length > 0
              ? `<div class="popover-section-label">${escapeHtml(t("otherLines"))}</div>
                 ${otherCandidates
                   .map(
                     (item) => `<button class="popover-item" data-action="pick-upstream" data-session-id="${
                       p.sessionId
                     }" data-upstream-id="${item.id}">
                      <span class="popover-item-left">
                        ${
                          item.hollow
                            ? `<span class="line-swatch is-hollow"></span>`
                            : `<span class="line-swatch" style="background:${item.color}"></span>`
                        }
                        <span class="popover-item-title">${escapeHtml(item.title)}</span>
                      </span>
                      <span class="session-age">${escapeHtml(localizeRecency(item.recency).replace(" ago", ""))}</span>
                    </button>`
                   )
                   .join("")}`
              : ""
          }
        </div>
      </div>
    `;
    const inp = document.getElementById("picker-search-input");
    if (inp) {
      inp.focus();
      inp.setSelectionRange(inp.value.length, inp.value.length);
      inp.addEventListener("input", () => {
        state.pickerQuery = inp.value;
        renderPopoverLayer();
      });
    }
    return;
  }

  if (p.type === "range-menu") {
    const opts = [
      { label: t("range7"), short: "7d" },
      { label: t("range14"), short: "14d" },
      { label: t("range30"), short: "30d" },
    ];
    const activeShort = localizedRangeShort();
    const hasCal = Boolean(
      state.settings.showContributionGraph &&
        Array.isArray(state.contributions) &&
        state.contributions.length > 0
    );
    const popW = Math.min(hasCal ? 358 : 224, window.innerWidth - 20);
    const estH = hasCal ? 236 : 64;
    const top = Math.max(10, Math.min(window.innerHeight - estH - 10, p.top));
    const anchorRight = typeof p.right === "number" ? p.right : (p.left || 200) + 120;
    const left = Math.max(10, Math.min(window.innerWidth - popW - 10, anchorRight - popW));

    layer.innerHTML = `
      <div class="horizon-popover ${hasCal ? "" : "is-compact"}" style="top:${top}px;left:${left}px">
        <div class="horizon-top ${hasCal ? "has-cal" : ""}">
          <span class="prefs-inline-lbl">${escapeHtml(isZh ? "时间窗口" : "Window")}</span>
          <div class="mini-seg" role="tablist">
            ${opts
              .map(
                (opt) => `<button class="mini-seg-btn ${activeShort === opt.short ? "is-active" : ""}"
                  data-action="select-range" data-range="${escapeHtml(opt.label)}" data-short="${opt.short}">
                  ${escapeHtml(opt.short)}
                </button>`
              )
              .join("")}
          </div>
        </div>
        ${hasCal ? renderContributionPopoverBody() : ""}
      </div>
    `;
    return;
  }

  if (p.type === "theme-menu") {
    const menuW = Math.min(164, window.innerWidth - 20);
    const top = Math.max(10, Math.min(window.innerHeight - 140, p.top));
    const anchorRight = typeof p.right === "number" ? p.right : window.innerWidth - 24;
    const left = Math.max(10, Math.min(window.innerWidth - menuW - 10, anchorRight - menuW));
    const curTheme = state.settings.theme || DEFAULT_SETTINGS.theme;
    const themeOptions = [
      { id: "system", label: t("themeSystem") },
      { id: "light", label: t("themeLight") },
      { id: "dark", label: t("themeDark") },
    ];

    layer.innerHTML = `
      <div class="popover-menu is-theme-menu" style="top:${top}px;left:${left}px" role="menu" aria-label="${escapeHtml(
        t("lblTheme")
      )}">
        ${themeOptions
          .map(
            (opt) => `<button class="popover-item ${curTheme === opt.id ? "is-selected" : ""}"
              data-action="set-theme-quick" data-theme="${opt.id}" role="menuitemradio" aria-checked="${
              curTheme === opt.id ? "true" : "false"
            }">
              <span class="popover-item-left">
                ${themeIconSvg(opt.id)}
                <span>${escapeHtml(opt.label)}</span>
              </span>
              ${curTheme === opt.id ? checkMarkSvg() : ""}
            </button>`
          )
          .join("")}
      </div>
    `;
    return;
  }

  if (p.type === "line-color-menu") {
    const line = state.lines.find((l) => l.id === p.lineId);
    if (!line) return;
    const popW = 196;
    const top = Math.max(10, Math.min(window.innerHeight - 90, p.top));
    const left = Math.max(10, Math.min(window.innerWidth - popW - 10, p.left));

    layer.innerHTML = `
      <div class="color-swatch-popover" style="top:${top}px;left:${left}px" role="menu" aria-label="${escapeHtml(
        t("pickLineColor")
      )}">
        <div class="popover-section-label">${escapeHtml(t("pickLineColor"))}</div>
        <div class="color-swatch-grid">
          ${PALETTE.map((hex) => {
            const isSelected = (line.color || "").toLowerCase() === hex.toLowerCase();
            return `<button type="button" class="color-swatch-choice ${isSelected ? "is-selected" : ""}"
              data-action="set-line-color" data-line-id="${line.id}" data-color="${hex}"
              style="--swatch-color:${resolveLineColor(hex)}" aria-label="${hex}">
              <span class="color-swatch-dot"></span>
            </button>`;
          }).join("")}
        </div>
      </div>
    `;
    return;
  }

  if (p.type === "reassign-line") {
    const found = findSessionEverywhere(p.sessionId);
    if (!found) return;
    const curLineId = found.line?.id || null;
    const menuW = Math.min(214, window.innerWidth - 20);
    const top = Math.max(10, Math.min(window.innerHeight - 240, p.top));
    const left = Math.max(10, Math.min(window.innerWidth - menuW - 10, p.left));
    layer.innerHTML = `
      <div class="popover-menu" style="top:${top}px;left:${left}px">
        <div class="popover-section-label">${escapeHtml(t("moveSessionTitle"))}</div>
        ${state.lines
          .map(
            (l) => `<button class="popover-item ${l.id === curLineId ? "is-selected" : ""}"
              data-action="move-session-to-line" data-session-id="${p.sessionId}" data-target-line-id="${l.id}">
              <span class="popover-item-left">
                <span class="line-swatch" style="background:${resolveLineColor(l.color)}"></span>
                <span>${escapeHtml(l.name)}</span>
              </span>
              ${l.id === curLineId ? checkMarkSvg() : ""}
            </button>`
          )
          .join("")}
      </div>
    `;
    return;
  }

  if (p.type === "cli-select") {
    const statusById = new Map((state.harnesses || []).map((h) => [h.id, h]));
    const curCli = (state.settings.cliHarness || "").trim();
    const menuW = Math.min(292, window.innerWidth - 20);
    const estH = 228;
    const top = Math.max(10, Math.min(window.innerHeight - estH - 10, p.top));
    const anchorRight = typeof p.right === "number" ? p.right : window.innerWidth - 24;
    const left = Math.max(10, Math.min(window.innerWidth - menuW - 10, anchorRight - menuW));

    layer.innerHTML = `
      <div class="popover-menu is-select-menu" style="width:${menuW}px;top:${top}px;left:${left}px" role="listbox" aria-label="${escapeHtml(
        t("lblCliHarness")
      )}">
        <div class="picker-list">
          ${CLI_ENGINE_ORDER.map((id) => {
            const ready = Boolean(statusById.get(id)?.cliAvailable);
            const selected = curCli === id;
            return `<button type="button" class="popover-item ${selected ? "is-selected" : ""}"
              data-action="select-cli-engine" data-cli="${id}" role="option" aria-selected="${
              selected ? "true" : "false"
            }">
              <span class="popover-item-left">
                <span class="avail-dot ${ready ? "is-ready" : "is-missing"}" aria-hidden="true"></span>
                ${harnessIconSvg(id, 14, { colored: true })}
                <span class="popover-item-title">${escapeHtml(harnessFullLabel(id))}</span>
              </span>
              <span class="popover-item-right">
                <span class="popover-item-meta">${escapeHtml(HARNESS_META[id]?.cli || id)}</span>
                ${selected ? checkMarkSvg() : ""}
              </span>
            </button>`;
          }).join("")}
          <div class="popover-divider"></div>
          <button type="button" class="popover-item ${curCli === "auto" ? "is-selected" : ""}"
            data-action="select-cli-engine" data-cli="auto" role="option" aria-selected="${
            curCli === "auto" ? "true" : "false"
          }">
            <span class="popover-item-left">
              <span class="popover-item-title">${escapeHtml(t("cliAutoOption"))}</span>
            </span>
            <span class="popover-item-right">
              <span class="popover-item-meta">${escapeHtml(t("cliAutoOrderMeta"))}</span>
              ${curCli === "auto" ? checkMarkSvg() : ""}
            </span>
          </button>
        </div>
      </div>
    `;
    syncScrollAffordances();
    return;
  }

  if (p.type === "api-select") {
    const curApi =
      (state.settings.apiProvider || DEFAULT_SETTINGS.apiProvider).trim() ||
      DEFAULT_SETTINGS.apiProvider;
    const menuW = Math.min(264, window.innerWidth - 20);
    const estH = 296;
    const top = Math.max(10, Math.min(window.innerHeight - estH - 10, p.top));
    const anchorRight = typeof p.right === "number" ? p.right : window.innerWidth - 24;
    const left = Math.max(10, Math.min(window.innerWidth - menuW - 10, anchorRight - menuW));

    layer.innerHTML = `
      <div class="popover-menu is-select-menu" style="width:${menuW}px;top:${top}px;left:${left}px" role="listbox" aria-label="${escapeHtml(
        t("lblApiProvider")
      )}">
        <div class="picker-list">
          ${API_PROVIDERS.map((ap) => {
            const selected = curApi === ap.id;
            const label = isZh ? ap.labelZh : ap.labelEn;
            return `<button type="button" class="popover-item ${selected ? "is-selected" : ""}"
              data-action="select-api-provider" data-provider="${ap.id}" role="option" aria-selected="${
              selected ? "true" : "false"
            }">
              <span class="popover-item-left">
                <span class="popover-item-title">${escapeHtml(label)}</span>
              </span>
              <span class="popover-item-right">
                ${selected ? checkMarkSvg() : ""}
              </span>
            </button>`;
          }).join("")}
        </div>
      </div>
    `;
    syncScrollAffordances();
    return;
  }

  if (p.type === "lang-select") {
    const curLang = lang();
    const menuW = Math.min(184, window.innerWidth - 20);
    const estH = 84;
    const top = Math.max(10, Math.min(window.innerHeight - estH - 10, p.top));
    const anchorRight = typeof p.right === "number" ? p.right : window.innerWidth - 24;
    const left = Math.max(10, Math.min(window.innerWidth - menuW - 10, anchorRight - menuW));

    layer.innerHTML = `
      <div class="popover-menu is-select-menu" style="width:${menuW}px;top:${top}px;left:${left}px" role="listbox" aria-label="${escapeHtml(
        t("lblLang")
      )}">
        <div class="picker-list">
          ${LANG_OPTIONS.map((opt) => {
            const selected = curLang === opt.id;
            return `<button type="button" class="popover-item ${selected ? "is-selected" : ""}"
              data-action="select-lang" data-lang="${opt.id}" role="option" aria-selected="${
              selected ? "true" : "false"
            }">
              <span class="popover-item-left">
                <span class="popover-item-title">${escapeHtml(opt.label)}</span>
              </span>
              <span class="popover-item-right">
                ${selected ? checkMarkSvg() : ""}
              </span>
            </button>`;
          }).join("")}
        </div>
      </div>
    `;
    return;
  }

  if (p.type === "short-threshold-select") {
    const curN = Number(state.settings.fewestUserInputs) || DEFAULT_SETTINGS.fewestUserInputs;
    const menuW = Math.min(188, window.innerWidth - 20);
    const estH = 248;
    const top = Math.max(10, Math.min(window.innerHeight - estH - 10, p.top));
    const anchorRight = typeof p.right === "number" ? p.right : window.innerWidth - 24;
    const left = Math.max(10, Math.min(window.innerWidth - menuW - 10, anchorRight - menuW));
    const values = SHORT_THRESHOLD_OPTIONS.includes(curN)
      ? SHORT_THRESHOLD_OPTIONS
      : [...SHORT_THRESHOLD_OPTIONS, curN].sort((a, b) => a - b);

    layer.innerHTML = `
      <div class="popover-menu is-select-menu" style="width:${menuW}px;top:${top}px;left:${left}px" role="listbox" aria-label="${escapeHtml(
        t("shortThresholdTitle")
      )}">
        <div class="picker-list">
          <div class="popover-section-label">${escapeHtml(t("shortThresholdTitle"))}</div>
          ${values
            .map((n) => {
              const selected = curN === n;
              return `<button type="button" class="popover-item ${selected ? "is-selected" : ""}"
                data-action="select-short-threshold" data-threshold="${n}" role="option" aria-selected="${
                selected ? "true" : "false"
              }">
                <span class="popover-item-left">
                  <span class="popover-item-title">${escapeHtml(t("shortThresholdBtn", { n }))}</span>
                </span>
                <span class="popover-item-right">
                  ${selected ? checkMarkSvg() : ""}
                </span>
              </button>`;
            })
            .join("")}
        </div>
      </div>
    `;
    syncScrollAffordances();
    return;
  }

  if (p.type === "sync-setup-prompt") {
    const popW = Math.min(296, window.innerWidth - 20);
    const estH = 128;
    const top = Math.max(10, Math.min(window.innerHeight - estH - 10, p.top));
    const anchorRight = typeof p.right === "number" ? p.right : window.innerWidth - 24;
    const left = Math.max(10, Math.min(window.innerWidth - popW - 10, anchorRight - popW));

    layer.innerHTML = `
      <div class="setup-prompt-popover" style="top:${top}px;left:${left}px" role="dialog" aria-label="${escapeHtml(
        t("setupPromptTitle")
      )}">
        <div class="setup-prompt-title">${escapeHtml(t("setupPromptTitle"))}</div>
        <div class="setup-prompt-body">${escapeHtml(t("setupPromptBody"))}</div>
        <div class="setup-prompt-actions">
          <button type="button" class="btn-quiet" data-action="close-popover">${escapeHtml(t("cancelBtn"))}</button>
          <button type="button" class="btn-sync has-pending" data-action="goto-processor-setup">
            <span>${escapeHtml(t("setupPromptGo"))}</span>
          </button>
        </div>
      </div>
    `;
  }
}

function rangeDays() {
  if (state.mode === "month" || state.rangeShort === "30d") return 30;
  if (state.rangeShort === "14d") return 14;
  return 7;
}

function apiQuerySuffix() {
  const qp = new URLSearchParams({ days: String(rangeDays()) });
  if (state.anchorDate) qp.set("date", state.anchorDate);
  return qp.toString();
}

function shouldForceMock(searchParams) {
  return (
    searchParams.get("mock") === "1" ||
    searchParams.get("recap") === "1" ||
    searchParams.get("dep") === "1" ||
    searchParams.get("menu") === "priority" ||
    searchParams.get("expand") === "1" ||
    searchParams.has("hover")
  );
}

const initialUrlParams = new URLSearchParams(window.location.search);
let urlLangOverride = initialUrlParams.get("lang");
let urlThemeOverride = initialUrlParams.get("theme");
let urlLensOverride = initialUrlParams.get("lens");
let didInitialApiExpand = false;

function normalizePlaneSettings(rawSettings) {
  const resolvedApiProvider =
    typeof rawSettings.apiProvider === "string"
      ? rawSettings.apiProvider
      : DEFAULT_SETTINGS.apiProvider;
  const resolvedApiModel =
    typeof rawSettings.apiModel === "string"
      ? rawSettings.apiModel
      : defaultApiModelForProvider(resolvedApiProvider);
  const normalizedSettings = {
    ...DEFAULT_SETTINGS,
    ...rawSettings,
    apiProvider: resolvedApiProvider,
    apiModel: resolvedApiModel,
    enabledHarnesses: {
      ...DEFAULT_SETTINGS.enabledHarnesses,
      ...(rawSettings.enabledHarnesses || {}),
    },
    harnessPaths: {
      ...DEFAULT_SETTINGS.harnessPaths,
      ...(rawSettings.harnessPaths || {}),
    },
  };
  if (!normalizedSettings.processorConfigured && normalizedSettings.cliHarness === "auto") {
    normalizedSettings.cliHarness = "";
  }
  return normalizedSettings;
}

function applyPlaneResponse(data) {
  if (!data || !Array.isArray(data.days)) return;
  const wasMock = state.dataSource === "mock";
  state.dataSource = "api";
  state.days = data.days;
  const lastDay = data.days[data.days.length - 1];
  if ("anchorDate" in data) {
    state.anchorDate = lastDay?.isToday ? null : data.anchorDate || null;
  }
  state.nowHour = typeof data.nowHour === "number" ? data.nowHour : null;
  state.tzLabel = data.tzLabel || state.tzLabel;
  state.archivedCount = typeof data.archivedCount === "number" ? data.archivedCount : 0;
  if (data.settings) {
    state.settings = normalizePlaneSettings(data.settings);
  }
  if (urlLangOverride === "zh" || urlLangOverride === "en") {
    state.settings.language = urlLangOverride;
  }
  if (urlThemeOverride === "light" || urlThemeOverride === "dark" || urlThemeOverride === "system") {
    state.settings.theme = urlThemeOverride;
  }
  if (urlLensOverride === "time" || urlLensOverride === "tokens" || urlLensOverride === "cost") {
    state.settings.metricLens = urlLensOverride;
  }
  applyThemeAndLang();

  state.lines = Array.isArray(data.lines) ? data.lines : [];
  state.unsummarized = Array.isArray(data.unsummarized) ? data.unsummarized : [];
  state.shortSessions = data.shortSessions || { totalCount: 0, items: [], weekMinutes: 0 };
  if (Array.isArray(data.contributions)) state.contributions = data.contributions;
  if (Array.isArray(data.harnesses)) state.harnesses = data.harnesses;
  if ("lastSync" in data) state.lastSync = data.lastSync || null;

  if (wasMock) {
    state.expandedListLines.delete("sqlite-sync");
    state.expandedTimelineLines.delete("sqlite-sync");
    state.expandedSessionIds.clear();
  }

  if (state.selectedDayIdx >= state.days.length) {
    state.selectedDayIdx = Math.max(0, state.days.length - 1);
  }

  if (!didInitialApiExpand) {
    didInitialApiExpand = true;
    const anyLineHasSessions = state.lines.some((l) => l.sessions.length > 0);
    if (anyLineHasSessions) {
      const firstActive = state.lines.find((l) => l.sessions.length > 0);
      if (firstActive) state.expandedListLines.add(firstActive.id);
    } else if (state.unsummarized.length > 0) {
      state.expandedListLines.add("__unsummarized__");
    }
  }
}

let lastApiRefreshMs = 0;
let toastEl = null;
let toastTimer = 0;

function showToast(message) {
  if (!message) return;
  if (!toastEl) {
    toastEl = document.createElement("div");
    toastEl.id = "planar-toast";
    toastEl.className = "app-toast";
    document.body.appendChild(toastEl);
  }
  toastEl.textContent = message;
  toastEl.classList.add("is-visible");
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toastEl?.classList.remove("is-visible");
  }, 2600);
}

async function loadPlaneFromApi(rescan = false) {
  try {
    const res = await fetch(`/api/plane?${apiQuerySuffix()}&rescan=${rescan ? "1" : "0"}`);
    if (!res.ok) return false;
    const data = await res.json();
    lastApiRefreshMs = Date.now();
    applyPlaneResponse(data);
    render();
    return true;
  } catch {
    return false;
  }
}

async function apiApply(path, method = "POST", body = undefined) {
  try {
    const sep = path.includes("?") ? "&" : "?";
    const res = await fetch(`${path}${sep}${apiQuerySuffix()}`, {
      method,
      headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    if (!res.ok) return null;
    const data = await res.json();
    lastApiRefreshMs = Date.now();
    applyPlaneResponse(data);
    render();
    return data;
  } catch {
    return null;
  }
}

async function openSessionExternal(sessionId) {
  try {
    const res = await fetch(`/api/sessions/${encodeURIComponent(sessionId)}/open`, {
      method: "POST",
    });
    if (!res.ok) {
      showToast("Failed to open session");
      return;
    }
    const data = await res.json();
    if (
      data.resumeCommand &&
      data.action === "copied_resume_command" &&
      navigator.clipboard?.writeText
    ) {
      try {
        await navigator.clipboard.writeText(data.resumeCommand);
      } catch {
        // Native pbcopy may have already populated the clipboard on macOS.
      }
    }
    showToast(data.message || "Opened session");
  } catch {
    showToast("Failed to reach Planar core");
  }
}

function refreshIfIdle() {
  if (
    state.dataSource !== "api" ||
    state.isSyncing ||
    state.route === "settings" ||
    state.popover ||
    state.dragState ||
    Date.now() - lastApiRefreshMs < 3000
  ) {
    return;
  }
  void loadPlaneFromApi(false);
}

// Synchronous plot width measurement BEFORE HTML generation (eliminates double-render jitter!)
function computePlotWidthSync() {
  const existingPlot = document.querySelector(".swimlane-plot-area");
  if (existingPlot) {
    const w = Math.round(existingPlot.getBoundingClientRect().width);
    if (w > 80) return w;
  }
  const shell = document.getElementById("app-shell");
  if (!shell) return 644;
  const cs = window.getComputedStyle(shell);
  const innerW =
    shell.clientWidth - parseFloat(cs.paddingLeft || "0") - parseFloat(cs.paddingRight || "0");
  const labelW = parseFloat(cs.getPropertyValue("--swimlane-label-w")) || 148;
  const gapW = parseFloat(cs.getPropertyValue("--swimlane-gap")) || 12;
  return Math.max(130, Math.round(innerW - labelW - gapW));
}

function syncElementScrollMask(el) {
  if (!el) return { canTop: false, canBottom: false };
  const canTop = el.scrollTop > 6;
  const canBottom = el.scrollHeight - el.clientHeight - el.scrollTop > 6;
  el.classList.toggle("has-scroll-top", canTop);
  el.classList.toggle("has-scroll-bottom", canBottom);
  return { canTop, canBottom };
}

function syncScrollAffordances() {
  const settingsContent = document.querySelector(".settings-content");
  if (settingsContent) {
    const { canBottom } = syncElementScrollMask(settingsContent);
    const cue = settingsContent.parentElement?.querySelector(".scroll-more-cue");
    if (cue) cue.classList.toggle("is-visible", canBottom);
  }

  const pickerList = document.querySelector(".picker-list");
  if (pickerList) {
    syncElementScrollMask(pickerList);
  }

  const veil = document.getElementById("viewport-scroll-veil");
  if (veil) {
    if (state.route !== "main") {
      veil.classList.remove("is-visible");
    } else {
      const doc = document.documentElement;
      const canMainBottom = doc.scrollHeight - window.innerHeight - window.scrollY > 12;
      veil.classList.toggle("is-visible", canMainBottom);
    }
  }
}

let scrollAffordanceRaf = 0;
function scheduleScrollAffordanceSync() {
  if (scrollAffordanceRaf) return;
  scrollAffordanceRaf = requestAnimationFrame(() => {
    scrollAffordanceRaf = 0;
    syncScrollAffordances();
  });
}

document.addEventListener("scroll", scheduleScrollAffordanceSync, { capture: true, passive: true });
window.addEventListener("resize", scheduleScrollAffordanceSync, { passive: true });

function render() {
  applyThemeAndLang();
  state.hoveredSessionKey = null;
  const motion = state.routeMotion ? `is-${state.routeMotion}` : "";
  state.routeMotion = "";
  const isSettings = state.route === "settings";
  document.documentElement.dataset.route = state.route;
  const shell = document.getElementById("app-shell");
  if (!shell) return;
  const prevSettingsContent = isSettings ? shell.querySelector(".settings-content") : null;
  const savedSettingsTop = prevSettingsContent ? prevSettingsContent.scrollTop : 0;
  const prevSettingsNav = isSettings ? shell.querySelector(".settings-nav") : null;
  const savedNavLeft = prevSettingsNav ? prevSettingsNav.scrollLeft : 0;
  const prevMainScrollY = !isSettings
    ? Math.max(window.scrollY || 0, document.documentElement.scrollTop || 0, document.body.scrollTop || 0)
    : 0;
  shell.classList.toggle("is-settings-route", isSettings);

  if (isSettings) {
    shell.innerHTML = renderSettingsView(motion);
    const nextContent = shell.querySelector(".settings-content");
    if (nextContent) {
      nextContent.scrollTop = state.resetSettingsScroll ? 0 : savedSettingsTop;
    }
    const nextNav = shell.querySelector(".settings-nav");
    if (nextNav) {
      nextNav.scrollLeft = savedNavLeft;
      if (state.resetSettingsScroll) {
        nextNav.querySelector(".settings-nav-item.is-active")?.scrollIntoView({
          block: "nearest",
          inline: "nearest",
        });
      }
    }
    state.resetSettingsScroll = false;
    bindAddLineForm();
  } else {
    state.plotWidth = computePlotWidthSync();
    shell.innerHTML = renderMainView(motion);
    if (!motion && prevMainScrollY > 0) {
      window.scrollTo(0, prevMainScrollY);
    }
  }
  renderPopoverLayer();
  syncScrollAffordances();
}

// Width-only ResizeObserver on .app-shell: responds to narrow IDE sidebar splitters while
// ignoring vertical height changes during expand/collapse (zero jitter).
let resizeRaf = 0;
let lastObservedShellW = 0;
const shellObserver = new ResizeObserver((entries) => {
  const entry = entries[0];
  if (!entry) return;
  const w = Math.round(entry.contentRect.width);
  if (w === lastObservedShellW) return;
  lastObservedShellW = w;
  if (state.route === "settings") return;
  if (resizeRaf) cancelAnimationFrame(resizeRaf);
  resizeRaf = requestAnimationFrame(() => {
    resizeRaf = 0;
    const nextW = computePlotWidthSync();
    if (Math.abs(nextW - state.plotWidth) >= 1) {
      state.plotWidth = nextW;
      render();
    }
  });
});
const shellRoot = document.getElementById("app-shell");
if (shellRoot) shellObserver.observe(shellRoot);

let resetConfirmTimer = 0;

async function runSyncBatches() {
  if (state.isSyncing) return;
  if (state.dataSource !== "api") {
    if (state.unsummarized.length === 0) {
      showToast(t("toastScanDone"));
      return;
    }
    state.isSyncing = true;
    state.syncStopRequested = false;
    state.syncProcessedTotal = 0;
    state.syncInitialPending = state.unsummarized.length;
    render();
    setTimeout(() => {
      const processedCount = state.unsummarized.length;
      for (const u of state.unsummarized) {
        const target = state.lines.find((l) => l.id === u.targetLineId) || state.lines[0];
        target.sessions.unshift({
          id: u.id,
          harness: u.harness || "codex",
          model: u.model || null,
          totalTokens: u.totalTokens || 0,
          costUsd: u.costUsd ?? null,
          title: u.resolvedTitle,
          rawTitle: u.firstPrompt,
          steps: u.steps,
          recency: u.running ? "running" : "just now",
          running: Boolean(u.running),
          stoppedAt:
            lang() === "zh"
              ? "刚刚通过归纳完成首中尾切片摘要并归入当前工作线。"
              : "Summarized via Head–Middle–Tail slicer and filed under this line.",
          leftOpen: "",
          dependsOnId: null,
          hidden: false,
          bursts: u.bursts,
        });
      }
      state.unsummarized = [];
      state.syncProcessedTotal = processedCount;
      state.isSyncing = false;
      state.syncStopRequested = false;
      render();
      showToast(t("toastSynced", { n: processedCount, backend: "mock" }));
    }, 480);
    return;
  }

  const visiblePending = state.unsummarized.filter(matchesHarness);
  state.isSyncing = true;
  state.syncStopRequested = false;
  state.syncProcessedTotal = 0;
  state.syncCompletedCount = 0;
  state.syncFailedCount = 0;
  state.syncInitialPending = visiblePending.length;
  state.syncBatchIndex = 0;
  state.syncTotalBatches = 0;
  state.syncBatchItems = [];
  state.syncLastFiled = null;
  const ctrl = new AbortController();
  state.syncAbortController = ctrl;
  render();

  let backendUsed = state.settings.processorMode || DEFAULT_SETTINGS.processorMode;
  let lastErrorMsg = "";
  let stoppedByUser = false;
  try {
    // Always invoke `/api/sync/stream` (which scans once on the backend) so grown
    // sessions and `slice_config_hash`-invalidated digests refresh even when the
    // unassigned `unsummarized` queue is empty.
    const syncSuffix =
      state.selectedHarness && state.selectedHarness !== "all"
        ? `${apiQuerySuffix()}&harness=${encodeURIComponent(state.selectedHarness)}`
        : apiQuerySuffix();

    const res = await fetch(`/api/sync/stream?${syncSuffix}`, {
      method: "POST",
      signal: ctrl.signal,
    });
    if (!res.ok || !res.body) {
      const fallback = await apiApply(`/api/sync?${syncSuffix}`, "POST");
      if (fallback?.lastSync?.backendUsed) backendUsed = fallback.lastSync.backendUsed;
      if (fallback?.lastSync?.lastError) lastErrorMsg = fallback.lastSync.lastError;
      state.syncProcessedTotal = fallback?.lastSync?.processed || 0;
      state.syncFailedCount = fallback?.lastSync?.failed || 0;
    } else {
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";

      const handleEventLine = (rawLine) => {
        const trimmed = rawLine.trim();
        if (!trimmed) return false;
        let evt = null;
        try {
          evt = JSON.parse(trimmed);
        } catch {
          return false;
        }
        if (!evt || typeof evt !== "object") return false;

        if (evt.type === "start") {
          state.syncInitialPending = typeof evt.total === "number" ? evt.total : state.syncInitialPending;
          state.syncTotalBatches = evt.totalBatches || 0;
          if (evt.backend) backendUsed = evt.backend;
          render();
          return false;
        }
        if (evt.type === "batchStart") {
          state.syncBatchIndex = evt.batchIndex || 1;
          if (evt.totalBatches > 0) state.syncTotalBatches = evt.totalBatches;
          state.syncBatchItems = Array.isArray(evt.items) ? evt.items : [];
          render();
          return false;
        }
        if (evt.type === "heartbeat") {
          lastApiRefreshMs = Date.now();
          return false;
        }
        if (evt.type === "batchDone") {
          state.syncBatchItems = [];
          state.syncProcessedTotal = evt.processed || 0;
          state.syncCompletedCount = evt.completed || state.syncProcessedTotal;
          state.syncFailedCount = evt.failed || 0;
          if (typeof evt.total === "number") {
            state.syncInitialPending = evt.total;
          }
          if (evt.totalBatches > 0) state.syncTotalBatches = evt.totalBatches;
          if (evt.backendUsed) backendUsed = evt.backendUsed;
          if (evt.error) lastErrorMsg = evt.error;
          if (Array.isArray(evt.filed) && evt.filed.length > 0) {
            const firstFiled = evt.filed[0];
            state.syncLastFiled = {
              lineName: firstFiled.lineName || "One-off",
              title: firstFiled.title || "",
              extraCount: Math.max(0, evt.filed.length - 1),
            };
          }
          if (evt.plane) {
            lastApiRefreshMs = Date.now();
            applyPlaneResponse(evt.plane);
          }
          render();
          if (state.syncStopRequested) {
            stoppedByUser = true;
            ctrl.abort();
            return true;
          }
          return false;
        }
        if (evt.type === "complete") {
          if (evt.summary) {
            state.syncProcessedTotal = evt.summary.processed || 0;
            state.syncFailedCount = evt.summary.failed || 0;
            if (evt.summary.backendUsed) backendUsed = evt.summary.backendUsed;
            if (evt.summary.lastError) lastErrorMsg = evt.summary.lastError;
          }
          if (evt.plane) {
            lastApiRefreshMs = Date.now();
            applyPlaneResponse(evt.plane);
          }
          render();
          return false;
        }
        return false;
      };

      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let nlIdx = buf.indexOf("\n");
        let shouldStop = false;
        while (nlIdx !== -1) {
          const line = buf.slice(0, nlIdx);
          buf = buf.slice(nlIdx + 1);
          if (handleEventLine(line)) {
            shouldStop = true;
            break;
          }
          nlIdx = buf.indexOf("\n");
        }
        if (shouldStop) break;
      }
      const tail = buf + decoder.decode();
      if (tail.trim() && !stoppedByUser) {
        handleEventLine(tail);
      }
    }

    if (stoppedByUser) {
      showToast(t("toastSyncStopped", { n: state.syncProcessedTotal }));
    } else if (state.syncProcessedTotal > 0 && state.syncFailedCount > 0) {
      showToast(
        t("toastSyncedWithFail", {
          n: state.syncProcessedTotal,
          failed: state.syncFailedCount,
          backend: backendUsed,
        })
      );
    } else if (state.syncProcessedTotal > 0) {
      showToast(t("toastSynced", { n: state.syncProcessedTotal, backend: backendUsed }));
    } else if (lastErrorMsg) {
      showToast(lastErrorMsg);
    } else {
      showToast(t("toastScanDone"));
    }
  } catch (err) {
    if (err?.name === "AbortError" || state.syncStopRequested) {
      showToast(t("toastSyncStopped", { n: state.syncProcessedTotal }));
    } else if (state.syncProcessedTotal > 0) {
      showToast(t("toastSynced", { n: state.syncProcessedTotal, backend: backendUsed }));
    } else {
      showToast(lastErrorMsg || "Sync connection interrupted");
    }
  } finally {
    state.isSyncing = false;
    state.syncStopRequested = false;
    state.syncBatchItems = [];
    state.syncAbortController = null;
    render();
  }
}

// Click & Change Handlers
document.addEventListener("click", (e) => {
  const popoverLayer = document.getElementById("popover-layer");
  if (!popoverLayer.hidden && e.target === popoverLayer) {
    state.popover = null;
    if (tooltipEl) tooltipEl.hidden = true;
    render();
    return;
  }

  const svgLane = e.target.closest(".lane-group");
  if (svgLane?.dataset.lineId && !svgLane.dataset.lineId.startsWith("__")) {
    const lid = svgLane.dataset.lineId;
    if (state.expandedTimelineLines.has(lid)) state.expandedTimelineLines.delete(lid);
    else state.expandedTimelineLines.add(lid);
    render();
    return;
  }

  const btn = e.target.closest("[data-action]");
  if (!btn) return;
  const action = btn.dataset.action;

  if (action === "set-settings-tab") {
    const nextTab = btn.dataset.tab || "lines";
    if (nextTab !== state.settingsTab) {
      state.resetSettingsScroll = true;
    }
    state.settingsTab = nextTab;
    state.popover = null;
    state.confirmResetDefaults = false;
    render();
    return;
  }

  if (action === "open-theme-menu") {
    e.stopPropagation();
    if (state.popover?.type === "theme-menu") {
      state.popover = null;
      render();
      return;
    }
    const r = btn.getBoundingClientRect();
    state.popover = { type: "theme-menu", top: r.bottom + 6, right: r.right };
    render();
    return;
  }

  if (action === "set-theme-quick") {
    urlThemeOverride = null;
    state.settings.theme = btn.dataset.theme || DEFAULT_SETTINGS.theme;
    state.popover = null;
    render();
    persistSettings();
    return;
  }

  if (action === "set-metric-lens") {
    const raw = btn.dataset.lens;
    const nextLens =
      raw === "tokens" || raw === "cost" || raw === "time" ? raw : DEFAULT_SETTINGS.metricLens;
    urlLensOverride = null;
    if (state.settings.metricLens !== nextLens) {
      state.settings.metricLens = nextLens;
      render();
      persistSettings();
    }
    return;
  }

  if (action === "toggle-show-all-lines") {
    state.showAllLines = !state.showAllLines;
    render();
    return;
  }

  if (action === "toggle-archived-sessions") {
    e.stopPropagation();
    const lid = btn.dataset.lineId;
    if (state.expandedArchivedInLine.has(lid)) state.expandedArchivedInLine.delete(lid);
    else state.expandedArchivedInLine.add(lid);
    render();
    return;
  }

  if (action === "dismiss-line-guide") {
    state.dismissedLineGuide = true;
    try {
      window.localStorage?.setItem("planar.dismissedLineGuide", "1");
    } catch {
      // Ignore storage failures
    }
    render();
    return;
  }

  if (action === "open-lines-settings") {
    openSettingsPage("lines");
    return;
  }

  if (action === "set-setting-val") {
    const field = btn.dataset.field;
    const val = btn.dataset.val;
    if (!field || val === undefined) return;
    if (field === "language") urlLangOverride = null;
    if (field === "theme") urlThemeOverride = null;
    if (field === "cliHarness" || field === "apiProvider") {
      state.processorTestResult = null;
    }
    state.settings[field] = val;
    render();
    persistSettings();
    return;
  }

  if (action === "toggle-cli-advanced") {
    state.showCliBinAdvanced = !state.showCliBinAdvanced;
    render();
    return;
  }

  if (action === "toggle-apikey-vis") {
    state.showApiKeyPlain = !state.showApiKeyPlain;
    render();
    return;
  }

  if (action === "toggle-harness-path-edit") {
    const h = btn.dataset.harness;
    if (!h) return;
    if (state.editingHarnessPaths.has(h)) {
      state.editingHarnessPaths.delete(h);
    } else {
      state.editingHarnessPaths.add(h);
    }
    render();
    return;
  }

  if (action === "reset-harness-path") {
    const h = btn.dataset.harness;
    if (!h) return;
    state.settings.harnessPaths = {
      ...DEFAULT_SETTINGS.harnessPaths,
      ...(state.settings.harnessPaths || {}),
      [h]: "",
    };
    state.editingHarnessPaths.delete(h);
    render();
    persistSettings();
    return;
  }

  if (action === "open-line-color-menu") {
    e.stopPropagation();
    if (state.popover?.type === "line-color-menu" && state.popover.lineId === btn.dataset.lineId) {
      state.popover = null;
      render();
      return;
    }
    const r = btn.getBoundingClientRect();
    state.popover = {
      type: "line-color-menu",
      lineId: btn.dataset.lineId,
      top: r.bottom + 5,
      left: r.left,
    };
    renderPopoverLayer();
    return;
  }

  if (action === "set-line-color") {
    const line = state.lines.find((l) => l.id === btn.dataset.lineId);
    const color = btn.dataset.color;
    state.popover = null;
    if (line && color) {
      line.color = color;
      markSettingsSaved();
      render();
      if (state.dataSource === "api") {
        void apiApply(`/api/lines/${encodeURIComponent(line.id)}`, "PATCH", { color });
      }
      return;
    }
    renderPopoverLayer();
    return;
  }

  if (action === "filter-harness") {
    const next = btn.dataset.harness || "all";
    state.selectedHarness = next !== "all" && state.selectedHarness === next ? "all" : next;
    render();
    return;
  }

  if (action === "select-contrib-day") {
    const iso = btn.dataset.date;
    if (!iso) return;
    if (tooltipEl) tooltipEl.hidden = true;
    state.popover = null;
    const latestIso = state.contributions?.[state.contributions.length - 1]?.date || "";
    if (state.anchorDate === iso || iso === latestIso) {
      state.anchorDate = null;
      if (iso === latestIso) state.mode = "day";
    } else {
      state.anchorDate = iso;
      state.mode = "day";
    }
    if (state.dataSource === "api") {
      void loadPlaneFromApi(false).then((ok) => {
        if (ok) {
          state.selectedDayIdx = Math.max(0, state.days.length - 1);
          render();
        }
      });
      return;
    }
    const foundIdx = state.days.findIndex((d) => d.isoDate === iso);
    if (foundIdx >= 0) state.selectedDayIdx = foundIdx;
    render();
    return;
  }

  if (action === "reset-anchor-date") {
    state.anchorDate = null;
    state.popover = null;
    if (tooltipEl) tooltipEl.hidden = true;
    if (state.dataSource === "api") {
      void loadPlaneFromApi(false);
      return;
    }
    state.selectedDayIdx = Math.max(0, state.days.length - 1);
    render();
    return;
  }

  if (action === "click-timeline-lane") {
    const lid = btn.dataset.lineId;
    if (lid && !lid.startsWith("__")) {
      if (state.expandedTimelineLines.has(lid)) state.expandedTimelineLines.delete(lid);
      else state.expandedTimelineLines.add(lid);
      render();
    }
    return;
  }

  if (action === "set-mode") {
    const nextMode = btn.dataset.mode;
    if (nextMode === "day" && state.mode !== "day") {
      state.selectedDayIdx = Math.max(0, state.days.length - 1);
    }
    state.mode = nextMode;
    if (nextMode === "month") {
      state.rangeShort = "30d";
      state.rangeLabel = t("range30");
    } else if (nextMode === "week" && state.rangeShort === "30d") {
      state.rangeShort = "7d";
      state.rangeLabel = t("range7");
    }
    if (state.dataSource === "mock") {
      ensureMockWindowDays(rangeDays());
    }
    render();
    if (state.dataSource === "api" && nextMode !== "day" && state.days.length !== rangeDays()) {
      void loadPlaneFromApi(false);
    }
    return;
  }

  if (action === "open-range-menu") {
    e.stopPropagation();
    if (state.popover?.type === "range-menu") {
      state.popover = null;
      render();
      return;
    }
    const r = btn.getBoundingClientRect();
    state.popover = { type: "range-menu", top: r.bottom + 6, left: r.left, right: r.right };
    render();
    const contribScroller = document.querySelector(".contrib-scroll");
    if (contribScroller) {
      contribScroller.scrollLeft = contribScroller.scrollWidth;
    }
    return;
  }

  if (action === "select-range") {
    state.rangeLabel = btn.dataset.range;
    state.rangeShort = btn.dataset.short || "7d";
    state.popover = null;
    if (state.rangeShort === "30d" && state.mode !== "day") {
      state.mode = "month";
    } else if (state.rangeShort !== "30d" && state.mode === "month") {
      state.mode = "week";
    }
    if (state.dataSource === "mock") {
      ensureMockWindowDays(rangeDays());
    }
    render();
    if (state.dataSource === "api") {
      void loadPlaneFromApi(false);
    }
    return;
  }

  if (action === "select-day") {
    state.selectedDayIdx = Number(btn.dataset.dayIdx);
    state.mode = "day";
    render();
    return;
  }

  if (action === "toggle-list-line") {
    const lid = btn.dataset.lineId;
    if (state.expandedListLines.has(lid)) state.expandedListLines.delete(lid);
    else state.expandedListLines.add(lid);
    render();
    return;
  }

  if (action === "toggle-session-recap") {
    const sid = btn.dataset.sessionId;
    if (state.expandedSessionIds.has(sid)) state.expandedSessionIds.delete(sid);
    else state.expandedSessionIds.add(sid);
    render();
    return;
  }

  if (action === "expand-idle-sessions") {
    e.stopPropagation();
    state.showAllIdleInLine.add(btn.dataset.lineId);
    render();
    return;
  }

  if (action === "collapse-idle-sessions") {
    e.stopPropagation();
    state.showAllIdleInLine.delete(btn.dataset.lineId);
    render();
    return;
  }

  if (action === "toggle-idle-groups") {
    state.hideIdleGroups = !state.hideIdleGroups;
    render();
    return;
  }

  if (action === "open-line-menu") {
    e.stopPropagation();
    const r = btn.getBoundingClientRect();
    state.popover = { type: "line-menu", lineId: btn.dataset.lineId, top: r.bottom + 5, left: r.left };
    render();
    return;
  }

  if (action === "set-priority") {
    const priority = Number(btn.dataset.priority);
    const line = state.lines.find((l) => l.id === btn.dataset.lineId);
    if (line) line.priority = priority;
    state.popover = null;
    render();
    if (state.dataSource === "api" && line) {
      void apiApply(`/api/lines/${encodeURIComponent(line.id)}`, "PATCH", { priority });
    }
    return;
  }

  if (action === "set-status") {
    const status = btn.dataset.status;
    const line = state.lines.find((l) => l.id === btn.dataset.lineId);
    if (line) line.status = status;
    state.popover = null;
    render();
    if (state.dataSource === "api" && line) {
      void apiApply(`/api/lines/${encodeURIComponent(line.id)}`, "PATCH", { status });
    }
    return;
  }

  if (action === "rename-line") {
    const line = state.lines.find((l) => l.id === btn.dataset.lineId);
    state.popover = null;
    if (line) {
      const next = window.prompt(t("renamePrompt"), line.name)?.trim();
      if (next) {
        line.name = next;
        markSettingsSaved();
        if (state.dataSource === "api") {
          void apiApply(`/api/lines/${encodeURIComponent(line.id)}`, "PATCH", { name: next });
        }
      }
    }
    render();
    return;
  }

  if (action === "open-merge-line-picker") {
    e.stopPropagation();
    const lid = btn.dataset.lineId;
    const r = btn.getBoundingClientRect();
    state.popover = {
      type: "merge-line-picker",
      lineId: lid,
      top: r.bottom + 5,
      left: r.left,
    };
    renderPopoverLayer();
    return;
  }

  if (action === "confirm-merge-line") {
    e.stopPropagation();
    const srcId = btn.dataset.sourceId;
    const tgtId = btn.dataset.targetId;
    state.popover = null;
    executeMergeLines(srcId, tgtId);
    return;
  }

  if (action === "open-upstream-picker") {
    e.stopPropagation();
    const r = btn.getBoundingClientRect();
    state.pickerQuery = "";
    state.popover = {
      type: "upstream-picker",
      sessionId: btn.dataset.sessionId,
      lineId: btn.dataset.lineId,
      top: r.top,
      left: r.left,
    };
    renderPopoverLayer();
    return;
  }

  if (action === "pick-upstream") {
    const sid = btn.dataset.sessionId;
    const upstreamId = btn.dataset.upstreamId;
    const found = findSessionEverywhere(sid);
    state.popover = null;
    if (state.dataSource === "api") {
      void apiApply("/api/dependencies", "PUT", { fromSessionId: sid, toSessionId: upstreamId });
      return;
    }
    if (found) found.session.dependsOnId = upstreamId;
    render();
    return;
  }

  if (action === "unlink-upstream") {
    e.stopPropagation();
    const sid = btn.dataset.sessionId;
    if (state.dataSource === "api") {
      void apiApply(`/api/dependencies/${encodeURIComponent(sid)}`, "DELETE");
      return;
    }
    const found = findSessionEverywhere(sid);
    if (found) found.session.dependsOnId = null;
    render();
    return;
  }

  if (action === "open-reassign-line") {
    e.stopPropagation();
    const r = btn.getBoundingClientRect();
    state.popover = { type: "reassign-line", sessionId: btn.dataset.sessionId, top: r.bottom + 4, left: r.left };
    renderPopoverLayer();
    return;
  }

  if (action === "move-session-to-line") {
    const sid = btn.dataset.sessionId;
    const targetLineId = btn.dataset.targetLineId;
    const found = findSessionEverywhere(sid);
    const target = state.lines.find((l) => l.id === targetLineId);
    state.popover = null;
    if (found && target && found.line?.id !== target.id) {
      state.expandedListLines.add(target.id);
      if (state.dataSource === "api") {
        void apiApply(`/api/sessions/${encodeURIComponent(sid)}`, "PATCH", { lineId: target.id });
        return;
      }
      if (found.line) {
        found.line.sessions = found.line.sessions.filter((s) => s.id !== found.session.id);
        target.sessions.unshift(found.session);
      } else {
        state.unsummarized = state.unsummarized.filter((u) => u.id !== found.session.id);
        target.sessions.unshift({
          ...found.session,
          title: found.session.resolvedTitle || found.session.firstPrompt,
          rawTitle: found.session.firstPrompt,
          stoppedAt: buildUnsummarizedSummary(found.session),
          leftOpen: "",
          dependsOnId: null,
          hidden: false,
        });
      }
    }
    render();
    return;
  }

  if (action === "hide-session") {
    e.stopPropagation();
    const sid = btn.dataset.sessionId;
    state.expandedSessionIds.delete(sid);
    if (state.dataSource === "api") {
      void apiApply(`/api/sessions/${encodeURIComponent(sid)}`, "PATCH", { hidden: true });
      return;
    }
    const found = findSessionEverywhere(sid);
    if (found) {
      if (found.line) found.session.hidden = true;
      else state.unsummarized = state.unsummarized.filter((u) => u.id !== sid);
    }
    render();
    return;
  }

  if (action === "open-external-session") {
    e.stopPropagation();
    const sid = btn.dataset.sessionId;
    if (state.dataSource === "api") {
      void openSessionExternal(sid);
      return;
    }
    const found = findSessionEverywhere(sid);
    if (found) {
      const s = found.session;
      const lines = [`Opening session in harness: ${s.harness || "agent"}`];
      if (s.title || s.firstPrompt) lines.push("", s.title || s.firstPrompt);
      if (s.cwd) lines.push("", `Workspace: ${s.cwd}`);
      if (s.sourcePath) lines.push(`Log: ${s.sourcePath}`);
      window.alert(lines.join("\n"));
    }
    return;
  }

  if (action === "stop-sync-after-batch") {
    if (state.syncStopRequested && state.syncAbortController) {
      state.syncAbortController.abort();
    } else {
      state.syncStopRequested = true;
    }
    render();
    return;
  }

  if (action === "close-popover") {
    state.popover = null;
    render();
    return;
  }

  if (action === "goto-processor-setup") {
    state.popover = null;
    openSettingsPage("processor");
    return;
  }

  if (action === "open-cli-select") {
    e.stopPropagation();
    if (state.popover?.type === "cli-select") {
      state.popover = null;
      render();
      return;
    }
    const r = btn.getBoundingClientRect();
    state.popover = { type: "cli-select", top: r.bottom + 6, right: r.right };
    render();
    return;
  }

  if (action === "select-cli-engine") {
    const cli = btn.dataset.cli || "";
    if (!cli) return;
    const prevCli = (state.settings.cliHarness || "").trim();
    const prevDefaultModel = HARNESS_META[prevCli]?.defaultCliModel || "";
    const nextDefaultModel = HARNESS_META[cli]?.defaultCliModel || "";
    state.settings.cliHarness = cli;
    state.settings.processorMode = "cli";
    state.settings.processorConfigured = true;
    if (!state.settings.cliModel || state.settings.cliModel === prevDefaultModel) {
      state.settings.cliModel = nextDefaultModel;
    }
    state.processorTestResult = null;
    state.popover = null;
    render();
    persistSettings();
    return;
  }

  if (action === "open-api-select") {
    e.stopPropagation();
    if (state.popover?.type === "api-select") {
      state.popover = null;
      render();
      return;
    }
    const r = btn.getBoundingClientRect();
    state.popover = { type: "api-select", top: r.bottom + 6, right: r.right };
    render();
    return;
  }

  if (action === "select-api-provider") {
    const prevProv = getApiProviderMeta(state.settings.apiProvider || DEFAULT_SETTINGS.apiProvider);
    const nextId = btn.dataset.provider || DEFAULT_SETTINGS.apiProvider;
    const nextProv = getApiProviderMeta(nextId);
    state.settings.apiProvider = nextId;
    state.settings.processorMode = "api";
    state.settings.processorConfigured = true;
    if (!state.settings.apiModel || state.settings.apiModel === prevProv.defaultModel) {
      state.settings.apiModel = nextProv.defaultModel;
    }
    state.processorTestResult = null;
    state.popover = null;
    render();
    persistSettings();
    return;
  }

  if (action === "open-lang-select") {
    e.stopPropagation();
    if (state.popover?.type === "lang-select") {
      state.popover = null;
      render();
      return;
    }
    const r = btn.getBoundingClientRect();
    state.popover = { type: "lang-select", top: r.bottom + 6, right: r.right };
    render();
    return;
  }

  if (action === "select-lang") {
    const nextLang =
      btn.dataset.lang === "en"
        ? "en"
        : btn.dataset.lang === "zh"
          ? "zh"
          : DEFAULT_SETTINGS.language;
    urlLangOverride = null;
    state.settings.language = nextLang;
    state.popover = null;
    render();
    persistSettings();
    return;
  }

  if (action === "open-short-threshold-select") {
    e.stopPropagation();
    if (state.popover?.type === "short-threshold-select") {
      state.popover = null;
      render();
      return;
    }
    const r = btn.getBoundingClientRect();
    state.popover = { type: "short-threshold-select", top: r.bottom + 6, right: r.right };
    render();
    return;
  }

  if (action === "select-short-threshold") {
    const n = Math.max(
      1,
      Math.min(50, Number(btn.dataset.threshold) || DEFAULT_SETTINGS.fewestUserInputs)
    );
    state.settings.fewestUserInputs = n;
    state.popover = null;
    render();
    persistSettings();
    return;
  }

  if (action === "sync-now") {
    if (!isProcessorConfigured(state.settings)) {
      e.stopPropagation();
      if (state.route === "settings" && state.settingsTab === "processor") {
        if ((state.settings.processorMode || DEFAULT_SETTINGS.processorMode) === "api") {
          state.settings.processorConfigured = true;
          persistSettings();
          void runSyncBatches();
          return;
        }
        const cliBtn = document.getElementById("cli-engine-select-btn");
        if (cliBtn) {
          const r = cliBtn.getBoundingClientRect();
          state.popover = { type: "cli-select", top: r.bottom + 6, right: r.right };
          render();
          return;
        }
      }
      const r = btn.getBoundingClientRect();
      state.popover = { type: "sync-setup-prompt", top: r.bottom + 6, right: r.right };
      render();
      return;
    }
    void runSyncBatches();
    return;
  }

  if (action === "open-settings") {
    openSettingsPage(btn.dataset.tab);
    return;
  }

  if (action === "close-settings" || action === "back-to-main") {
    closeSettingsPage();
    return;
  }

  if (action === "set-processor-mode") {
    const nextMode = btn.dataset.mode === "api" ? "api" : "cli";
    state.settings.processorMode = nextMode;
    if (nextMode === "api") {
      state.settings.processorConfigured = true;
    }
    state.processorTestResult = null;
    render();
    persistSettings();
    return;
  }

  if (action === "test-processor") {
    if (state.isTestingProcessor) return;
    const isCli = (state.settings.processorMode || DEFAULT_SETTINGS.processorMode) !== "api";
    if (isCli && !(state.settings.cliHarness || "").trim()) {
      const cliBtn = document.getElementById("cli-engine-select-btn");
      if (cliBtn) {
        const r = cliBtn.getBoundingClientRect();
        state.popover = { type: "cli-select", top: r.bottom + 6, right: r.right };
        render();
        return;
      }
      state.processorTestResult = {
        ok: false,
        backendUsed: "cli",
        latencyMs: 0,
        message: t("procNeedSelectHint"),
      };
      render();
      return;
    }
    if (!state.settings.processorConfigured) {
      state.settings.processorConfigured = true;
      persistSettings();
    }
    state.isTestingProcessor = true;
    state.processorTestResult = null;
    render();
    if (state.dataSource === "api") {
      void fetch("/api/processor/test", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(state.settings),
      })
        .then(async (res) => {
          if (!res.ok) {
            const errBody = await res.json().catch(() => null);
            throw new Error(errBody?.error || `HTTP ${res.status}`);
          }
          state.processorTestResult = await res.json();
        })
        .catch((err) => {
          state.processorTestResult = {
            ok: false,
            backendUsed: state.settings.processorMode || DEFAULT_SETTINGS.processorMode,
            latencyMs: 0,
            message: err?.message || "Request failed",
          };
        })
        .finally(() => {
          state.isTestingProcessor = false;
          render();
        });
      return;
    }
    setTimeout(() => {
      state.isTestingProcessor = false;
      state.processorTestResult = {
        ok: true,
        backendUsed:
          state.settings.processorMode === "api"
            ? `api:${state.settings.apiProvider || DEFAULT_SETTINGS.apiProvider}`
            : `cli:${state.settings.cliHarness || "auto"}`,
        latencyMs: 24,
        message: "OK (mock)",
      };
      render();
    }, 220);
    return;
  }

  if (action === "regroup-lines") {
    if (state.isRegrouping) return;
    if (!isProcessorConfigured(state.settings)) {
      e.stopPropagation();
      const r = btn.getBoundingClientRect();
      state.popover = { type: "sync-setup-prompt", top: r.bottom + 6, right: r.right };
      render();
      return;
    }
    if (state.dataSource === "api") {
      state.isRegrouping = true;
      render();
      void apiApply("/api/regroup", "POST")
        .then((data) => {
          if (data?.lastSync?.lastError) {
            showToast(data.lastSync.lastError);
          } else if (data?.lastSync) {
            showToast(
              t("toastRegrouped", {
                n: data.lastSync.processed || 0,
                backend:
                  data.lastSync.backendUsed ||
                  state.settings.processorMode ||
                  DEFAULT_SETTINGS.processorMode,
              })
            );
          }
        })
        .finally(() => {
          state.isRegrouping = false;
          render();
        });
      return;
    }
    showToast(t("toastRegrouped", { n: 3, backend: "mock" }));
    return;
  }

  if (action === "toggle-harness-enabled") {
    const h = btn.dataset.harness;
    if (!h) return;
    state.settings.enabledHarnesses = {
      ...DEFAULT_SETTINGS.enabledHarnesses,
      ...(state.settings.enabledHarnesses || {}),
    };
    state.settings.enabledHarnesses[h] = !(state.settings.enabledHarnesses[h] !== false);
    render();
    persistSettings();
    return;
  }

  if (action === "rescan-harnesses") {
    if (state.isRescanning) return;
    if (state.dataSource === "api") {
      state.isRescanning = true;
      render();
      void (async () => {
        try {
          await apiApply("/api/settings", "PUT", state.settings);
          await apiApply("/api/scan", "POST");
          markSettingsSaved();
          showToast(t("toastSavedPaths"));
        } finally {
          state.isRescanning = false;
          render();
        }
      })();
      return;
    }
    markSettingsSaved();
    showToast(t("toastSavedPaths"));
    return;
  }

  if (action === "keep-line") {
    const l = state.lines.find((x) => x.id === btn.dataset.lineId);
    if (l) l.pinned = true;
    markSettingsSaved();
    render();
    if (state.dataSource === "api" && l) {
      void apiApply(`/api/lines/${encodeURIComponent(l.id)}`, "PATCH", { pinned: true });
    }
    return;
  }

  if (action === "unpin-line") {
    const l = state.lines.find((x) => x.id === btn.dataset.lineId);
    if (l) l.pinned = false;
    markSettingsSaved();
    render();
    if (state.dataSource === "api" && l) {
      void apiApply(`/api/lines/${encodeURIComponent(l.id)}`, "PATCH", { pinned: false });
    }
    return;
  }

  if (action === "step") {
    const f = btn.dataset.field;
    const d = Number(btn.dataset.delta);
    const isUnit = f === "fewestUserInputs" || f === "sessionsShownPerLine" || f === "syncBatchLimit";
    const min = isUnit ? 1 : 5;
    const max = isUnit ? 50 : 200;
    state.settings[f] = Math.max(min, Math.min(max, (Number(state.settings[f]) || min) + d));
    render();
    persistSettings();
    return;
  }

  if (action === "scroll-settings-more") {
    const scroller = document.querySelector(".settings-content");
    if (scroller) {
      scroller.scrollBy({ top: Math.round(scroller.clientHeight * 0.68), behavior: "smooth" });
    }
    return;
  }

  if (action === "scroll-main-more") {
    window.scrollBy({ top: Math.round(window.innerHeight * 0.68), behavior: "smooth" });
    return;
  }

  if (action === "toggle-setting") {
    const f = btn.dataset.field;
    state.settings[f] = !state.settings[f];
    render();
    persistSettings();
    return;
  }

  if (action === "reset-defaults") {
    if (!state.confirmResetDefaults) {
      state.confirmResetDefaults = true;
      render();
      if (resetConfirmTimer) clearTimeout(resetConfirmTimer);
      resetConfirmTimer = setTimeout(() => {
        if (state.confirmResetDefaults) {
          state.confirmResetDefaults = false;
          render();
        }
      }, 3500);
      return;
    }
    if (resetConfirmTimer) clearTimeout(resetConfirmTimer);
    state.confirmResetDefaults = false;
    urlLangOverride = null;
    urlThemeOverride = null;
    urlLensOverride = null;
    state.settings = {
      ...DEFAULT_SETTINGS,
      enabledHarnesses: { ...DEFAULT_SETTINGS.enabledHarnesses },
      harnessPaths: { ...DEFAULT_SETTINGS.harnessPaths },
    };
    state.processorTestResult = null;
    render();
    persistSettings();
  }
});

// Pointer-draggable 3-Zone Trajectory Slicer Handles
let activeSlicerDrag = null;

document.addEventListener("pointerdown", (e) => {
  const handle = e.target.closest("[data-slicer-drag]");
  if (!handle) return;
  const track = document.getElementById("slicer-bar-track");
  if (!track) return;
  e.preventDefault();
  const rect = track.getBoundingClientRect();
  activeSlicerDrag = {
    which: handle.dataset.slicerDrag, // "open" | "close"
    left: rect.left,
    width: Math.max(80, rect.width),
    baseTotal: Math.max(120, state.settings.openingSteps + state.settings.closingSteps + 40),
    dirty: false,
  };
  try {
    handle.setPointerCapture(e.pointerId);
  } catch {
    // Ignore synthetic pointer events without capture support
  }
});

document.addEventListener("pointermove", (e) => {
  if (!activeSlicerDrag) return;
  const rel = Math.max(0.06, Math.min(0.94, (e.clientX - activeSlicerDrag.left) / activeSlicerDrag.width));
  const { which, baseTotal } = activeSlicerDrag;
  if (which === "open") {
    const maxOpen = Math.max(5, Math.min(200, baseTotal - state.settings.closingSteps - 20));
    const nextSteps = Math.max(5, Math.min(maxOpen, Math.round((rel * baseTotal) / 5) * 5));
    if (nextSteps !== state.settings.openingSteps) {
      state.settings.openingSteps = nextSteps;
      activeSlicerDrag.dirty = true;
      render();
    }
  } else {
    const maxClose = Math.max(5, Math.min(200, baseTotal - state.settings.openingSteps - 20));
    const nextSteps = Math.max(5, Math.min(maxClose, Math.round(((1 - rel) * baseTotal) / 5) * 5));
    if (nextSteps !== state.settings.closingSteps) {
      state.settings.closingSteps = nextSteps;
      activeSlicerDrag.dirty = true;
      render();
    }
  }
});

function finishSlicerDrag() {
  if (!activeSlicerDrag) return;
  const wasDirty = activeSlicerDrag.dirty;
  activeSlicerDrag = null;
  if (wasDirty) {
    persistSettings();
  }
}

document.addEventListener("pointerup", finishSlicerDrag);
document.addEventListener("pointercancel", finishSlicerDrag);

document.addEventListener("change", (e) => {
  const el = e.target;
  const action = el.dataset?.action;
  if (!action) return;
  let changed = false;

  if (action === "setting-text") {
    const field = el.dataset.field;
    state.settings[field] = el.value.trim();
    if (
      (state.settings.processorMode || DEFAULT_SETTINGS.processorMode) === "api" &&
      (field === "apiBaseUrl" || field === "apiModel" || field === "apiKey")
    ) {
      state.settings.processorConfigured = true;
    }
    state.processorTestResult = null;
    changed = true;
  } else if (action === "harness-path") {
    const h = el.dataset.harness;
    if (h) {
      state.settings.harnessPaths = {
        ...DEFAULT_SETTINGS.harnessPaths,
        ...(state.settings.harnessPaths || {}),
        [h]: el.value.trim(),
      };
      changed = true;
    }
  } else if (action === "char-limit") {
    state.settings[el.dataset.field] = Math.max(0, Number(el.value) || 0);
    changed = true;
  }

  if (changed) {
    persistSettings();
  }
});

document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === ",") {
    e.preventDefault();
    if (state.route === "settings") closeSettingsPage();
    else openSettingsPage();
    return;
  }

  if (e.key === "Escape") {
    if (state.popover) {
      state.popover = null;
      if (tooltipEl) tooltipEl.hidden = true;
      render();
      return;
    }
    if (state.route === "settings") {
      closeSettingsPage();
      return;
    }
    return;
  }

  const tag = document.activeElement?.tagName;
  const isTyping = tag === "INPUT" || tag === "TEXTAREA" || document.activeElement?.isContentEditable;
  if (
    !isTyping &&
    !e.metaKey &&
    !e.ctrlKey &&
    !e.altKey &&
    (e.key === "t" || e.key === "T") &&
    state.route === "main" &&
    state.anchorDate &&
    !state.popover
  ) {
    e.preventDefault();
    state.anchorDate = null;
    if (state.dataSource === "api") {
      void loadPlaneFromApi(false);
    } else {
      state.selectedDayIdx = Math.max(0, state.days.length - 1);
      render();
    }
    return;
  }

  if (!state.popover) return;
  if (state.popover.type === "line-menu") {
    const line = state.lines.find((l) => l.id === state.popover.lineId);
    if (!line) return;
    if (["0", "1", "2", "3", "4"].includes(e.key)) {
      const priority = Number(e.key);
      line.priority = priority;
      state.popover = null;
      render();
      if (state.dataSource === "api") {
        void apiApply(`/api/lines/${encodeURIComponent(line.id)}`, "PATCH", { priority });
      }
    } else if (e.key === "r" || e.key === "R") {
      e.preventDefault();
      state.popover = null;
      const next = window.prompt(t("renamePrompt"), line.name)?.trim();
      if (next) {
        line.name = next;
        if (state.dataSource === "api") {
          void apiApply(`/api/lines/${encodeURIComponent(line.id)}`, "PATCH", { name: next });
        }
      }
      render();
    }
  }
});

// Synchronized Hover, Session → Plane Trajectory Spotlight & 5-Mode Dark Tooltip
const tooltipEl = document.getElementById("timeline-tooltip");

function findSessionForSpotlight(sessionId) {
  const inMain = findSessionEverywhere(sessionId);
  if (inMain) return inMain.session;
  return state.shortSessions.items.find((s) => s.id === sessionId) || null;
}

function setHoveredLane(laneKey) {
  if (state.hoveredLaneId === laneKey) return;
  state.hoveredLaneId = laneKey;
  for (const el of document.querySelectorAll("[data-lane-key]")) {
    el.classList.toggle("is-hovered", el.dataset.laneKey === laneKey);
  }
}

function updateSessionSpotlight(sessRow) {
  const sid = sessRow?.dataset.spotlightSession || "";
  const laneKey = sessRow?.dataset.spotlightLane || "";
  const nextKey = sid && laneKey ? `${laneKey}|${sid}` : null;
  if (state.hoveredSessionKey === nextKey) return;
  state.hoveredSessionKey = nextKey;

  for (const el of document.querySelectorAll(".lane-group.is-dimmed-for-spotlight")) {
    el.classList.remove("is-dimmed-for-spotlight");
  }
  for (const lbl of document.querySelectorAll(".swimlane-label.is-spotlight-parent")) {
    lbl.classList.remove("is-spotlight-parent", "is-hovered");
  }
  const spotLayer = document.getElementById("tl-session-spotlight");
  if (spotLayer) spotLayer.innerHTML = "";
  if (!nextKey || !spotLayer) return;

  // When parent lane is expanded into sub-lanes, `sub:${sid}` is already highlighted directly.
  if (lastRenderedLaneGeo.has(`sub:${sid}`)) return;

  const geo = lastRenderedLaneGeo.get(laneKey);
  const sess = findSessionForSpotlight(sid);
  if (!geo || !sess || !Array.isArray(sess.bursts) || sess.bursts.length === 0) return;

  const spans = [];
  const promptXs = [];
  if (geo.isDay) {
    for (const [bd, t0, t1, prompts] of sess.bursts) {
      if (bd !== geo.dIdx) continue;
      spans.push([geo.xOfDay(t0), geo.xOfDay(t1)]);
      for (const p of prompts || []) promptXs.push(geo.xOfDay(p));
    }
  } else {
    for (const [bd, t0, t1, prompts] of sess.bursts) {
      if (bd < 0 || bd >= geo.numDays) continue;
      spans.push([geo.xOfWindow(bd, t0), geo.xOfWindow(bd, t1)]);
      for (const p of prompts || []) promptXs.push(geo.xOfWindow(bd, p));
    }
  }
  if (spans.length === 0) return;

  const parentGroup = document.querySelector(`.lane-group[data-lane-key="${laneKey}"]`);
  if (parentGroup) parentGroup.classList.add("is-dimmed-for-spotlight");
  const parentLabel = document.querySelector(`.swimlane-label[data-lane-key="${laneKey}"]`);
  if (parentLabel) parentLabel.classList.add("is-spotlight-parent", "is-hovered");

  const islands = bandProfile([spans], false);
  let html = "";
  for (const island of islands) {
    html += `<path class="tl-band" style="--hue:${geo.color}" d="${bandPath(island, geo.cy)}"/>`;
  }
  if (promptXs.length > 0) {
    promptXs.sort((a, b) => a - b);
    let lastTier0X = -999;
    const dotR = geo.isDay ? "1.6" : "1.35";
    for (const px of promptXs) {
      let py = geo.cy - 5.6;
      if (px - lastTier0X < 4.2) {
        py = geo.cy - 8.8;
      } else {
        lastTier0X = px;
      }
      html += `<circle cx="${px.toFixed(1)}" cy="${py.toFixed(1)}" r="${dotR}" fill="${geo.color}" fill-opacity="0.92"/>`;
    }
  }
  spotLayer.innerHTML = html;
}

document.addEventListener("mousemove", (e) => {
  const laneEl = e.target.closest("[data-lane-key]") || e.target.closest("[data-hover-lane]");
  const nextKey = laneEl ? laneEl.dataset.laneKey || laneEl.dataset.hoverLane : null;
  setHoveredLane(nextKey);

  const sessRow = e.target.closest("[data-spotlight-session]");
  updateSessionSpotlight(sessRow);

  const tipTarget = e.target.closest(".tl-hover");
  if (!tipTarget?.dataset.tip) {
    tooltipEl.hidden = true;
    return;
  }
  const data = JSON.parse(tipTarget.dataset.tip);
  tooltipEl.hidden = false;

  if (data.kind === "line-summary") {
    const tokRow = data.tokensText
      ? `<div class="tt-metric-row"><span class="tt-metric-val">${escapeHtml(
          data.tokensText
        )}</span><span class="tt-metric-lbl">${escapeHtml(data.costText || t("tokensLbl"))}</span></div>`
      : "";
    tooltipEl.innerHTML = `
      <div class="tt-heading">${escapeHtml(data.title)}</div>
      <div class="tt-metric-row"><span class="tt-metric-val">${escapeHtml(data.agentTime)}</span><span class="tt-metric-lbl">${escapeHtml(t("agentTimeLbl"))}</span></div>
      <div class="tt-metric-row"><span class="tt-metric-val">${escapeHtml(data.wallClock)}</span><span class="tt-metric-lbl">${escapeHtml(t("wallClockLbl"))}</span></div>
      ${tokRow}
      <div class="tt-sub">${escapeHtml(data.footer)}</div>
    `;
  } else if (data.kind === "sub-summary") {
    const tokRow = data.tokensText
      ? `<div class="tt-metric-row"><span class="tt-metric-val">${escapeHtml(
          data.tokensText
        )}</span><span class="tt-metric-lbl">${escapeHtml(data.costText || t("tokensLbl"))}</span></div>`
      : "";
    tooltipEl.innerHTML = `
      <div class="tt-heading">${escapeHtml(data.title)}</div>
      <div class="tt-metric-row"><span class="tt-metric-val">${escapeHtml(data.activeText)}</span><span class="tt-metric-lbl">${escapeHtml(
        t("activeInLast", { n: state.days.length || rangeDays() })
      )}</span></div>
      ${tokRow}
    `;
  } else if (data.kind === "contrib-day") {
    const mins = Number(data.totalMinutes || 0);
    const dayTok = Number(data.totalTokens || 0);
    const dayCostStr = fmtCostCompact(data.costUsd);
    const tokRow =
      dayTok > 0
        ? `<div class="tt-metric-row"><span class="tt-metric-val">${escapeHtml(
            fmtTokensCompact(dayTok)
          )} tok</span><span class="tt-metric-lbl">${escapeHtml(dayCostStr || t("tokensLbl"))}</span></div>`
        : "";
    const breakdown = HARNESS_ORDER.map((id) => {
      const hm = Number(data.byHarness?.[id] || 0);
      return hm > 0
        ? `<div class="tt-metric-row"><span class="tt-metric-val">${escapeHtml(
            fmtMinutesCompact(hm)
          )}</span><span class="tt-metric-lbl tt-harness-lbl">${harnessIconSvg(id, 10.5)}<span>${escapeHtml(
            harnessLabel(id)
          )}</span></span></div>`
        : "";
    }).join("");
    tooltipEl.innerHTML = `
      <div class="tt-heading">${escapeHtml(data.date || "")}</div>
      <div class="tt-metric-row">
        <span class="tt-metric-val">${escapeHtml(
          mins > 0 ? fmtMinutesCompact(mins) : t("contribNoActivity")
        )}</span>
        <span class="tt-metric-lbl">${Number(data.sessionCount || 0)} ${escapeHtml(t("contribSessions"))}</span>
      </div>
      ${tokRow}
      ${breakdown}
    `;
  } else if (data.kind === "day-bar") {
    tooltipEl.innerHTML = `
      <div class="tt-heading">${escapeHtml(data.title)}</div>
      <div class="tt-metric-row"><span class="tt-metric-val">${escapeHtml(data.sub)}</span></div>
    `;
  } else {
    tooltipEl.innerHTML = `<div class="tt-heading" style="margin:0">${escapeHtml(data.main)}</div>`;
  }

  tooltipEl.style.left = `${Math.min(window.innerWidth - tooltipEl.offsetWidth - 10, Math.max(10, e.clientX - 24))}px`;
  tooltipEl.style.top = `${Math.max(10, e.clientY - tooltipEl.offsetHeight - 12)}px`;
});

// Drag-and-Drop Session Dependency Tree
let dragBadgeEl = null;

function ensureDragBadge() {
  if (!dragBadgeEl) {
    dragBadgeEl = document.createElement("div");
    dragBadgeEl.className = "drag-badge";
    dragBadgeEl.hidden = true;
    document.body.appendChild(dragBadgeEl);
  }
  return dragBadgeEl;
}

function executeMergeLines(sourceId, targetId) {
  if (!sourceId || !targetId || sourceId === targetId) return;
  const src = state.lines.find((l) => l.id === sourceId);
  const tgt = state.lines.find((l) => l.id === targetId);
  if (!src || !tgt || src.isSystemBucket) return;

  const srcName = src.name;
  const tgtName = tgt.name;

  state.expandedListLines.delete(sourceId);
  state.expandedTimelineLines.delete(sourceId);
  state.expandedArchivedInLine.delete(sourceId);

  if (state.dataSource === "api") {
    void apiApply(`/api/lines/${encodeURIComponent(sourceId)}/merge`, "POST", {
      targetLineId: targetId,
      pinTarget: true,
      appendScopeHint: true,
    }).then((res) => {
      if (res) {
        markSettingsSaved();
        showToast(t("toastMerged", { src: srcName, tgt: tgtName }));
      }
    });
    return;
  }

  tgt.sessions = [...tgt.sessions, ...src.sessions];
  if (!tgt.isSystemBucket) {
    tgt.pinned = true;
    const curDesc = (tgt.description || "").trim();
    if (!curDesc.toLowerCase().includes(srcName.toLowerCase())) {
      tgt.description = curDesc ? `${curDesc}; ${srcName}` : `Includes ${srcName}`;
    }
  }
  state.lines = state.lines.filter((l) => l.id !== sourceId);
  markSettingsSaved();
  showToast(t("toastMerged", { src: srcName, tgt: tgtName }));
  render();
}

document.addEventListener("dragstart", (e) => {
  const sessRow = e.target.closest(".session-row[draggable='true']");
  if (sessRow) {
    state.dragState = {
      kind: "session",
      sessionId: sessRow.dataset.sessionId,
      lineId: sessRow.dataset.lineId,
      isCurrentlyChild: sessRow.dataset.isChild === "1",
      targetSessionId: null,
      unlink: false,
    };
    sessRow.classList.add("is-dragging");
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", sessRow.dataset.sessionId);
    return;
  }
  const lineRow = e.target.closest(".line-row[draggable='true']");
  if (lineRow?.dataset.dragLineId) {
    state.dragState = {
      kind: "line",
      sourceLineId: lineRow.dataset.dragLineId,
      targetLineId: null,
    };
    lineRow.classList.add("is-dragging");
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", lineRow.dataset.dragLineId);
  }
});

document.addEventListener("dragover", (e) => {
  if (!state.dragState) return;
  e.preventDefault();
  const badge = ensureDragBadge();

  for (const el of document.querySelectorAll(".session-row.is-drop-target, .line-row.is-drop-target")) {
    el.classList.remove("is-drop-target");
  }

  if (state.dragState.kind === "line") {
    const overLineRow = e.target.closest(".line-row[data-drop-line-id]");
    const targetId = overLineRow?.dataset.dropLineId;
    if (targetId && targetId !== state.dragState.sourceLineId) {
      overLineRow.classList.add("is-drop-target");
      state.dragState.targetLineId = targetId;
      const srcLine = state.lines.find((l) => l.id === state.dragState.sourceLineId);
      const tgtLine = state.lines.find((l) => l.id === targetId);
      badge.textContent = t("mergeDragBadge", {
        src: srcLine?.name || state.dragState.sourceLineId,
        tgt: tgtLine?.name || targetId,
      });
      badge.hidden = false;
    } else {
      state.dragState.targetLineId = null;
      badge.hidden = true;
    }
    badge.style.left = `${Math.min(window.innerWidth - 240, Math.max(10, e.clientX + 12))}px`;
    badge.style.top = `${Math.max(10, e.clientY - 30)}px`;
    return;
  }

  const overRow = e.target.closest(".session-row[draggable='true']");
  if (
    overRow &&
    overRow.dataset.lineId === state.dragState.lineId &&
    overRow.dataset.sessionId !== state.dragState.sessionId
  ) {
    overRow.classList.add("is-drop-target");
    const targetFound = findSessionEverywhere(overRow.dataset.sessionId);
    const shortTitle =
      targetFound && targetFound.session.title.length > 26
        ? `${targetFound.session.title.slice(0, 26)}...`
        : targetFound?.session.title || "";
    state.dragState.targetSessionId = overRow.dataset.sessionId;
    state.dragState.unlink = false;
    badge.textContent = `↳ Depends on "${shortTitle}"`;
    badge.hidden = false;
  } else if (state.dragState.isCurrentlyChild) {
    state.dragState.targetSessionId = null;
    state.dragState.unlink = true;
    badge.textContent = "← Unlink dependency";
    badge.hidden = false;
  } else {
    state.dragState.targetSessionId = null;
    state.dragState.unlink = false;
    badge.hidden = true;
  }

  badge.style.left = `${Math.min(window.innerWidth - 220, Math.max(10, e.clientX + 12))}px`;
  badge.style.top = `${Math.max(10, e.clientY - 30)}px`;
});

document.addEventListener("drop", (e) => {
  if (!state.dragState) return;
  e.preventDefault();
  if (state.dragState.kind === "line") {
    const { sourceLineId, targetLineId } = state.dragState;
    cleanupDrag();
    if (sourceLineId && targetLineId && sourceLineId !== targetLineId) {
      executeMergeLines(sourceLineId, targetLineId);
    }
    return;
  }
  const dragged = findSessionEverywhere(state.dragState.sessionId);
  const { targetSessionId, unlink } = state.dragState;
  cleanupDrag();
  if (dragged) {
    if (state.dataSource === "api") {
      if (targetSessionId) {
        void apiApply("/api/dependencies", "PUT", {
          fromSessionId: dragged.session.id,
          toSessionId: targetSessionId,
        });
        return;
      }
      if (unlink) {
        void apiApply(`/api/dependencies/${encodeURIComponent(dragged.session.id)}`, "DELETE");
        return;
      }
    }
    if (targetSessionId) {
      dragged.session.dependsOnId = targetSessionId;
    } else if (unlink) {
      dragged.session.dependsOnId = null;
    }
  }
  render();
});

document.addEventListener("dragend", () => {
  cleanupDrag();
});

function cleanupDrag() {
  state.dragState = null;
  if (dragBadgeEl) dragBadgeEl.hidden = true;
  for (const el of document.querySelectorAll(
    ".session-row.is-dragging, .session-row.is-drop-target, .line-row.is-dragging, .line-row.is-drop-target"
  )) {
    el.classList.remove("is-dragging", "is-drop-target");
  }
}

// URL query hooks for automated visual testing & direct states
if (initialUrlParams.has("sidebar")) {
  const sbWidth = Math.max(280, Number(initialUrlParams.get("sidebar")) || 350);
  document.body.classList.add("is-sidebar-preview");
  document.body.style.maxWidth = `${sbWidth}px`;
}
if (initialUrlParams.get("lang") === "zh" || initialUrlParams.get("lang") === "en") {
  state.settings.language = initialUrlParams.get("lang");
}
if (["light", "dark", "system"].includes(initialUrlParams.get("theme"))) {
  state.settings.theme = initialUrlParams.get("theme");
}
if (["time", "tokens", "cost"].includes(initialUrlParams.get("lens"))) {
  state.settings.metricLens = initialUrlParams.get("lens");
}
if (["all", ...HARNESS_ORDER].includes(initialUrlParams.get("harness"))) {
  state.selectedHarness = initialUrlParams.get("harness");
}
if (initialUrlParams.get("view") === "settings") {
  state.route = "settings";
}
if (["general", "processor", "harnesses", "lines"].includes(initialUrlParams.get("tab"))) {
  state.settingsTab = initialUrlParams.get("tab");
}

window.__planarOpenSettings = (tab) => {
  openSettingsPage(tab);
};
window.__planarRunSync = () => {
  if (!isProcessorConfigured(state.settings)) {
    openSettingsPage("processor");
    return;
  }
  if (state.route !== "main") {
    state.route = "main";
    state.popover = null;
    render();
  }
  void runSyncBatches();
};
if (initialUrlParams.get("mode") === "day") {
  state.mode = "day";
  state.selectedDayIdx = initialUrlParams.has("day") ? Number(initialUrlParams.get("day")) : 4;
}
if (initialUrlParams.get("mode") === "week") {
  state.mode = "week";
  if (initialUrlParams.get("expand") === "1") state.expandedTimelineLines.add("sqlite-sync");
}
if (initialUrlParams.get("mode") === "month") {
  state.mode = "month";
  state.rangeShort = "30d";
  ensureMockWindowDays(30);
}
if (initialUrlParams.get("hover")) {
  state.hoveredLaneId = `line:${initialUrlParams.get("hover")}`;
}
if (initialUrlParams.get("recap") === "1") {
  state.expandedSessionIds.add("p3");
  state.expandedListLines.add("planar");
}
if (initialUrlParams.get("dep") === "1") {
  const ss2 = findSessionEverywhere("ss2");
  if (ss2) ss2.session.dependsOnId = "ss1";
  state.expandedListLines.add("sqlite-sync");
}
if (initialUrlParams.get("menu") === "priority") {
  state.popover = { type: "line-menu", lineId: "sqlite-sync", top: 285, left: 140 };
}

window.matchMedia?.("(prefers-color-scheme: dark)")?.addEventListener?.("change", () => {
  if ((state.settings.theme || DEFAULT_SETTINGS.theme) === "system") {
    render();
  }
});

render();
if (!shouldForceMock(initialUrlParams)) {
  void loadPlaneFromApi(false);
  setTimeout(() => {
    refreshIfIdle();
  }, 2500);
  window.addEventListener("focus", refreshIfIdle);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") refreshIfIdle();
  });
  setInterval(() => {
    if (document.visibilityState === "visible") refreshIfIdle();
  }, 15000);
}

export { applyPlaneResponse, DEFAULT_SETTINGS, isProcessorConfigured, renderSettingsView, state };
