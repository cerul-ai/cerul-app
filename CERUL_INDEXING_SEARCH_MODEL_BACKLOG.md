# Cerul 索引、搜索、本地模型可靠性功能清单

整理时间：2026-06-18

本文档把近期对话和本机排查合并成一个可执行 backlog。目标不是单点修 UI，而是把用户现在遇到的几类核心问题一起收口：本地文件定位误判、语义搜索不可信、模型下载状态误导、索引阶段卡住、批量删除体验不完整。

## 当前已确认的问题

1. 已索引本地视频会被误判为“源文件找不到”，即使文件实际还在原路径。
2. 用户重新定位同一个文件后，会被重新排进索引队列，导致重复处理。
3. 搜索看起来像文本匹配，不像语义向量检索；用户无法知道当前用了 vector、hybrid 还是 FTS fallback。
4. 模型准备状态会把本地模型误报为 ready，实际转写时仍会补下载 ASR / ForcedAligner 权重。
5. “转写中”会隐藏真实的模型下载，用户看到 20 分钟卡住，但实际是在 Hugging Face 慢速下载 ForcedAligner。
6. PR #60 的模型下载 UI 改动方向正确，但还有 review blocker：本地模型 ready 判断太松、HF 下载完成后来源/速度可能丢、ModelScope 仍用 `master`。
7. 删除 UI 的修复是半截状态：PR #60 里有部分全选、spinner、热区改动，但 `cards.tsx` 和 `detail-issue-panel.tsx` 的关键修复仍在未提交工作区。
8. “删除后很久看不到数据消失”目前主要靠前端乐观移除改善，没有证明后端删除链路已被修好。
9. 多视频索引仍是用户感知上的串行等待：封面、元数据、转写、embedding 没有很好地拆成可并行阶段。

## P0：先修正确性和误导性状态

### 1. 修复本地文件定位路径写入

状态：已完成，本地验证通过。`PATCH /items/:id` 已改用 `set_item_raw_path` 同步 `items.raw_path` 和 `metadata.raw_path`，并新增 `raw_path_exists` 让前端可判断路径是否真实存在。

问题：定位文件时，API PATCH 只更新了部分字段，容易导致 `items.raw_path` 和 `metadata.raw_path` 不一致。

需要做：

- `PATCH /items/:id/raw-path` 或等价 API 必须调用统一的 `set_item_raw_path`。
- 同步更新 `items.raw_path` 和 `metadata.raw_path`。
- 更新后立即重新读取 item，确保 UI 拿到的是真实路径。
- 对路径做存在性校验，但不要把所有处理失败都归类成“文件找不到”。

验收：

- 文件还在原路径时，不出现“找不到该视频的文件”。
- 用户定位到同一个文件路径后，数据库两处 raw path 保持一致。
- 重启 App 后仍能打开原文件。

### 2. 修复错误分类

状态：已完成，本地验证通过。前端现在只有在错误明确像源文件缺失且 `raw_path_exists=false` 时才显示“找不到文件”；模型/ffmpeg/embedding 等处理错误不会再因为有本地 raw path 被误归类成 missing-file。

问题：本地处理失败被过度归类成“source file missing”，用户被错误引导去重新定位文件。

需要做：

- 只有当错误文本明确包含 `source file does not exist` / `file not found` 等源文件缺失信号，并且当前 raw path 确实不存在时，才展示“文件找不到”。
- ffmpeg、ASR、OCR、embedding、Qdrant、模型缺失、权限错误要显示真实类别。
- UI 面板保留“技术详情”，便于复制诊断。
- 日志里记录 `raw_path_exists=true/false`、错误分类、原始错误。

验收：

- 文件存在但模型缺失时，显示“本地模型未准备好”或“本地处理失败”，不显示“文件挪动了”。
- 文件真的不存在时，仍显示定位文件入口。

### 3. 修复定位后的重索引策略

状态：已完成，本地验证通过。定位文件只 PATCH 路径并刷新数据，不再自动调用 reindex；需要重新处理时必须由用户显式点击“重新索引”。

问题：用户定位到同一个文件夹/同一路径后，即使视频已索引且文件存在，也会重新排队。

需要做：

- 如果新路径等于旧路径、文件存在、item 已索引，不自动重索引。
- 如果只是 raw path 修复且 chunks / embeddings 完整，只更新路径状态。
- 只有文件 hash / size / modified time 变化，或用户明确点击“重新索引”时才重索引。
- 对“定位文件夹”和“定位具体文件”分别处理：文件夹定位应匹配原文件名或 external_id，不能盲目重建索引。

验收：

- 已索引视频重新定位到同一路径后，不进入任务队列。
- 真正换了文件时，会提示需要重索引。

### 4. 搜索结果和 API 增加检索模式诊断

状态：已完成并推送到 PR #60，commit `860ea69`。`POST /search` 返回 `results + diagnostics`，包含 `retrieval_mode`、fallback reason、vector/FTS 命中数、active embedding profile、Qdrant collection 和 point count hint；前端结果页显示本次检索模式 debug 文案；新增 `GET /search/diagnostics` 返回 item/chunk/Qdrant point 健康计数。

问题：用户无法判断搜索是语义向量、hybrid 还是文本 fallback；图 2/图 3 表现更像 FTS 文本匹配或向量索引缺失。

需要做：

- 搜索 API 返回诊断字段：
  - `retrieval_mode`: `hybrid` / `vector` / `fts` / `fts_fallback` / `empty`
  - `fallback_reason`: 例如 `embedding_unavailable`、`qdrant_empty`、`query_embedding_failed`
  - `vector_hits_count`
  - `fts_hits_count`
  - `embedding_profile_id`
  - `qdrant_collection`
- UI debug 区或复制诊断中显示本次检索模式。
- 日志记录 query embedding 是否成功、Qdrant 查询耗时、FTS fallback 原因。
- 增加健康检查：当前库 item/chunk 数、已 embedding chunk 数、Qdrant point count 是否一致。

验收：

- 搜索“张亮麻辣烫股东都有谁”这类非逐字匹配问题时，可以明确看到是否触发了 vector/hybrid。
- 如果向量索引没建好，UI 或诊断能说出原因，而不是只显示“没有匹配项”。

### 5. 修本地模型 ready 判断

状态：已完成并推送到 PR #60，commit `087b842`。`local-mlx` catalog 安装态已改为 group-level readiness，并补了临时下载文件不计入 ready、多 repo 必须齐全的单测。

问题：`models.rs` 目前只检查单个 `spec.source` 是否有超过 64MB 缓存；ASR 实际需要 ASR + ForcedAligner 两个 repo，半截下载也可能超过 64MB。

需要做：

- 本地模型安装状态改成 group-level readiness。
- ASR ready 必须同时满足：
  - ASR repo snapshot 完整
  - ForcedAligner repo snapshot 完整
  - 必需文件存在
  - 有实际权重文件
  - 没有 `.incomplete`/未完成锁导致的假阳性
- embedding 和 OCR 也使用同一套 snapshot 完整性检查。
- `blocked_reason` 明确区分 runtime 不可用、权重未下载、权重不完整。

验收：

- 半截下载不再显示“已就绪”。
- ASR 缺 ForcedAligner 时，模型页显示 ASR 未完成，而不是 ready。

### 6. 修 Qwen ASR 隐式 Hugging Face 下载

状态：已完成并推送到 PR #60。Qwen ASR / ForcedAligner 已先通过 `resolve_snapshot()` 解析为本地 snapshot 路径，再交给 `mlx_qwen3_asr`；pipeline 现在会在转写前进入 `preparing_models` stage 并调用 sidecar `prepare_transcription` 预解析 ASR/ForcedAligner snapshot，Whisper 路径也会先解析为本地 snapshot，避免在 `transcribing` 中隐式走 Hugging Face 下载。

问题：转写路径直接把 `Qwen/Qwen3-ASR-0.6B` 和 `Qwen/Qwen3-ForcedAligner-0.6B` repo id 传给 `mlx_qwen3_asr`，该库会自己走 Hugging Face 下载，绕过 Cerul 的 ModelScope/CDN/cache 路由。

已经开始修：

- `mlx-sidecar/cerul_mlx_sidecar.py` 已加 `_resolved_qwen_asr_path()`。
- Qwen ASR 和 ForcedAligner 会先通过 `resolve_snapshot()` 解析成本地路径，再交给 `mlx_qwen3_asr`。

还需要做：

- 给这个行为补测试或 smoke script。
- 在 job 开始转写前做 ASR/ForcedAligner preflight。
- 如果权重缺失，job stage 应进入 `preparing_models`，而不是 `transcribing`。
- 模型准备阶段要显示来源、速度、进度。

验收：

- 本地已有 ModelScope snapshot 时，转写不再创建 Hugging Face `.incomplete` 文件。
- 69 秒视频不会在“转写中”卡 20 分钟下载模型。

### 7. ModelScope 必须使用 pinned revision

状态：已完成并推送到 PR #60，commit `087b842`。ModelScope snapshot 目录、repo file API、resolve URL、probe URL 全部传入默认模型 pinned revision，并加了无网络 resolver smoke。

问题：ModelScope resolver 仍默认 `master`，会绕过默认模型的 pinned revision 保证。

需要做：

- `modelscope_resolve_url()` 接受并使用 pinned revision。
- `modelscope_repo_files()` 不写死 `revision=master`。
- `modelscope_snapshot_dir()` 不写死 `snapshots/master`，默认模型使用 `PINNED_MODEL_REVISIONS`。
- 已有 `master` cache 可迁移或重新校验，但不能继续作为默认 trusted snapshot。

验收：

- 默认 ASR、ForcedAligner、embedding、OCR 都从固定 revision 解析。
- ModelScope 选中时不再下载 moving branch。

### 8. Hugging Face 下载完成后保留来源和速度

状态：已完成并推送到 PR #60，commit `087b842`。HF `snapshot_download()` fallback 会写 sticky `last_source=Hugging Face`，下载超过 1 秒时记录基于 snapshot 大小和耗时估算的平均速度。

问题：PR #60 的 sticky diagnostics 只在自定义 URL downloader 里更新；HF `snapshot_download()` 路径不会写 `last_source` / `last_download_bps`。

需要做：

- 给 HF snapshot path 增加下载来源记录。
- 若无法拿到细粒度速度，至少记录 `last_source=hf` 和总耗时/平均速度。
- 完成/失败都写入 `prepare-status.json`。
- 设置页和复制诊断不能承诺不存在的数据。

验收：

- HF 下载完成后，设置页能显示“上次来源 Hugging Face”。
- 如果速度不可用，UI 不显示假的峰值速度。

## P1：用户体验和索引性能

### 9. 删除 UI 修复收口

状态：已完成并推送到 PR #60，commit `3cccde0`。`components/cards.tsx` 已改用完整 `.item-select` 热区，避免被旧 `.sel-check` 缩成 20px 小框；`detail-issue-panel.tsx` 的定位/重索引/删除 loading icon 已补 `spin` class。批量删除当前已有乐观移除，失败后会刷新后端真实状态并显示错误。

现状：

- PR #60 已做：全选按钮、部分 spinner、部分热区 CSS、批量删除乐观移除。
- 未提交工作区仍有：`components/cards.tsx` 的 select label 修复、`detail-issue-panel.tsx` 的 spinner class 修复。

需要做：

- 决定删除 UI 修复是补进 PR #60，还是单独开小 PR。
- 保证网格和列表模式选择框都易点。
- 缺失文件面板里的删除/重新索引 icon 必须真正转动。
- 删除失败时，前端乐观移除要能回滚或刷新出真实状态。
- 后端删除 API 需要暴露具体错误，不要静默失败。

验收：

- 批量删除时，选中项立即从列表消失，失败时能提示并恢复/刷新。
- 缺失文件面板删除时，Loader 动画可见。
- 全选只作用于当前过滤结果，并支持取消全选。

### 10. 模型下载 UI 不展示 OCR 过程

状态：已完成，本地验证通过。默认本地模型准备只包含 embed/asr，`local_capability.total_mb` 和 `prepare-status` overall 进度也只按用户可管理模型计算；OCR 仍保留在 `models` 诊断列表里，但不会进入默认下载步骤。

现状：首启下载弹窗已经隐藏 `ocr`，但其他设置页/诊断仍要确认边界。

需要做：

- OCR 作为内置 bundled dependency，不作为用户需要下载/管理的模型项展示。
- 进度总量排除 OCR，除非它真的需要联网修复。
- 如果 OCR bundled 损坏，显示“修复内置模型”，而不是“下载画面文字模型”。

验收：

- 用户首次下载模型时，不看到“画面文字 · PP-OCRv6”。
- 复制诊断可以包含 OCR ready 状态，但不作为用户步骤展示。

### 11. 模型下载路由改成点击后实测

状态：已完成并推送到 PR #60。`auto` 模式已经在用户点击下载时并行 probe Cerul CDN / Hugging Face / ModelScope，然后按实测吞吐选择来源；`CERUL_MODEL_DOWNLOAD_REGION` 只作为 tie-break 和全失败 fallback。新增无网络 smoke 覆盖 ModelScope 最快会被选中、国内 region 不会压过实测速度、ModelScope 探测失败会进入诊断。

问题：靠 locale/timezone 判断国内用户不可靠；显式 `CERUL_MODEL_DOWNLOAD_REGION` 也不适合普通用户理解。

需要做：

- 用户点击下载时，对 CDN、Hugging Face、ModelScope 做短时间并行测速。
- 选最快的可用源，region/env 只作为 tie-break 或测试 override。
- 持久化测速结果和最终选择。
- UI 提供“为什么选这个源？”展开信息。
- 国内用户没有触发 ModelScope 时，要能从诊断看到是测速输了、探测失败、还是候选源没加入。

验收：

- 国内网络下，如果 ModelScope 最快，会自动选 ModelScope。
- 如果没选 ModelScope，诊断能解释原因。

### 12. 索引阶段拆分和并行

问题：用户看到多个视频排队，但一个视频卡在 step 2/5，其他视频没有明显进展。

需要做：

- 把索引拆成资源类型不同的阶段：
  - metadata/probe
  - thumbnail extraction
  - audio extraction
  - ASR transcription
  - frame/OCR/visual chunks
  - embedding
  - search index commit
- 非模型阶段允许多视频并发。
- 模型阶段按资源设并发限制：
  - ASR/ForcedAligner 通常单并发
  - embedding 可小并发或批处理
  - ffmpeg/thumbnail 可多并发
- 任务队列要显示“为什么排队”：等待 ASR、等待 embedding、等待模型下载、等待文件 IO。

验收：

- 多个视频导入后，封面和 metadata 很快出现。
- 一个视频转写时，其他视频可以先完成封面/metadata。
- UI 不再只显示“排队中”，而是显示等待资源。

### 13. 先生成封面和基础信息

问题：用户看到灰卡和 spinner 很焦虑，即使视频存在且可读取。

需要做：

- 导入后立即 probe duration、has_audio、size、mtime。
- 尽早抽一张封面图并写入 `thumbnail_chunk_id` 或专用 thumbnail 字段。
- 封面生成不依赖完整索引。
- 对无音频/无法转写视频，也能先显示封面和基础元数据。

验收：

- 添加视频后几秒内看到封面，不等 ASR/embedding。
- 队列中视频也有封面和 duration。

### 14. ETA 和进度改成可信状态

问题：当前 ETA 经常不准；step 2 卡很久时用户无法判断是在下载、模型加载、转写还是死锁。

需要做：

- 每个 stage 单独记录 started_at、last_heartbeat_at、bytes_done、items_done。
- 没有真实进度时，不显示精确 ETA；显示“正在加载模型 / 正在下载模型 / 正在转写”。
- 若超过阈值无进展，显示“可能卡住”并提供复制诊断。
- 模型下载进度不能混进转写进度。
- 进度条可以分段：准备模型、转写、切块、向量化、提交索引。

验收：

- step 2/5 不再靠 elapsed-time easing 假装推进。
- 用户能看到真实瓶颈：下载模型、模型加载、ASR 计算、网络、IO。

## P2：维护、修复和诊断工具

### 15. 本地模型修复/清理工具

需要做：

- 设置页增加“修复本地模型”或诊断入口。
- 清理 `.incomplete` 文件和孤立 lock 前要确认没有 active downloader。
- 支持重新校验 pinned snapshots。
- 删除模型 cache 时，错误必须返回给 UI，不能只 warn。

验收：

- 半截下载后，用户可以一键修复，不需要手动删 Application Support。

### 16. 索引和搜索健康检查

需要做：

- 增加健康检查命令/API：
  - items count
  - chunks count
  - transcript chunks count
  - embedding chunks count
  - Qdrant points count
  - FTS rows count
  - orphan jobs count
  - missing raw paths count
- 提供“重建搜索索引”动作，只重建缺失向量/FTS，不重新转写视频。

验收：

- 搜索异常时，可以判断是 embedding 没建、Qdrant 空、还是 query fallback。

### 17. 诊断包

需要做：

- 一键复制诊断包含：
  - app version
  - processing mode / inference mode
  - active model ids
  - model readiness per repo
  - current jobs/stages
  - retrieval mode for last search
  - raw path exists flags
  - recent errors
- 不包含 API keys、完整私密路径可选择脱敏。

验收：

- 用户发一份诊断即可判断卡在哪里，不再靠截图猜。

## 建议拆 PR 顺序

### PR A：本地文件定位和错误分类

包含：

- raw path 写入统一化
- 错误分类修正
- 定位后不盲目重索引
- 对应测试

为什么先做：这是数据正确性问题，会直接误导用户删除或重复索引。

### PR B：搜索诊断和向量健康检查

包含：

- 搜索 API retrieval diagnostics
- UI/debug/复制诊断展示
- Qdrant point count / embedding 状态检查
- 重建搜索索引入口的设计或第一版 API

为什么第二：用户现在无法判断“语义搜索到底有没有生效”。

### PR C：本地模型 ready 和 ASR 隐式下载修复

包含：

- group-level model readiness
- Qwen ASR 使用 Cerul resolved snapshot path
- ModelScope pinned revision
- HF sticky diagnostics
- 转写前模型 preflight

为什么第三：这是当前“索引慢”的直接根因。

### PR D：删除 UI 和删除链路收口

包含：

- 选框热区、全选、spinner 完整提交
- 后端删除错误透出
- 前端乐观删除 + 失败回滚/刷新
- 删除中的任务取消/隐藏策略确认

为什么单独拆：这和模型下载 PR #60 scope 混在一起会影响 review。

### PR E：索引体验和并行 pipeline

包含：

- 早期封面/metadata
- 分阶段并发
- 可信进度/ETA
- stage heartbeat 和 stall 诊断

为什么最后：收益大，但改动面也最大，需要在前面正确性问题稳定后做。

## 不建议继续做的事

- 不要用 locale/timezone 自动判断下载区域作为主逻辑。
- 不要把 OCR 作为普通下载步骤展示给用户。
- 不要用“缓存超过 64MB”判断模型 ready。
- 不要在“转写中”阶段静默下载模型。
- 不要把所有本地处理失败都显示成“文件找不到”。
- 不要只靠前端乐观移除宣称删除问题已解决。

## 当前工作区注意事项

- PR #60 当前不建议直接合并，应先处理 review blocker。
- 本地工作区已有未提交改动，包含删除 UI、索引 pipeline、storage chunks、sidecar ASR 路径修复等，拆 PR 时要先分清归属。
- 当前安装在 `/Applications/Cerul.app` 的版本不会自动使用源码里的 sidecar 修复，需要重新 build/package 或运行开发版验证。
