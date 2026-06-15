// Capture the implemented G (home) + H (tasks) surfaces from the running vite
// fixture into design/fgh-shots/after-*.png for the diff report.
const { app, BrowserWindow } = require("electron");
const fs = require("fs");
const path = require("path");
const OUT = path.join(__dirname, "..", "design", "fgh-shots");
const VITE = "http://127.0.0.1:1420";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const shots = [
  { name: "after-G-home", url: `${VITE}/#home?fixture=design`, h: 900, wait: 1500 },
  {
    name: "after-H-tasks",
    url: `${VITE}/#library?fixture=design`,
    h: 900,
    wait: 1500,
    act: `(() => { const b=[...document.querySelectorAll('button')].find(x=>x.querySelector('.badge-count')); if(b)b.click(); return !!b; })()`,
    after: 700,
  },
];

async function run() {
  fs.mkdirSync(OUT, { recursive: true });
  const win = new BrowserWindow({ width: 1180, height: 900, show: true, useContentSize: true });
  for (const s of shots) {
    try {
      win.setContentSize(1180, s.h);
      await win.loadURL(s.url);
      await sleep(s.wait);
      if (s.act) {
        const r = await win.webContents.executeJavaScript(s.act);
        console.log(`  act ${s.name}: ${r}`);
        await sleep(s.after || 500);
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
