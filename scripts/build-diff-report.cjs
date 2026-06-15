// Builds design/handoff-fidelity-diff-2026-06-14.html with the comparison
// screenshots base64-embedded, so the report is a single portable file.
const fs = require("fs");
const path = require("path");

const SHOTS = path.join(__dirname, "..", "design", "diff-shots");
const OUT = path.join(__dirname, "..", "design", "handoff-fidelity-diff-2026-06-14.html");

const img = (name) => {
  const b64 = fs.readFileSync(path.join(SHOTS, `${name}.png`)).toString("base64");
  return `data:image/png;base64,${b64}`;
};

// proto vs mine image pair
const cmp = (protoName, mineName, protoCap, mineCap) => `
  <div class="cmp">
    <figure class="proto"><img loading="lazy" src="${img(protoName)}" alt="${protoCap}"><figcaption><span class="b">原型</span>${protoCap}</figcaption></figure>
    <figure class="mine"><img loading="lazy" src="${img(mineName)}" alt="${mineCap}"><figcaption><span class="b">实现</span>${mineCap}</figcaption></figure>
  </div>`;

const html = `<!doctype html>
<html lang="zh"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Cerul · 实现 vs 设计原型 · 差异核对（含截图）</title>
<style>
  :root{--ink:#1c2229;--ink2:#525d6b;--ink3:#8a93a0;--card:#fff;--hair:#e6e9ee;--steel:#3e6b9d;--steel-soft:#eef4fb;--steel-line:#d4e0ee;--success:#2e7d56;--success-soft:#e7f4ed;--copper:#a06f39;--copper-soft:#f3ebe0;--flag:#c8553d;--flag-soft:#fbeae5;--card-sh:0 1px 2px rgba(28,40,60,.05),0 14px 30px -12px rgba(28,40,60,.22);--mono:ui-monospace,"SF Mono","JetBrains Mono",Menlo,Consolas,monospace;--sans:-apple-system,system-ui,"PingFang SC","Hiragino Sans GB","Segoe UI",Roboto,sans-serif;}
  *{box-sizing:border-box;} body{margin:0;font-family:var(--sans);color:var(--ink);line-height:1.55;background:radial-gradient(680px 360px at 50% 0%,#fff,rgba(255,255,255,0) 66%),linear-gradient(180deg,#fafbfc,#eef1f4);padding:40px 22px 80px;}
  .wrap{max-width:1040px;margin:0 auto;}
  .eyebrow{font-family:var(--mono);font-size:11px;letter-spacing:.16em;text-transform:uppercase;color:var(--steel);font-weight:600;}
  h1{font-size:30px;letter-spacing:-.02em;margin:8px 0 6px;} .sub{color:var(--ink2);font-size:14.5px;max-width:74ch;} .meta{font-family:var(--mono);font-size:11.5px;color:var(--ink3);margin-top:10px;}
  .scoreboard{display:flex;gap:12px;flex-wrap:wrap;margin:22px 0 8px;}
  .stat{flex:1;min-width:150px;background:var(--card);border:1px solid rgba(255,255,255,.85);box-shadow:var(--card-sh);border-radius:12px;padding:14px 16px;}
  .stat .n{font-size:26px;font-weight:700;font-family:var(--mono);letter-spacing:-.02em;} .stat .l{font-size:12px;color:var(--ink3);margin-top:2px;}
  .stat.green .n{color:var(--success);} .stat.amber .n{color:var(--copper);} .stat.gray .n{color:var(--ink3);}
  .legend{display:flex;gap:14px;flex-wrap:wrap;margin:14px 0 30px;font-size:12.5px;color:var(--ink2);}
  .tag{display:inline-flex;align-items:center;gap:6px;font-family:var(--mono);font-size:10.5px;font-weight:600;padding:2px 9px;border-radius:999px;white-space:nowrap;}
  .tag.match{color:var(--success);background:var(--success-soft);} .tag.gap{color:var(--copper);background:var(--copper-soft);} .tag.intent{color:var(--steel);background:var(--steel-soft);} .tag.todo{color:var(--flag);background:var(--flag-soft);}
  section{margin:34px 0;} h2{font-size:21px;letter-spacing:-.01em;margin:0 0 4px;display:flex;align-items:center;gap:10px;flex-wrap:wrap;}
  h2 .pill{font-family:var(--mono);font-size:11px;font-weight:600;color:var(--steel);background:var(--steel-soft);border:1px solid var(--steel-line);padding:2px 9px;border-radius:999px;}
  .sec-note{color:var(--ink3);font-size:13px;margin:0 0 14px;}
  .cmp{display:grid;grid-template-columns:1fr 1fr;gap:14px;margin:14px 0 6px;}
  .cmp figure{margin:0;background:var(--card);border:1px solid rgba(255,255,255,.85);box-shadow:var(--card-sh);border-radius:12px;overflow:hidden;}
  .cmp figure img{display:block;width:100%;height:auto;background:#eef1f4;}
  .cmp figcaption{padding:9px 13px;font-size:12.5px;font-weight:600;display:flex;align-items:center;gap:8px;border-top:1px solid var(--hair);}
  .cmp .proto figcaption{color:var(--ink2);} .cmp .mine figcaption{color:var(--steel);}
  .cmp figcaption .b{font-family:var(--mono);font-size:10px;font-weight:700;padding:1px 7px;border-radius:999px;}
  .cmp .proto .b{background:#eef1f5;color:var(--ink3);} .cmp .mine .b{background:var(--steel-soft);color:var(--steel);}
  .cap{font-size:12px;color:var(--ink3);margin:2px 2px 18px;}
  table{width:100%;border-collapse:separate;border-spacing:0;background:var(--card);border:1px solid rgba(255,255,255,.85);box-shadow:var(--card-sh);border-radius:12px;overflow:hidden;font-size:13px;margin-top:6px;}
  th,td{text-align:left;padding:11px 14px;vertical-align:top;border-bottom:1px solid var(--hair);} th{font-size:11px;text-transform:uppercase;letter-spacing:.06em;color:var(--ink3);font-weight:600;background:#fbfcfd;} tr:last-child td{border-bottom:0;}
  td.proto{color:var(--ink2);} td.mine{color:var(--ink);} td .lbl{display:block;font-weight:600;margin-bottom:2px;}
  code{font-family:var(--mono);font-size:11.5px;background:var(--steel-soft);color:var(--steel);padding:1px 5px;border-radius:4px;} .colsev{white-space:nowrap;width:1%;}
  .matchbox{background:var(--success-soft);border:1px solid #c8e6d6;border-radius:10px;padding:12px 15px;font-size:13px;color:#23613f;margin:14px 0 0;} .matchbox b{color:#1c4d31;}
  footer{margin-top:40px;padding-top:18px;border-top:1px solid var(--hair);font-size:12px;color:var(--ink3);}
  @media(max-width:760px){.cmp{grid-template-columns:1fr;}}
</style></head><body><div class="wrap">

<header>
  <div class="eyebrow">● 设计核对 · DESIGN FIDELITY DIFF</div>
  <h1>实现 vs 设计原型 · 差异清单（含截图）</h1>
  <p class="sub">分支 <code>redesign/luminous-handoff</code>（PR #43）逐屏对照 <code>design_handoff_cerul/Cerul AFTER.dc.html</code>。左＝设计原型（support.js 实时渲染），右＝我的实现（fixture 真渲染）。两侧均由 Electron 在 1320px 宽窗口同条件截取。</p>
  <p class="meta">2026-06-14 · 截图：design/diff-shots/*.png</p>
</header>

<div class="scoreboard">
  <div class="stat green"><div class="n">2 / 5</div><div class="l">基本还原（背景、空状态）</div></div>
  <div class="stat amber"><div class="n">11</div><div class="l">保真差距（建议修）</div></div>
  <div class="stat"><div class="n">5</div><div class="l">刻意为之（保功能/数据）</div></div>
  <div class="stat gray"><div class="n">1</div><div class="l">未实现（DMG）</div></div>
</div>
<div class="legend">
  <span class="tag match">✓ MATCH 已还原</span><span class="tag gap">▲ GAP 保真差距·可修</span><span class="tag intent">◆ INTENT 刻意·保功能</span><span class="tag todo">✕ TODO 未实现</span>
</div>

<section>
  <h2><span class="pill">§1</span> 全局背景 <span class="tag match">基本还原</span></h2>
  <p class="sec-note">这屏对得最齐——可在下面任意截图的「背景＋侧栏」直接对比：顶光渐变、侧栏与主区连成一片、白卡浮起。</p>
  <div class="matchbox"><b>已还原：</b> Luminous 顶光渐变 / Graphite（暗）；侧栏透明连续（发丝线 <code>rgba(28,40,60,.06)</code>）；<code>--card-sh / --win-sh</code>、<code>--steel-line #d4e0ee</code>、白卡高光描边；选中导航＝钢蓝底；状态点呼吸。</div>
</section>

<section>
  <h2><span class="pill">§2</span> 开屏引导（3 步）</h2>
  <p class="sec-note">骨架（滑动药丸、Logo 方块+光晕+扫光、几何插画、芯片、$0.00）已还原；差异是「我把原型的极简两按钮换成了完整功能控件」。</p>
  ${cmp("proto-onboarding-0", "impl-onboarding-0", "步骤 0 · 欢迎", "步骤 0 · 欢迎（多了「全局搜索权限」提示框）")}
  <p class="cap">▲ 欢迎页：我在快捷键卡下面多了 macOS 辅助功能权限提示（真功能需要）；标题文案也略有不同。</p>
  ${cmp("proto-onboarding-1", "impl-onboarding-1", "步骤 1 · 添加来源（两个按钮，干净）", "步骤 1 · 添加来源（完整功能控件）")}
  <p class="cap">◆ 最大的不同：原型是「选择文件夹… / 关注 YouTube 频道」两个按钮；我保留了完整的文件夹选择＋扫描常用位置＋YouTube URL 输入/校验/预览——明显更密，但能真用。</p>
  <table>
    <tr><th>位置</th><th>设计原型</th><th>我的实现</th><th class="colsev">类别</th></tr>
    <tr><td><span class="lbl">步骤 1 控件</span></td><td class="proto">两个按钮，干净</td><td class="mine">完整文件夹/YouTube 功能控件，更密</td><td><span class="tag intent">◆</span></td></tr>
    <tr><td><span class="lbl">欢迎页权限框</span></td><td class="proto">无</td><td class="mine">多了「全局搜索权限·打开系统设置」</td><td><span class="tag intent">◆</span></td></tr>
    <tr><td><span class="lbl">步骤 2 说明栈</span></td><td class="proto">仅 3 芯片 + $0.00 行</td><td class="mine">芯片下多了三行模型说明栈</td><td><span class="tag intent">◆</span></td></tr>
    <tr><td><span class="lbl">欢迎标题文案</span></td><td class="proto">搜索你视频里说过、展示过、讨论过的一切。</td><td class="mine">搜索你的视频中说过、展示过或讨论过的内容。</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">「稍后再说」</span></td><td class="proto">纯文字</td><td class="mine">带了快进图标</td><td><span class="tag gap">▲</span></td></tr>
  </table>
</section>

<section>
  <h2><span class="pill">§3</span> 空状态 <span class="tag match">基本还原</span></h2>
  <p class="sec-note">拖拽区、示例芯片、结果预览卡都还原了——下面成对看。</p>
  ${cmp("proto-empty", "impl-empty", "空状态（拖拽区 + 示例芯片 + 预览 ghost）", "空状态（同款）")}
  <table>
    <tr><th>位置</th><th>设计原型</th><th>我的实现</th><th class="colsev">类别</th></tr>
    <tr><td><span class="lbl">示例芯片</span></td><td class="proto">芯片带放大镜图标，上方有「索引完，这些都能立刻搜到」小标</td><td class="mine">芯片无图标，无小标</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">其余</span></td><td class="proto" colspan="2">眉标、标题、$0.00 正文、拖拽区（folder tile + 选择文件夹…/关注YouTube）、ghost 预览——<b class="" style="color:var(--success)">已还原</b></td><td><span class="tag match">✓</span></td></tr>
  </table>
</section>

<section>
  <h2><span class="pill">§4</span> 资料库 · 索引进行中</h2>
  <p class="sec-note">横幅改钢蓝、$0.00·本机 绿色、闪光处理卡、抽屉绿色费用面板都对了；保真差距最多。注意：原型这屏<b>默认开着任务抽屉</b>（右侧），所以右侧的抽屉对比也在这张图里。</p>
  ${cmp("proto-library", "impl-library", "资料库（原型默认开抽屉）", "资料库（我的，抽屉单独开）")}
  <p class="cap">对比要点 ↑：① 副标题原型有「共 5 项·1 项处理中」计数，我没有；② 原型标题栏有「● 任务·1 进行中」按钮，我用左栏图标；③ 横幅标题原型把条目名写在标题里（正在本机索引·API-first…），我拆成了两行；④ 卡片状态标签原型是铜/绿+mono，我是琥珀 sans；⑤ 卡片网格原型固定 2 列，我是响应式 auto-fill（窄＝2 列，宽会变 3 列）。</p>
  ${cmp("proto-tasks", "impl-tasks", "任务抽屉（原型，316px 浮层）", "任务抽屉（我的，400px + 全屏遮罩）")}
  <p class="cap">抽屉对比 ↑：绿色 $0.00 费用面板、进行中任务、失败任务错误井都还原了；差异是<b>形态</b>——原型是 316px 贴右浮层、内容仍占满；我用的是现有 400px 抽屉 + 全屏深色遮罩。（原型点了「任务」是 toggle，这张把默认开着的抽屉关上了，故只见资料库。）</p>
  <table>
    <tr><th>位置</th><th>设计原型</th><th>我的实现</th><th class="colsev">类别</th></tr>
    <tr><td><span class="lbl">卡片网格</span></td><td class="proto">固定 <code>repeat(2,1fr)</code></td><td class="mine">响应式 auto-fill（宽屏变 3 列）</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">副标题计数</span></td><td class="proto">…· 共 5 项 · 1 项处理中</td><td class="mine">无计数</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">任务开关</span></td><td class="proto">标题栏「● 任务·1 进行中」</td><td class="mine">左栏图标 + 角标</td><td><span class="tag intent">◆</span></td></tr>
    <tr><td><span class="lbl">横幅标题</span></td><td class="proto">正在本机索引 · API-first…（内联）</td><td class="mine">正在本机索引 1 个项目 + 另起 meta 行</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">横幅图标</span></td><td class="proto">钢蓝圆环 + 中心小方块</td><td class="mine">Loader2 旋转图标</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">处理卡角标</span></td><td class="proto">转写中</td><td class="mine">索引中</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">处理卡副文案</span></td><td class="proto">Talks 2026 · 正在本机处理</td><td class="mine">Talks 2026 · 已索引 昨天（数据/文案不对）</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">状态标签</span></td><td class="proto">铜「仅语音可搜」/ 绿「全文可搜」· mono</td><td class="mine">琥珀「仅语音可搜」· sans · 标签体系不同</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">卡片浮起</span></td><td class="proto">card-sh + 白描边，明显浮起</td><td class="mine">更克制（细边框）</td><td><span class="tag gap">▲</span></td></tr>
    <tr><td><span class="lbl">抽屉形态</span></td><td class="proto">316px 贴右浮层，无全屏遮罩</td><td class="mine">400px + 全屏深色遮罩</td><td><span class="tag intent">◆</span></td></tr>
  </table>
  <div class="matchbox"><b>已还原：</b> 横幅琥珀→钢蓝 + 同步 <code>%</code> + 绿色 <code>$0.00·本机</code>；处理卡闪光+旋转环+处理角标；抽屉绿色 <code>$0.00</code> 费用面板；失败任务错误井 + 查看来源/技术详情。</div>
</section>

<section>
  <h2><span class="pill">§5</span> DMG 安装包 <span class="tag todo">未实现</span></h2>
  <p class="sec-note">原型 §5（Finder 拖拽安装窗 + Gatekeeper 提示）没做——它是打包资源（electron-builder <code>dmg.background</code> 背景图，在 <code>apps/electron-shell/package.json</code> 的 build.dmg），不是渲染层 UI，单独跟进。</p>
</section>

<footer><b>读法：</b>「◆ 刻意」是为保留真实功能做的取舍；「▲ 保真差距」可对齐原型；「✕」只剩 DMG。想修哪几条 ▲（如卡片 2 列、角标改「转写中」、副标题加计数、状态标签改铜/绿+mono、横幅图标改环+方块），点名即可，我直接改并 push 到 PR #43。</footer>
</div></body></html>`;

fs.writeFileSync(OUT, html);
const kb = Math.round(fs.statSync(OUT).size / 1024);
console.log(`wrote ${OUT} (${kb} KB)`);
