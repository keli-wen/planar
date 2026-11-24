<p align="center">
  <img src="./assets/icon.svg" width="76" height="76" alt="Planar icon" />
</p>

<h1 align="center">Planar</h1>

<p align="center">
  <strong>一张你和 Agent 都读得懂的工作地图。</strong><br />
  <a href="./README.md">English</a> · <a href="./README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  Planar 静默只读 Claude Code、Codex、Antigravity 与 DeepSeek Harness（<code>dsh</code>）留在本地的会话，把它们铺成随时间延展的工作线：每条线停在哪，时间真正花在哪，下一步该交给哪个 Agent。
</p>

<p align="center">
  <code>本地优先</code> · <code>纯只读零侵入</code> · <code>macOS 应用 · Skill · MCP</code> · <code>MIT</code>
</p>

> [!NOTE]
> **实验阶段（`0.1.0-alpha.0`）** — Planar 目前处于 **Experimental** 快速迭代期，同时作为一个 **Agent-Native 开源仓库标杆范例** 演进。在 `v0.1.0` 稳定前，数据结构与工作流可能持续调整。

<p align="center">
  <img src="./assets/ecosystem.svg" width="860" alt="Planar 生态总览" />
</p>

## 我们提供什么

Planar 基于一套**严格只读、零终端套壳**的本地核心，面向人和 Agent 提供两种主力形态（前端同时原生适配 `320px` IDE 窄侧栏）：

1. **Planar App（macOS 桌面端与菜单栏应用）** — 克制直观的工作地图（`Week` / `Day` / `Month` 泳道、20 周热力图、内联 `RECAP` 回溯卡片、拖拽式跨会话依赖与一键回到原会话）。
2. **Agent Skill 与只读 MCP** — 为 Coding Agents 挂载 [`skills/planar/SKILL.md`](./skills/planar/SKILL.md)（平时零上下文开销，对话需要时自动按需读取地图），或为 Claude Desktop 与任意个人 Agent 接入只读 **MCP** 服务。人不需要记忆任何 CLI 命令，直接用自然语言跟你的 Agent 交流即可。

## 核心使用旅程（CUJs）

| **工作停在哪（Plan）** | **时间花在哪（Reflect）** | **Agent 从哪接手（Context）** |
| :--- | :--- | :--- |
| 离开两小时或次日早上打开 Planar，30 秒看清每条活跃工作线的停点（`Stopped at`）、遗留项（`Left open`）与跨会话依赖。 | 用截断区间并集诚实区分 `agent time` 与 `wall clock`（12 分钟空闲封顶），结合 20 周热力图与独立 `Tokens` / `Cost` 透镜复盘精力去向。 | 直接问你的个人 Agent「昨天哪条线没收尾」或「接着推进搜索重构」——它通过 Skill / MCP 按需读取 `< 3k` tokens 去噪上下文，而不用翻数 MB 原始日志。 |

<p align="center">
  <img src="./assets/ui-week.png" width="760" alt="Planar 周视图" />
</p>

<p align="center">
  <img src="./assets/ui-day.png" width="760" alt="Planar 日视图与会话回溯卡片" />
</p>

## 为什么是一张地图？

- **按语义工作线归拢，而不是按文件夹**：即使十几个并行会话共享同一个工作目录，Planar 依然把它们归拢到稳定的工作线（`Pinned` 固定线 + `Found by Planar` 发现线 + `One-off`）。增量 `Sync` 永不篡改你的固定结构；只有显式 `Regroup` 才重排未固定线。
- **Head–Middle–Tail 三段式切片**：自动忽略 `< 3` 轮的短对话；无条件剥离工具输出噪音，保留完整开头/结尾以及**中间段的用户提问**（留住中途改方向的原因），仅需约 5% 的 token 成本。

## 本地体验（Experimental）

在 `v0.1.0` 稳定前暂未发布预编译 GitHub Releases，当前可直接从源码体验（需 Rust stable Edition `2024` 与 Node.js `>= 20`）：

<details>
<summary><strong>构建 macOS App、静态预览前端，或安装 Skill / MCP</strong></summary>

```bash
# 安装开发工具并启用仓库 Git Hooks
npm install
npm run hooks:install

# 零后端立即预览前端（?mock=1，附加 &sidebar=350 可预览窄侧栏模式）
npm run preview

# 构建并启动 macOS Planar.app（桌面端 + 菜单栏）
npm run app:install && npm run app:open

# 安装本地内核以启用 Agent Skill / MCP 接入
cargo install --path crates/planar-cli
planar mcp config    # 输出各客户端可直接粘贴的 MCP 配置
```

将 [`skills/planar/SKILL.md`](./skills/planar/SKILL.md) 链接或复制到你的 Agent Skills 目录，即可在日常对话中按需调用地图完成规划、复盘与上下文交接。

</details>

## Agent-Native 仓库结构

Planar 不仅服务于 Coding Agents，仓库本身也采用 **Agent-Native** 工程结构，让 Claude Code、Codex、Antigravity、DSH、Cursor 与人类贡献者都能通过渐进式披露秒懂上下文并用机械化门禁自验证：

| 层级 | 路径 | 职责 |
| :--- | :--- | :--- |
| **统一入口路由** | [`AGENTS.md`](./AGENTS.md)（`CLAUDE.md` 软链指向） | 精简 L0 入口：核心不变量、目录契约与本地验证命令 |
| **统一设计规范** | [`DESIGN.md`](./DESIGN.md) | 视觉美学、浅深色 Token、字排规范、`bandPath` 几何与前端设计自检清单 |
| **持久化上下文** | [`contexts/CONTEXT_MAP.md`](./contexts/CONTEXT_MAP.md) | 唯一上下文根目录：产品设计、系统架构、版本与发布规范、前端契约与 Hooks 架构 |
| **仓库开发 Skills** | [`.agents/skills/planar-dev/SKILL.md`](./.agents/skills/planar-dev/SKILL.md) | 贡献者 SOP（`planar-dev` + 版本锁定的 Rust Skills，通过 `.claude/skills` 软链共享） |
| **分级审查协议** | [`REVIEW.md`](./REVIEW.md) 与 [`CONTRIBUTING.md`](./CONTRIBUTING.md) | 按变更影响半径定义 `T0`–`T3` 四档 Review 模式、不变量核对表与贡献指南 |
| **双层机械化门禁** | [`.githooks/`](./.githooks) 与 [`scripts/hooks/`](./scripts/hooks) | 通用 Git Hooks、Claude/Codex `PreToolUse` PR & Issue 守卫，以及一键 `npm run verify` |
| **产品内置 Skill** | [`skills/planar/SKILL.md`](./skills/planar/SKILL.md) | 随 Planar 发布的渐进式披露协议（`L0 → 批量 L1 → 有界 L2`） |

## 开源协议

本项目基于 [MIT License](./LICENSE) 开源。
