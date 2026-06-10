const API_BASE_URL = "http://127.0.0.1:7777";
const refreshIntervalMs = 5000;
const numberFormat = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });

const elements = {
  status: document.getElementById("status"),
  indexed: document.getElementById("indexed"),
  queued: document.getElementById("queued"),
  active: document.getElementById("active"),
  progress: document.getElementById("progress"),
  detail: document.getElementById("detail"),
  updated: document.getElementById("updated"),
};

async function fetchJson(path) {
  const response = await fetch(`${API_BASE_URL}${path}`, { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`request failed: ${response.status}`);
  }
  return response.json();
}

function setText(element, value) {
  if (element) {
    element.textContent = value;
  }
}

function setProgress(percent) {
  if (elements.progress) {
    elements.progress.style.width = `${Math.max(0, Math.min(100, percent))}%`;
  }
}

function countByStatus(records, statuses) {
  const statusSet = new Set(statuses);
  return records.filter((record) => statusSet.has(String(record.status ?? "").toLowerCase())).length;
}

function summarizeItems(items) {
  const indexed = countByStatus(items, ["indexed"]);
  const processing = countByStatus(items, ["processing", "transcribing", "embedding", "summarizing"]);
  const pending = countByStatus(items, ["new", "pending", "queued", "discovered"]);
  return { indexed, pending, processing, total: items.length };
}

function summarizeJobs(jobs) {
  const active = countByStatus(jobs, ["running", "processing", "active"]);
  const queued = countByStatus(jobs, ["queued", "pending"]);
  const failed = countByStatus(jobs, ["failed", "error"]);
  const latestActive = jobs.find((job) => ["running", "processing", "active"].includes(String(job.status ?? "").toLowerCase()));
  return { active, queued, failed, latestActive };
}

function renderHealthy(items, jobs) {
  const itemSummary = summarizeItems(items);
  const jobSummary = summarizeJobs(jobs);
  const queued = Math.max(itemSummary.pending, jobSummary.queued);
  const active = Math.max(itemSummary.processing, jobSummary.active);
  const denominator = Math.max(1, itemSummary.total);
  const progress = itemSummary.total > 0 ? (itemSummary.indexed / denominator) * 100 : 0;

  setText(elements.indexed, numberFormat.format(itemSummary.indexed));
  setText(elements.queued, numberFormat.format(queued));
  setText(elements.active, numberFormat.format(active));
  setProgress(progress);

  if (active > 0) {
    setText(elements.status, "Indexing");
    setText(elements.detail, jobSummary.latestActive?.stage_message || "Processing library updates");
  } else if (jobSummary.failed > 0) {
    setText(elements.status, "Needs review");
    setText(elements.detail, `${jobSummary.failed} job${jobSummary.failed === 1 ? "" : "s"} need attention`);
  } else {
    setText(elements.status, "Ready");
    setText(elements.detail, itemSummary.total > 0 ? `${itemSummary.total} item${itemSummary.total === 1 ? "" : "s"} tracked` : "No indexed items yet");
  }

  setText(elements.updated, `Updated ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`);
}

function renderOffline() {
  setText(elements.status, "Offline");
  setText(elements.indexed, "0");
  setText(elements.queued, "0");
  setText(elements.active, "0");
  setText(elements.detail, "Local service is not reachable");
  setText(elements.updated, "Waiting for Cerul");
  setProgress(0);
}

async function refreshStatus() {
  try {
    const [items, jobs] = await Promise.all([fetchJson("/items"), fetchJson("/jobs")]);
    renderHealthy(Array.isArray(items) ? items : [], Array.isArray(jobs) ? jobs : []);
  } catch {
    renderOffline();
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

void refreshStatus();
window.setInterval(() => {
  void refreshStatus();
}, refreshIntervalMs);
