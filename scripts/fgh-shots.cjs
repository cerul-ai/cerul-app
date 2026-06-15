// Capture the F/G/H prototype (design/Cerul_FGH_React_Tailwind.html) sections
// into design/fgh-shots/proto-*.png. Run: <electron> scripts/fgh-shots.cjs
const { app, BrowserWindow } = require("electron");
const fs = require("fs");
const path = require("path");

const OUT = path.join(__dirname, "..", "design", "fgh-shots");
const URL = "http://127.0.0.1:4599/Cerul_FGH_React_Tailwind.html";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const scrollJs = (text, off) => `(() => {
  const els = [...document.querySelectorAll('h2,h3')];
  const el = els.find(e => e.offsetParent !== null && e.textContent.includes(${JSON.stringify(text)}));
  if (!el) return JSON.stringify({ ok: false });
  el.scrollIntoView({ block: 'start' });
  window.scrollBy(0, -${off});
  return JSON.stringify({ ok: true, at: Math.round(window.scrollY) });
})()`;

const shots = [
  { name: "proto-F-installer", scroll: ["安装包 · DMG 拖拽安装", 56], h: 660 },
  { name: "proto-G-home", scroll: ["主页 · 有内容时", 56], h: 980 },
  { name: "proto-H-tasks", scroll: ["任务面板 · 进行中", 56], h: 850 },
];

async function run() {
  fs.mkdirSync(OUT, { recursive: true });
  const win = new BrowserWindow({ width: 1200, height: 980, show: true });
  await win.loadURL(URL);
  await sleep(5000); // CDN React + Tailwind + Babel compile

  for (const s of shots) {
    try {
      win.setSize(1200, s.h);
      await sleep(400);
      const r = await win.webContents.executeJavaScript(scrollJs(s.scroll[0], s.scroll[1]));
      console.log(`  scroll ${s.name}: ${r}`);
      await sleep(500);
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
