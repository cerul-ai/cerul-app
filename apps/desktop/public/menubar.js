const API_BASE_URL = "http://127.0.0.1:7777";
const INTERNAL_API_PREFIX = "/internal";
const REFRESH_MS = 5000;
const LANG_KEY = "cerul.lang.v1";
const RECENT_LIMIT = 3;

const STRINGS = {
  zh: {
    htmlLang: "zh-CN",
    openMain: "打开主窗口 →",
    close: "关闭",
    search: "快速搜索…",
    recent: "最近",
    connectingTitle: "正在连接本地服务…",
    offlineTitle: "本地服务未运行",
    offlineHint: "打开 Cerul 主窗口即可启动",
    emptyTitle: "还没有索引任何内容",
    emptyHint: "把视频或播客拖进主窗口开始",
    needsReview: "{n} 个任务需要处理",
    working: "正在处理…",
    stageDownload: "正在下载…",
    stageTranscribe: "正在转写语音…",
    stageEmbed: "正在生成索引…",
    stageSummarize: "正在生成摘要…",
    stageVision: "正在理解画面…",
    stageScan: "正在扫描来源…",
    footer: "全局搜索",
    untitled: "未命名项目",
    failedMeta: "失败",
    justNow: "刚刚",
    minutesAgo: "{n} 分钟前",
    hoursAgo: "{n} 小时前",
    daysAgo: "{n} 天前",
  },
  en: {
    htmlLang: "en",
    openMain: "Open main window →",
    close: "Close",
    search: "Quick search…",
    recent: "Recent",
    connectingTitle: "Connecting to local service…",
    offlineTitle: "Local service is not running",
    offlineHint: "Open the Cerul main window to start it",
    emptyTitle: "Nothing indexed yet",
    emptyHint: "Drop videos or podcasts into the main window",
    needsReview: "{n} jobs need attention",
    working: "Working…",
    stageDownload: "Downloading…",
    stageTranscribe: "Transcribing audio…",
    stageEmbed: "Building the index…",
    stageSummarize: "Summarizing…",
    stageVision: "Understanding visuals…",
    stageScan: "Scanning sources…",
    footer: "Global search",
    untitled: "Untitled item",
    failedMeta: "Failed",
    justNow: "just now",
    minutesAgo: "{n}m ago",
    hoursAgo: "{n}h ago",
    daysAgo: "{n}d ago",
  },
};

function resolveLang() {
  try {
    const stored = window.localStorage.getItem(LANG_KEY);
    if (stored === "zh" || stored === "en") {
      return stored;
    }
  } catch {
    // Storage unavailable — fall through to the system locale.
  }
  return String(navigator.language || "").toLowerCase().startsWith("zh") ? "zh" : "en";
}

let lang = resolveLang();
let lastSnapshot = null; // { items, jobs } from the most recent successful poll.

function t(key, vars) {
  const template = (STRINGS[lang] && STRINGS[lang][key]) || STRINGS.en[key] || key;
  if (!vars) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (match, name) => (name in vars ? String(vars[name]) : match));
}

const el = {
  openMain: document.getElementById("openMain"),
  closeButton: document.getElementById("closeButton"),
  searchText: document.getElementById("searchText"),
  card: document.getElementById("card"),
  cardStage: document.getElementById("cardStage"),
  cardPercent: document.getElementById("cardPercent"),
  barFill: document.getElementById("barFill"),
  recent: document.getElementById("recent"),
  recentLabel: document.getElementById("recentLabel"),
  recentList: document.getElementById("recentList"),
  state: document.getElementById("state"),
  stateDot: document.getElementById("stateDot"),
  stateTitle: document.getElementById("stateTitle"),
  stateHint: document.getElementById("stateHint"),
  footerLabel: document.getElementById("footerLabel"),
};

function applyStaticStrings() {
  document.documentElement.setAttribute("lang", t("htmlLang"));
  if (el.openMain) el.openMain.textContent = t("openMain");
  if (el.closeButton) el.closeButton.setAttribute("aria-label", t("close"));
  if (el.searchText) el.searchText.textContent = t("search");
  if (el.recentLabel) el.recentLabel.textContent = t("recent");
  if (el.footerLabel) el.footerLabel.textContent = t("footer");
}

async function fetchJson(path) {
  const response = await fetch(`${API_BASE_URL}${INTERNAL_API_PREFIX}${path}`, { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`request failed: ${response.status}`);
  }
  return response.json();
}

function statusOf(record) {
  return String(record && record.status ? record.status : "").toLowerCase();
}

function activeJobOf(jobs) {
  return jobs.find((job) => ["queued", "running", "processing", "active"].includes(statusOf(job))) || null;
}

function failedCountOf(jobs) {
  return jobs.filter((job) => ["failed", "error"].includes(statusOf(job))).length;
}

// Backend stage messages are not localized, so map job metadata onto our own
// strings instead of echoing them into a zh UI.
function stageLabel(job) {
  const hint = `${job.job_type || ""} ${job.stage || ""}`.toLowerCase();
  if (hint.includes("download")) return t("stageDownload");
  if (hint.includes("transcribe") || hint.includes("asr") || hint.includes("whisper")) return t("stageTranscribe");
  if (hint.includes("embed") || hint.includes("index")) return t("stageEmbed");
  if (hint.includes("summar")) return t("stageSummarize");
  if (hint.includes("video") || hint.includes("vision") || hint.includes("understand") || hint.includes("frame")) return t("stageVision");
  if (hint.includes("scan") || hint.includes("discover")) return t("stageScan");
  if (lang === "en" && job.stage_message) return job.stage_message;
  return t("working");
}

function relativeTime(unixSeconds) {
  if (!unixSeconds) {
    return "";
  }
  const deltaSec = Math.max(0, Date.now() / 1000 - unixSeconds);
  if (deltaSec < 90) return t("justNow");
  const minutes = Math.round(deltaSec / 60);
  if (minutes < 60) return t("minutesAgo", { n: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return t("hoursAgo", { n: hours });
  return t("daysAgo", { n: Math.round(hours / 24) });
}

function show(element, visible) {
  if (element) {
    element.hidden = !visible;
  }
}

function renderState(kind) {
  show(el.card, false);
  show(el.recent, false);
  show(el.state, true);
  if (el.stateDot) {
    el.stateDot.classList.toggle("pulse", kind === "connecting");
  }
  if (el.stateTitle) {
    el.stateTitle.textContent = t(kind === "connecting" ? "connectingTitle" : kind === "offline" ? "offlineTitle" : "emptyTitle");
  }
  const hint = kind === "offline" ? t("offlineHint") : kind === "empty" ? t("emptyHint") : "";
  if (el.stateHint) {
    el.stateHint.textContent = hint;
    el.stateHint.hidden = hint === "";
  }
}

function recentRow({ failed, title, meta }) {
  const row = document.createElement("li");
  row.className = "recent-row";
  const dot = document.createElement("span");
  dot.className = `dot ${failed ? "failed" : "done"}`;
  dot.setAttribute("aria-hidden", "true");
  const text = document.createElement("span");
  text.className = "recent-title";
  text.textContent = title;
  const metaEl = document.createElement("span");
  metaEl.className = `recent-meta${failed ? " failed" : ""}`;
  metaEl.textContent = meta;
  row.append(dot, text, metaEl);
  return row;
}

function renderSnapshot(snapshot) {
  const items = Array.isArray(snapshot.items) ? snapshot.items : [];
  const jobs = Array.isArray(snapshot.jobs) ? snapshot.jobs : [];

  const activeJob = activeJobOf(jobs);
  const failed = failedCountOf(jobs);
  const indexed = items
    .filter((item) => statusOf(item) === "indexed" && item.indexed_at)
    .sort((a, b) => (b.indexed_at || 0) - (a.indexed_at || 0))
    .slice(0, RECENT_LIMIT);

  if (!activeJob && failed === 0 && indexed.length === 0) {
    renderState("empty");
    return;
  }

  show(el.state, false);
  show(el.card, Boolean(activeJob));
  if (activeJob) {
    const percent = Math.round(Math.min(Math.max(activeJob.progress || 0, 0), 1) * 100);
    if (el.cardStage) el.cardStage.textContent = stageLabel(activeJob);
    if (el.cardPercent) el.cardPercent.textContent = `${percent}%`;
    if (el.barFill) el.barFill.style.width = `${percent}%`;
  }

  const rows = [];
  if (failed > 0) {
    rows.push(recentRow({ failed: true, title: t("needsReview", { n: failed }), meta: t("failedMeta") }));
  }
  for (const item of indexed) {
    rows.push(
      recentRow({
        failed: false,
        title: item.title || t("untitled"),
        meta: relativeTime(item.indexed_at),
      }),
    );
  }
  show(el.recent, rows.length > 0);
  if (el.recentList) {
    el.recentList.replaceChildren(...rows);
  }
}

async function refresh() {
  try {
    const [items, jobs] = await Promise.all([fetchJson("/items"), fetchJson("/jobs")]);
    lastSnapshot = { items, jobs };
    renderSnapshot(lastSnapshot);
  } catch {
    lastSnapshot = null;
    renderState("offline");
  }
}

async function invokeDesktop(command) {
  const bridge = window.cerulDesktop;
  if (bridge && typeof bridge.invoke === "function") {
    await bridge.invoke(command);
  }
}

for (const button of document.querySelectorAll("[data-command]")) {
  button.addEventListener("click", () => {
    const command = button.getAttribute("data-command");
    if (command) {
      void invokeDesktop(command);
    }
  });
}

// The main window writes the language to localStorage on the same origin, so a
// change there re-localizes this window live.
window.addEventListener("storage", (event) => {
  if (event.key === LANG_KEY) {
    lang = resolveLang();
    applyStaticStrings();
    if (lastSnapshot) {
      renderSnapshot(lastSnapshot);
    } else {
      renderState("offline");
    }
  }
});

applyStaticStrings();
renderState("connecting");
void refresh();
window.setInterval(() => {
  void refresh();
}, REFRESH_MS);
