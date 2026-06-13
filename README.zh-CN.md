<div align="center">
  <br />
  <a href="https://cerul.ai">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="apps/desktop/public/brand/icon-silver/cerul-icon-silver-256.png" />
      <img src="apps/desktop/public/brand/icon-graphite/cerul-icon-graphite-256.png" alt="Cerul" width="96" />
    </picture>
  </a>
  <h1>Cerul App</h1>
  <p><strong>把你看过、听过的一切，变成可搜索的本地记忆。</strong></p>
  <p>把它指向你的文件夹、YouTube 频道和播客订阅源。Cerul 会在<strong>本地</strong>监看、转写并索引它们 —— 然后让你跨语音与画面内容按语义搜索，入口可以是桌面应用、全局浮层，或本地 API。</p>

  <p>
    <a href="https://cerul.ai"><strong>官网</strong></a> &middot;
    <a href="https://github.com/cerul-ai/cerul"><strong>主仓库</strong></a> &middot;
    <a href="https://x.com/cerul_hq"><img src="https://img.shields.io/badge/follow-%40cerul__hq-000?style=flat-square&logo=x" alt="Follow on X" /></a> &middot;
    <a href="https://discord.gg/qHDEMQB9vN"><img src="https://img.shields.io/badge/join-Discord-5865F2?style=flat-square&logo=discord&logoColor=white" alt="Join Discord" /></a>
  </p>

  <p>
    <a href="./LICENSE"><img alt="License" src="https://img.shields.io/badge/license-FSL--1.1--ALv2-3b82f6?style=flat-square" /></a>
    <img alt="Platforms" src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-22c55e?style=flat-square" />
    <img alt="Status" src="https://img.shields.io/badge/status-alpha-f59e0b?style=flat-square" />
  </p>

  <p>
    <a href="README.md">English</a> &middot;
    <strong>简体中文</strong>
  </p>
</div>

<br />

> [!NOTE]
> **Alpha 阶段。** Cerul App 是 [Cerul Cloud](https://cerul.ai) 的开源、可自托管配套客户端。当前版本：**0.0.1-alpha.3**。核心已可运行 —— 桌面外壳、本地 API、索引流水线、混合搜索、浮层和托盘今天就能跑。已签名的公开安装包仍处于发布门禁阶段；在某个 GitHub Release 明确标注"已签名/已公证"安装包之前，请先从源码构建运行。详见 [项目状态与路线图](#项目状态与路线图)。

## 为什么用 Cerul App

你学到的大部分东西都藏在视频和音频里 —— 演讲、播客、课程、录制的会议 —— 而这是最难搜索的内容。转录稿只记录了"说了什么"，其余部分则锁在你再也不会回头逐帧拖动的文件里。

Cerul App 把你自己的媒体变成一份可搜索、**本地优先**的记忆：

- **你的机器，你的数据。** 媒体、转录稿和向量索引全部留在本地磁盘。推理通过*你自己*掌控的 provider key 运行，或使用完全本地的模型 —— 无需 Cerul 账号。
- **按语义搜索。** 混合检索把全文检索（SQLite/FTS）与向量搜索（内置本地 [Qdrant](https://qdrant.tech)）结合，让你找到的是那个"瞬间"，而不只是关键词。
- **常驻而不打扰。** 全局快捷键浮层、菜单栏托盘、后台索引、开机自启，让它始终离你一个按键之遥。
- **面向 Agent。** 本地 REST API 监听在 `127.0.0.1:7777`，让编码 Agent 和脚本能查询你的媒体库。

## 工作原理

索引流水线以可靠性为先 —— 即使嵌入失败，文本搜索依然可用：

1. **抓取**：从本地文件夹、YouTube（`yt-dlp`）或播客 RSS 获取媒体。
2. **提取**：用 `ffmpeg` 提取音频并采样画面帧。
3. **转写**：通过 Remote API provider 或本地 Qwen3-VL / MLX 运行时进行转录。
4. **文本入库**：立即写入 SQLite/FTS —— 马上就能搜。
5. **嵌入**：对转录分块做向量化，嵌入成功后写入 Qdrant。

> 画面理解（幻灯片、图表、屏幕文字，经由 Gemini）是条目详情页上的**可选 beta 增强步骤**，不是流水线中的必经环节。

## 数据源与界面

| 数据源 | 界面 | 推理 |
|---|---|---|
| 本地文件夹 | 桌面窗口（媒体库、数据源、设置、详情） | **Remote API** —— 你自己的 provider key（默认） |
| YouTube 频道与视频 | 全局搜索浮层（快捷键） | **本地模型** —— Qwen3-VL / MLX（macOS arm64） |
| 播客 RSS 订阅源 | 本地 REST API（`127.0.0.1:7777`） | |

## 快速开始

> 需要 Rust（stable）、Node 22 + pnpm 9，以及原生构建工具（`ffmpeg`、`protobuf`、`cmake`）。

```bash
git clone https://github.com/cerul-ai/cerul-app.git
cd cerul-app
pnpm install

./run.sh
```

需要先清空构建缓存的干净重建：

```bash
./rebuild.sh
```

## 配置

请在应用的 **设置 → 模型** 中配置 provider 连接。兼容 OpenAI 的端点也可用：基础 URL 可填写类似 `https://api.lazu.ai/v1`，然后探索模型列表，或直接手动输入模型 ID。

从源码开发时，`run.sh` 也可以读取从 [`.env.example`](.env.example) 复制出来的本地 `.env` 文件。这只是为了方便开发时预置默认 provider 参数：

```bash
# 转写（ASR）
CERUL_ASR_MODEL=whisper-1
CERUL_ASR_API_KEY=...
CERUL_ASR_BASE_URL=https://api.openai.com/v1

# 嵌入
CERUL_EMBEDDING_MODEL=...
CERUL_EMBEDDING_API_KEY=...
CERUL_EMBEDDING_BASE_URL=...
```

也可以在应用的 Models 设置里切换到完全本地的模型（Qwen3-VL / MLX）。

## 本地 API

应用运行后，可以通过 HTTP 查询你的媒体库 —— 方便 Agent 和自动化使用：

```bash
# 健康检查
curl 127.0.0.1:7777/health

# 按语义搜索
curl -X POST 127.0.0.1:7777/search \
  -H 'content-type: application/json' \
  -d '{"q": "他们关于 scaling laws 说了什么"}'
```

其他路由覆盖数据源（`/sources`）、条目（`/items`）和重新索引。完整契约由 `127.0.0.1:7777/openapi.json` 实时提供。

## 项目结构

```text
apps/
  desktop/         前端 UI（媒体库、数据源、设置、浮层）
  electron-shell/  Electron 运行时、托盘、快捷键、媒体流
crates/            Rust 核心 —— API、存储、索引、搜索、数据源
mlx-sidecar/       本地模型运行时（Qwen3-VL / MLX，macOS arm64）
scripts/           构建、打包与冒烟测试脚本
```

## 项目状态与路线图

Cerul App 处于 **alpha** 阶段。整条链路已端到端跑通，但面向大众的安装分发仍受签名、安装版冒烟覆盖和第三方二进制审查的门禁约束。

**今天已可用**
- Electron 桌面外壳、本地 REST API、存储与索引流水线
- 混合（FTS + 向量）搜索、搜索浮层、托盘、通知、开机自启
- 文件夹、YouTube、RSS 数据源；Remote API 与本地模型推理

**已签名公开安装包之前还需**
- macOS 代码签名与公证，随后是 Windows/Linux 打包
- GitHub Release 更新检查与已发布的 alpha 制品
- 完整的安装版发布冒烟覆盖
- 第三方二进制许可证审查（`ffmpeg`、`yt-dlp`、`qdrant`）

想要开箱即装？Star 并 Watch 本仓库 —— 首批已签名构建会以 GitHub Release 形式发布。

## 它与 Cerul 的关系

Cerul App 是 [Cerul](https://github.com/cerul-ai/cerul) 平台的**开源、自托管**层 —— 用你自己的机器、你自己的 key 运行它。[Cerul Cloud](https://cerul.ai) 是面向团队的托管服务，提供托管式索引、视频搜索 API 和账号级同步。本应用完全可独立运行；Cloud 账号是可选的。Cerul Cloud 的账号后端**不包含**在本仓库中；桌面客户端只在你登录时调用其公开账号 API。

## 项目治理

- [`SECURITY.md`](SECURITY.md)、[`PRIVACY.md`](PRIVACY.md)、[`CONTRIBUTING.md`](CONTRIBUTING.md)、[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) 与 [`TRADEMARKS.md`](TRADEMARKS.md)。

## 参与贡献

欢迎提交 issue 和 pull request。开发时，开 PR 前请先验证你的改动：

```bash
cargo check --workspace
pnpm --filter @cerul/desktop build
scripts/smoke.sh
```

## 许可证

[FSL-1.1-ALv2](LICENSE) © Cerul. 源码可得;每个版本在发布两年后自动转为 Apache-2.0。
