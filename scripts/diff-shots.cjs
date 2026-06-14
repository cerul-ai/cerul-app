// One-off design-QA capture: screenshots both the design prototype and the
// implementation (via the fixture harness) into design/diff-shots/*.png.
// Run: node_modules/.bin/electron scripts/diff-shots.cjs
const { app, BrowserWindow } = require("electron");
const fs = require("fs");
const path = require("path");

const OUT = path.join(__dirname, "..", "design", "diff-shots");
const VITE = "http://127.0.0.1:1420";
const PROTO = "http://127.0.0.1:4599/design_handoff_cerul/Cerul%20AFTER.dc.html";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Find a *visible* element containing `text`, scroll it near the top.
const scrollJs = (text, off) => `(() => {
  const sc = document.scrollingElement || document.documentElement;
  const els = [...document.querySelectorAll('div,span,h1,h2,h3,p,button,strong')];
  const cands = els
    .filter(e => e.offsetParent !== null && e.getBoundingClientRect().height > 4 && e.children.length < 10 && e.textContent.includes(${JSON.stringify(text)}))
    .map(e => ({ y: e.getBoundingClientRect().top + sc.scrollTop }))
    .filter(c => c.y > 200)
    .sort((a, b) => a.y - b.y);
  if (!cands.length) return JSON.stringify({ ok: false, sh: sc.scrollHeight });
  const target = Math.max(0, cands[0].y - ${off});
  sc.scrollTop = target;
  return JSON.stringify({ ok: true, y: Math.round(cands[0].y), at: Math.round(sc.scrollTop), n: cands.length });
})()`;

// Click a visible button/element whose text includes `text`.
const clickJs = (text, sel = "button") => `(() => {
  const els = [...document.querySelectorAll(${JSON.stringify(sel)})];
  const el = els.find(e => e.offsetParent !== null && e.textContent.includes(${JSON.stringify(text)}));
  if (!el) return 'miss';
  el.click();
  return 'clicked:' + el.textContent.trim().slice(0,20);
})()`;

const shots = [
  // ---------- IMPLEMENTATION (vite fixture) ----------
  { name: "impl-onboarding-0", kind: "impl", url: `${VITE}/#onboarding?fixture=design`, wait: 1400 },
  { name: "impl-onboarding-1", kind: "impl", url: `${VITE}/#onboarding?fixture=design`, wait: 1400,
    act: clickJs("开始设置", ".onb-actions button"), after: 700 },
  { name: "impl-empty", kind: "impl", url: `${VITE}/#home?fixture=design&empty=1`, wait: 1400 },
  { name: "impl-library", kind: "impl", url: `${VITE}/#library?fixture=design`, wait: 1500 },
  { name: "impl-tasks", kind: "impl", url: `${VITE}/#library?fixture=design`, wait: 1500,
    actFn: async (win) => {
      await win.webContents.executeJavaScript(`(() => { const b=[...document.querySelectorAll('button')].find(x=>x.querySelector('.badge-count')); if(b)b.click(); return !!b; })()`);
    }, after: 800 },

  // ---------- PROTOTYPE (support.js) — target the unique section headings ----------
  { name: "proto-onboarding-0", kind: "proto", url: PROTO, wait: 1800, scroll: ["开屏引导", 24] },
  { name: "proto-onboarding-1", kind: "proto", url: PROTO, wait: 1800,
    act: clickJs("开始设置"), after: 700, scroll: ["开屏引导", 24] },
  { name: "proto-empty", kind: "proto", url: PROTO, wait: 1800, scroll: ["空状态", 24] },
  { name: "proto-library", kind: "proto", url: PROTO, wait: 1800, scroll: ["资料库 · 索引进行中", 24] },
  { name: "proto-tasks", kind: "proto", url: PROTO, wait: 1800,
    act: clickJs("任务 · 1 进行中"), after: 700, scroll: ["资料库 · 索引进行中", 24] },
];

async function run() {
  fs.mkdirSync(OUT, { recursive: true });
  const win = new BrowserWindow({
    width: 1320, height: 900, show: true,
    webPreferences: { offscreen: false },
  });

  const only = process.env.SHOT_KIND;
  for (const s of shots) {
    if (only && s.kind !== only) continue;
    try {
      const h = s.kind === "proto" ? 1320 : 900;
      win.setSize(1320, h);
      await win.loadURL(s.url);
      await sleep(s.wait || 1200);
      if (s.scroll) {
        const r = await win.webContents.executeJavaScript(scrollJs(s.scroll[0], s.scroll[1]));
        console.log(`  scroll ${s.name}: ${r}`);
        await sleep(400);
      }
      if (s.act) {
        const r = await win.webContents.executeJavaScript(s.act);
        console.log(`  act ${s.name}: ${r}`);
      }
      if (s.actFn) await s.actFn(win);
      if (s.after) await sleep(s.after);
      if (s.scroll && (s.act || s.actFn)) {
        await win.webContents.executeJavaScript(scrollJs(s.scroll[0], s.scroll[1]));
        await sleep(300);
      }
      const img = await win.webContents.capturePage();
      fs.writeFileSync(path.join(OUT, `${s.name}.png`), img.toPNG());
      console.log(`✓ ${s.name}`);
    } catch (e) {
      console.log(`✗ ${s.name}: ${e.message}`);
    }
  }
  win.destroy();
  app.quit();
}

app.whenReady().then(run);
app.on("window-all-closed", () => app.quit());
