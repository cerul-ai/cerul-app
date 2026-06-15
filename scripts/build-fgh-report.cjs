// Builds design/Cerul_FGH_fidelity-report.html — prototype vs implementation
// for F/G/H, with screenshots base64-embedded (single portable file).
const fs = require("fs");
const path = require("path");
const SHOTS = path.join(__dirname, "..", "design", "fgh-shots");
const OUT = path.join(__dirname, "..", "design", "Cerul_FGH_fidelity-report.html");

const img = (name) =>
  `data:image/png;base64,${fs.readFileSync(path.join(SHOTS, `${name}.png`)).toString("base64")}`;

const cmp = (proto, after, protoCap, afterCap) => `
  <div class="cmp">
    <figure class="proto"><img loading="lazy" src="${img(proto)}" alt="${protoCap}"><figcaption><span class="b">原型</span>${protoCap}</figcaption></figure>
    <figure class="mine"><img loading="lazy" src="${img(after)}" alt="${afterCap}"><figcaption><span class="b">实现</span>${afterCap}</figcaption></figure>
  </div>`;

const html = `<!doctype html>
<html lang="zh"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Cerul · F/G/H 还原核对（含截图）</title>
<style>
  :root{--ink:#1c2229;--ink2:#525d6b;--ink3:#8a93a0;--card:#fff;--hair:#e6e9ee;--steel:#3e6b9d;--steel-soft:#eef4fb;--steel-line:#d4e0ee;--success:#2e7d56;--success-soft:#e7f4ed;--copper:#a06f39;--copper-soft:#f3ebe0;--flag:#c8553d;--flag-soft:#fbeae5;--card-sh:0 1px 2px rgba(28,40,60,.05),0 14px 30px -12px rgba(28,40,60,.22);--mono:ui-monospace,"SF Mono","JetBrains Mono",Menlo,Consolas,monospace;--sans:-apple-system,system-ui,"PingFang SC","Hiragino Sans GB","Segoe UI",Roboto,sans-serif;}
  *{box-sizing:border-box;} body{margin:0;font-family:var(--sans);color:var(--ink);line-height:1.55;background:radial-gradient(680px 360px at 50% 0%,#fff,rgba(255,255,255,0) 66%),linear-gradient(180deg,#fafbfc,#eef1f4);padding:40px 22px 80px;}
  .wrap{max-width:1060px;margin:0 auto;}
  .eyebrow{font-family:var(--mono);font-size:11px;letter-spacing:.16em;text-transform:uppercase;color:var(--steel);font-weight:600;}
  h1{font-size:30px;letter-spacing:-.02em;margin:8px 0 6px;} .sub{color:var(--ink2);font-size:14.5px;max-width:76ch;} .meta{font-family:var(--mono);font-size:11.5px;color:var(--ink3);margin-top:10px;}
  .scoreboard{display:flex;gap:12px;flex-wrap:wrap;margin:22px 0 30px;}
  .stat{flex:1;min-width:150px;background:var(--card);border:1px solid rgba(255,255,255,.85);box-shadow:var(--card-sh);border-radius:12px;padding:14px 16px;}
  .stat .n{font-size:24px;font-weight:700;font-family:var(--mono);letter-spacing:-.02em;} .stat .l{font-size:12px;color:var(--ink3);margin-top:2px;}
  .stat.green .n{color:var(--success);} .stat.steel .n{color:var(--steel);} .stat.amber .n{color:var(--copper);}
  section{margin:34px 0;} h2{font-size:21px;letter-spacing:-.01em;margin:0 0 4px;display:flex;align-items:center;gap:10px;flex-wrap:wrap;}
  h2 .pill{font-family:var(--mono);font-size:12px;font-weight:600;color:var(--steel);background:var(--steel-soft);border:1px solid var(--steel-line);padding:2px 9px;border-radius:999px;}
  .tag{display:inline-flex;align-items:center;gap:6px;font-family:var(--mono);font-size:10.5px;font-weight:600;padding:2px 9px;border-radius:999px;}
  .tag.match{color:var(--success);background:var(--success-soft);} .tag.note{color:var(--copper);background:var(--copper-soft);}
  .sec-note{color:var(--ink3);font-size:13px;margin:0 0 14px;}
  .cmp{display:grid;grid-template-columns:1fr 1fr;gap:14px;margin:14px 0 6px;}
  .cmp figure{margin:0;background:var(--card);border:1px solid rgba(255,255,255,.85);box-shadow:var(--card-sh);border-radius:12px;overflow:hidden;}
  .cmp figure img{display:block;width:100%;height:auto;background:#eef1f4;}
  .cmp figcaption{padding:9px 13px;font-size:12.5px;font-weight:600;display:flex;align-items:center;gap:8px;border-top:1px solid var(--hair);}
  .cmp .proto figcaption{color:var(--ink2);} .cmp .mine figcaption{color:var(--steel);}
  .cmp figcaption .b{font-family:var(--mono);font-size:10px;font-weight:700;padding:1px 7px;border-radius:999px;}
  .cmp .proto .b{background:#eef1f5;color:var(--ink3);} .cmp .mine .b{background:var(--steel-soft);color:var(--steel);}
  .cap{font-size:12.5px;color:var(--ink2);margin:6px 2px 18px;}
  .cap b{color:var(--ink);}
  .matchbox{background:var(--success-soft);border:1px solid #c8e6d6;border-radius:10px;padding:12px 15px;font-size:13px;color:#23613f;margin:14px 0 0;} .matchbox b{color:#1c4d31;}
  .notebox{background:var(--copper-soft);border:1px solid #e6d3b8;border-radius:10px;padding:12px 15px;font-size:13px;color:#7a5226;margin:12px 0 0;} .notebox b{color:#5e3f1c;}
  code{font-family:var(--mono);font-size:11.5px;background:var(--steel-soft);color:var(--steel);padding:1px 5px;border-radius:4px;}
  footer{margin-top:40px;padding-top:18px;border-top:1px solid var(--hair);font-size:12px;color:var(--ink3);}
  @media(max-width:760px){.cmp{grid-template-columns:1fr;}}
</style></head><body><div class="wrap">

<header>
  <div class="eyebrow">● F / G / H 还原核对 · DESIGN FIDELITY</div>
  <h1>Cerul · F/G/H 原型还原（含截图对比）</h1>
  <p class="sub">按 <code>design/Cerul_FGH_React_Tailwind.html</code> 还原三屏：F 安装包(DMG) · G 主页(有内容) · H 任务面板。左＝原型，右＝实现，均由 Electron 同条件截取。已并入分支 <code>redesign/luminous-handoff</code> / PR #43。</p>
  <p class="meta">2026-06-15 · 截图：design/fgh-shots/*.png</p>
</header>

<div class="scoreboard">
  <div class="stat steel"><div class="n">3 / 3</div><div class="l">屏已还原（F · G · H）</div></div>
  <div class="stat green"><div class="n">G + H</div><div class="l">应用内实现 · fixture 实测</div></div>
  <div class="stat green"><div class="n">F ✓</div><div class="l">真打 DMG 包 · 挂载验证通过</div></div>
</div>

<section>
  <h2><span class="pill">G</span> 主页 · 有内容时 <span class="tag match">已还原</span></h2>
  <p class="sec-note">最显眼的是「继续观看」从横排小卡升级成大幅 hero 横幅；顶部 logo 也换成了桌面端 app 图标。</p>
  ${cmp("proto-G-home", "after-G-home", "原型 · HomeWithContent", "实现 · 真实 app（fixture）")}
  <p class="cap">对比要点：<b>顶部 hero logo</b> 换成了真实的桌面端 app 图标（拉丝银方块 + 石墨标志）+ 钢蓝光晕；<b>继续观看 hero 横幅</b>（深色渐变 + 钢蓝光晕 + 噪点 + 玻璃大播放键 + 来源胶囊 + 时长 + 标题阴影 + 进度条 + 继续播放按钮）已 1:1 还原；大搜索框、统计行、正在索引 chip、最近索引网格都对齐。</p>
  <div class="matchbox"><b>已还原：</b> <b>桌面端 app 图标 hero logo</b> + 光晕、大搜索框、统计行、正在索引 chip、<b>继续观看大横幅</b>（含进度条/继续播放）、最近索引 4 列网格。</div>
</section>

<section>
  <h2><span class="pill">H</span> 任务面板 · 进行中 / 完成 / 失败 <span class="tag match">已还原</span></h2>
  <p class="sec-note">任务抽屉从「分组列表」重做成「<b>可筛选的时间线</b>」——这是 H 的核心改动。</p>
  ${cmp("proto-H-tasks", "after-H-tasks", "原型 · TasksPanel（useState 筛选）", "实现 · 任务抽屉（真实 app）")}
  <p class="cap">对比要点：<b>筛选 chips</b>（全部/进行中/已完成/失败 + 计数，选中＝钢蓝实心）+ <b>本批 $0.00 · 全部在本机</b> 费用胶囊；<b>竖向时间线</b>（钢蓝/红/绿节点）；进行中卡（转写中 pill + 进度条 + %  + 步骤/已用/剩约）、失败卡（失败 pill + 错误井 + 去设置修复 + 技术详情展开）、完成卡（已完成 + 用量）。<b>额外保留</b>了之前加的暂停/取消控制。</p>
  <div class="matchbox"><b>已还原：</b> 筛选 chips + 计数、费用胶囊、时间线节点、进行中/失败/完成三种卡、进度条填充、错误井 + 修复按钮、技术详情展开；筛选交互（useState）实测可用。</div>
</section>

<section>
  <h2><span class="pill">F</span> 安装包 · DMG 拖拽安装 <span class="tag match">已打包验证</span></h2>
  <p class="sec-note">F 是 electron-builder 的 DMG 背景资源。已重绘成原型设计、<b>真打了一个 DMG 包并挂载验证</b>。</p>
  ${cmp("proto-F-installer", "dmg-composite", "原型 · InstallerDMG", "实现 · 真实 DMG 合成效果（背景 + app 图标 + Applications + 标签）")}
  <p class="cap">对比要点：<b>「拖到这里安装」钢蓝标签 + 钢蓝箭头</b>、Applications 钢蓝虚线投放目标、底部 <b>Gatekeeper 提示卡</b>（i 徽章 + 系统设置 › 隐私与安全性 · 仍要打开）。<b>已按反馈删掉顶部品牌锁定</b>（app 图标本身已代表品牌，装机窗里那行 logo+标语用处不大），版面更精简、图标重新居中。右图是真实 app 图标 + Applications 文件夹合成进背景后的效果。</p>
  <div class="matchbox"><b>已真打包验证：</b> <code>electron-builder</code> 打出 <code>Cerul-0.0.1-alpha.4-arm64.dmg</code>（ad-hoc 签名），挂载<b>干净无「已损坏」报错</b>，app 图标（拉丝银方块）+ Applications 别名 + 文字标签全部就位。背景改成<b>近白均匀色</b>——拉伸窗口也无色缝；顶部品牌锁定按反馈移除、图标重新居中。</div>
</section>

<footer>
  三屏均已并入 PR #43。G/H 为应用内 React 实现（fixture 实测）；F 为 DMG 打包背景资源。所有新增样式集中在 <code>apps/desktop/src/styles/handoff.css</code>，功能接线（继续播放、筛选、暂停/取消）均保留。
</footer>
</div></body></html>`;

fs.writeFileSync(OUT, html);
console.log(`wrote ${OUT} (${Math.round(fs.statSync(OUT).size / 1024)} KB)`);
