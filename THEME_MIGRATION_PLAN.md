# Cerul App 主题迁移实施计划（design-audit 分支）

> 2026-07-10。依据 cerul-brand `I_应用主题/`（commit 7cdbdfd）的定稿：**舰桥导航 + 甲"呼吸"搜索框 + 引文卡工作台播放器；亮=暖玻璃（默认）/ 暗=冷石墨**。
> 本文档是执行前的施工图。**先按此文档逐条确认，确认后才动代码。**

---

## 0. 目标与非目标

**目标**：把桌面 App 所有页面的视觉风格迁移到定稿主题，含两处结构性改动（导航 rail→舰桥、播放器详情页重排），本地跑起来给 owner 验收。

**明确不动的东西**（红线）：
- **Logo / 图标**：继续用 `apps/desktop/public/brand/` 现有资产与 3-block mark，不重绘。
- **文案**：不改任何现有 i18n 文案与 slogan；mockup 里的示意文案（"下午好""别拖进度条了"等）**不进代码**。新组件必须新增的 UI 标签（如"引文卡""加入引用篮"）按现有 i18n 规范补 zh/en 两份 key，数量控制到最少。
- **API / 数据逻辑**：所有后端调用、路由、状态管理、快捷键系统、设置项含义不变。只动展示层。
- **功能范围**：不新增后端能力。计划中依赖后端的新交互（范围筛选、⌘↵ 综合回答、影院模式）见 §8 的降级策略。

**基线事实**（已核实）：
- 样式入口 `apps/desktop/src/styles.css`，@import 顺序：`tokens.css → ui.css → app.css → extensions.css → handoff.css → settings-redesign.css → home-redesign.css → overlay-redesign.css`（handoff.css 是"最后赢"的重皮层）。
- `App.tsx` 2247 行，rail 相关 className 37 处；播放器详情 `screens/item-detail.tsx`（1422 行）+ `app.css` 的 `.detail-split`（视频固定在 360–420px 左栏）。
- 主题切换已有 `[data-theme="light"/"dark"]` 机制，tokens.css 按主题覆写变量——**迁移可以只换值不换机制**。
- 验证链：`nvm use 20 && pnpm typecheck`、`pnpm smoke`、本地跑 `./run.sh`（Electron + vite 1420）。前端零测试，视觉验证靠手测清单 + 截图对照 cerul-brand `I_应用主题/previews/`。

---

## 1. 阶段总览（每阶段一个 commit，都在 design-audit 上）

| 阶段 | 内容 | 风险 | 预估体量 |
|---|---|---|---|
| P0 | 基线快照与对照组 | 无 | 小 |
| P1 | tokens.css 换血（暖玻璃/冷石墨） | 低 | 中 |
| P2 | 舰桥导航替换 rail | **高** | 大 |
| P3 | 搜索框"呼吸"态 | 中 | 中 |
| P4 | 播放器详情重排 + 引文卡 | **高** | 大 |
| P5 | 其余页面顺色与清理 | 中 | 中 |
| P6 | 双主题全量验收 + 死样式清理 | 低 | 中 |

P1 结束后全 app 已经"变色"，可先给 owner 看一眼色调；P2/P4 是两块硬骨头，互相独立、可并行也可先后。

---

## 2. P0 · 基线快照

**做什么**
1. 确认工作树干净、位于 `design-audit`；`git log` 记下起点 commit。
2. `nvm use 20 && pnpm typecheck && pnpm smoke`，记录基线是否绿（不绿则先报告，不带病施工）。
3. `./run.sh` 跑起来，用 9 个界面各截一张基线图存 `.artifacts/theme-migration/before/`（首页/搜索结果/资料库/来源/播放器详情/任务/设置/引导/⌥Space 呼出），作为迁移对照组。

**怎么测**：无功能改动，快照本身即产出。

---

## 3. P1 · tokens.css 换血（只换值，不换变量名）

**做什么**
1. 重写 `styles/tokens.css` 两个主题块的**取值**，变量名与结构保持不变（`--bg-app / --surface / --text / --accent / …`），这样 8 个 CSS 文件的几千处引用自动生效：
   - 亮色：`#eef1f4` 冷灰系 → 暖玻璃（bg 渐变基 `#F9F6F1→#EFEAE1`、surface `#FFFDF9`、文字 `#2B2622/#8A8177/#A2988C`、hairline 暖化）；铜 accent 值不动。
   - 暗色：冷石墨**基本不动**（现值即定稿），只把个别与新亮色不协调的边线/浮层微调。
2. 新增少量新变量（供 P2–P4 用）：`--bridge-bg / --bridge-line / --cite-bg / --cite-line / --focus-ring-glow` 等，集中放 tokens.css 末尾并注释来源（I_应用主题 README §三）。
3. `handoff.css` 顶部的 Luminous 渐变（`.app` 背景、radial 白光）改为暖玻璃渐变；`--card-sh` 阴影色从冷蓝 `rgba(28,40,60,…)` 调为暖 `rgba(96,74,52,…)`。
4. 排查硬编码冷灰 hex：`grep -n '#eef1f4\|#e9ebee\|#d5dae0\|rgba(28, *40, *60' styles/*.css`，逐个替换为变量或暖值。

**怎么测**
- `pnpm typecheck`（CSS 不影响，但流程统一跑）；`./run.sh` 起 app，亮/暗各过一遍 9 屏：无"漏网冷灰"块、文字对比度正常（正文 ≥4.5:1 抽查 muted 文字）、选中/hover/focus 三态可见。
- 与 `I_应用主题/previews/` 逐屏对色（色调一致即可，此阶段不要求布局一致）。

**回滚**：单 commit revert 即可。

---

## 4. P2 · 舰桥导航（rail → bridge）

**做什么**
1. 新建 `components/bridge.tsx` + `styles/bridge.css`（追加到 styles.css import 末尾）：暗色胶囊（两主题恒暗），布局 = mark 图标 → 页签（沿用现有导航项与 i18n label：搜索/资料库/来源——**页签集合以现 rail 为准，不按 mockup 增删**）→ 搜索框槽位（P3 填充，先放现有搜索入口）→ 任务入口 → 头像。
2. `App.tsx`：`<aside class="rail">` 区块替换为 `<Bridge/>`；主布局从"左右分栏"改为"上下结构"（`.app` grid 调整）。rail 的 37 处引用逐一迁移：
   - `rail-top`（mark）→ 舰桥左端；**不带 wordmark**（定稿）。
   - 导航项 → 舰桥页签（active 态 = 铜色 tint 胶囊）。
   - `rail-update`（更新提示/popover）→ 头像菜单一行 + 有更新时头像角标。
   - `rail-status`（CORE/索引状态）→ 头像菜单页脚状态条（mono）。
   - `rail-footer` 的 任务/设置/登录 → 任务留舰桥、设置与登录进头像菜单。
3. 新建头像菜单（244px 下拉）：个人资料 / 设置 ⌘, / 任务（计数）/ 主题切换 / 页脚状态条。**全部是现有功能的换位**（设置页、任务抽屉、主题 toggle、账号信息都已存在），只写壳不写新逻辑。
4. 旧 `.rail-*` CSS 先保留（P6 统一删），避免中途穿帮。

**怎么测**
- `pnpm typecheck` 绿；`./run.sh` 手测清单：四个导航项路由正常；任务抽屉能开；设置可进；更新流（用 `updaterState` 的 mock/dev 态）在头像菜单可见；⌥Space 呼出不受影响；窄窗（≤900px）舰桥不换行不溢出（页签优先、搜索框可压缩）。
- 暗/亮两主题下舰桥与背景层次正确（亮色下阴影、暗色下光边）。
- 对照 previews/light-01 与 dark-01 的舰桥形态。

**风险与对策**：App.tsx 改动面大 → 先把 rail 区块整体抽成组件再改样式，两步各自可编译；键盘焦点顺序（tab 序）要在手测清单里过。

---

## 5. P3 · 搜索框"呼吸"态

**做什么**
1. 现有搜索入口（home 搜索框 + results 顶部框）统一收进舰桥中段组件 `BridgeSearch`：静止态 = 提示文案（**复用现有 placeholder i18n key**）+ `⌥Space` 角标。
2. 聚焦态：舰桥容器加 `.is-tall`，高度动画 M1 240ms（`--dur` 已有）；长出的范围行 v1 只放**现有检索已支持的筛选**（查 `screens/results.tsx` 现有 filter 参数——如目前仅"全库"，则先只显示"全库/当前视频"两枚，当前视频仅在详情页出现）；`⌘↵ 综合回答` **本次不做**，不渲染该提示。
3. 搜索提交后跳转 results 的现有逻辑不变；esc 收起走现有 `use-dismissable`。

**怎么测**
- 手测：首页/资料库/详情页三处聚焦→长高→输入→回车出结果→esc 收起；`⌥Space` 全局呼出仍是独立 overlay，互不打架；`prefers-reduced-motion` 下无动画直接切换。
- 动画掉帧目测（长高只动 `grid-template-rows/max-height + opacity`，不动布局重排大项）。

---

## 6. P4 · 播放器详情重排 + 引文卡

**做什么**（`screens/item-detail.tsx` + `app.css .detail-split` 区）
1. 布局换骨：`.detail-split(360px | 1fr)` → `.pd-grid(1.5fr | 1fr)`：
   - 左上：视频主舞台（现有 player 组件原样搬入，章节分段进度条已有 `PlayerChapter`，样式按定稿调）；
   - 左下：**引文卡 `CitationCard`**（新组件）：深底石墨卡（两主题同深底），大字引文 + 铜 mono 署名（`— 人名 · 片名 · 编号 · 时间戳`；来源编号若数据层暂无 `A-xxxx`，v1 先用现有 item id 缩写，编号体系另立后端任务）+ 引用帧缩略图（用现有帧缩略图接口；没有则占位 E9 渐变）+ 动作：复制引用（复用现有 formatters 的引用文本逻辑）/ Markdown / 导出卡片（v1 = 复制为图片暂缓，可先隐藏）/ 加入引用篮；
   - 右栏：工作台卡（页签 转写/章节/要点）：转写 = 现 `transcript-reading` 内容改列表样式；章节 = 现章节列表；**要点 = 现 understanding 面板（摘要/话题）整体挪入**，不改其数据流。
2. 交互：监听转写区 `selectionchange`，选中文本落在某行时该行标记 `已选 → 引文卡` 并填充 CitationCard；未选中时 CitationCard 显示"当前播放句"。
3. 引用篮 v1：纯前端 state（React state + localStorage），底栏计数 +"导出全部"= 拼接引用文本复制。
4. 影院模式（⌘⇧F）**本次不做**，页头按钮先不渲染（配色未定稿）。
5. 新增 i18n key（预估 ≤10 个：引文卡/复制引用/加入引用篮/引用篮/导出全部/要点/已选提示等），zh/en 同步。

**怎么测**
- `pnpm typecheck`；手测：播放/暂停/seek 正常；点转写行跳时间戳（现有行为）不回归；选中文字生成卡、复制引用的文本格式正确（含时间戳）；引用篮加/清/导出；understanding 数据在"要点"页签正常加载（含加载中/失败态——原组件已有）；窄窗（<1024px）降级为上下单列（现 `.detail-split` 已有断点，沿用）。
- 对照 previews/light-03、dark-03。

**风险与对策**：item-detail 1422 行、状态多 → 只动 JSX 结构与 className，不动 hooks/数据函数；分两个子 commit（布局换骨 / 引文卡交互）。

---

## 7. P5 · 其余页面顺色与清理

逐页过（多为 CSS 级调整，复用 P1 token 后的自然效果，重点抓不协调处）：
- **home.tsx / home-redesign.css**：搜索入口移除（已进舰桥），最近入库卡片按暖玻璃卡样式；不加 mockup 的问候语。
- **results.tsx**：结果卡对齐定稿（缩略图+引文+铜时间 chip+动作行），"跳到这一秒/复制引用"用现有文案 key。
- **library.tsx / sources.tsx**：列表行/表格换暖玻璃 hairline 风格；状态 pill 沿用现有语义色规则（绿=成功保持现状不扩散）。
- **settings.tsx / settings-redesign.css**：这文件里有大量按旧色写死的效果（copper text-shadow 等），逐条对新 token 校色；分页侧栏结构不动。
- **onboarding.tsx**、**OverlayApp（⌥Space）/overlay-redesign.css**、**menubar**（shell 层 html/js）：只校色，不改结构；menubar 的色值是独立写死的，需要单独同步一份暖玻璃/冷石墨值。
- **moments.tsx / result-detail.tsx**：顺色。

**怎么测**：亮/暗 × 9 屏手测过一遍 + 空状态（E9/E11 规则不回归）+ 处理中状态（中性灰+动效，不出现琥珀）。

---

## 8. P6 · 全量验收、清理与交付

1. 删除死样式：旧 `.rail-*`、`.detail-split`（确认无引用后）、被覆盖失效的 handoff 段；`grep` 复核无孤儿 className。
2. 全量验证：`nvm use 20 && pnpm typecheck && pnpm smoke`；`cargo check` 不需要（未动 Rust）。
3. 双主题 × 9 屏 + 呼出 + menubar 截图存 `.artifacts/theme-migration/after/`，与 before/ 和 brand previews 三方对照。
4. `./run.sh` 保持运行，**交给 owner 实机验收**（本计划的最终交付动作）。
5. 验收通过后再谈 commit 整理/PR；不主动动 main、不发版（release 有 owner 门禁）。

---

## 9. 降级与开放问题（需要 owner 知情或后续立项）

| 事项 | 本次处理 | 后续 |
|---|---|---|
| 搜索范围筛选（人物/本周） | 只渲染后端已支持的维度 | 检索参数扩展后再放开 |
| ⌘↵ 综合回答 | 不做不渲染 | 问答链路立项后加 |
| 影院模式 ⌘⇧F | 不做（配色未定稿） | 定稿墨黑与否后单独做 |
| 来源编号 A-xxxx | 引文卡先用现有 id 缩写 | 编号体系（含引用格式）单独立项 |
| 导出卡片（图片） | 按钮暂藏，复制文本/Markdown 先行 | 分享卡渲染管线复用 OG 方案 |
| 引用篮持久化 | localStorage v1 | 后续入库 |

## 10. 执行纪律

- 每阶段开工前后各跑一次 typecheck；每阶段一个（或两个）commit，信息前缀 `theme:`。
- 任何一步发现要改文案/API 才能继续的，停下来先问，不擅自越线。
- 手测清单执行时同步记录到 `.artifacts/theme-migration/checklist.md`，验收时给 owner 看。
