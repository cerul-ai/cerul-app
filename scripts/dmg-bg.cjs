// Render design/dmg-bg-source.html to the electron-builder DMG background
// (apps/desktop/public/brand/dmg/dmg-background.png + @2x).
const { app, BrowserWindow } = require("electron");
const fs = require("fs");
const path = require("path");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const OUT = path.join(__dirname, "..", "apps", "desktop", "public", "brand", "dmg");
const SRC = "file://" + path.join(OUT, "dmg-background-source.html");

async function run() {
  const win = new BrowserWindow({
    width: 660,
    height: 400,
    show: true,
    useContentSize: true,
    webPreferences: { offscreen: false },
  });
  await win.loadURL(SRC);
  await sleep(700);
  const img = await win.webContents.capturePage();
  const { width } = img.getSize();
  console.log("captured", img.getSize());
  const img2x = width >= 1200 ? img : img.resize({ width: 1320, height: 800 });
  const img1x = img.resize({ width: 660, height: 400 });
  fs.writeFileSync(path.join(OUT, "dmg-background@2x.png"), img2x.toPNG());
  fs.writeFileSync(path.join(OUT, "dmg-background.png"), img1x.toPNG());
  console.log("wrote dmg-background.png + @2x to", OUT);
  win.destroy();
  app.quit();
}
app.whenReady().then(run);
app.on("window-all-closed", () => app.quit());
