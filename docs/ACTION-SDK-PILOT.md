# Action SDK 首次真实试点

这不是规范打分，是把 ShadowCore / ActionParity 工具链接进 PodApp 六个官方程序舱后的实测结果。

## 试点范围

| 指标 | 接入前 | 接入后 |
|---|---:|---:|
| 官方 Pod | 6 | 6 |
| 稳定 Action | 11 | 11 |
| `actions.mjs` / `index.html` 中手写的可执行 Action ID | 26 | 0 |
| 无效 `action-parity.json` | 2 | 0 |
| 从 Manifest 生成的 SDK | 0 | 6 |
| CI 可发现的处理器缺失、额外或类型错误 | 0 类 | 3 类 |

“26 → 0”由 Git 基线逐文件统计，只计算引号包裹、会被实际执行的 Action ID；注释里的说明文字不计入。
Action ID 现在只在 `action-parity.json` 定义一次，生成器产出 `ACTION` 常量和
`defineActions()` 完整性闸门，GUI 与处理器都引用它。

## 开发者实际少做什么

修改动作不再需要人工同步 Manifest、处理器键和 GUI 调用字符串。固定循环变成：

```powershell
# 1. 只改 action-parity.json 和业务实现
npm run action-sdk:generate

# 2. AI 或开发者交付前跑同一条检查
npm run check:actions
```

`check:actions` 同时验证六份生成物、加载六个处理器注册表，执行缺少/额外/非函数处理器的
反向契约测试，并用 ActionParity 0.6.1
验证全部 Manifest，并确认 `cargo:test:` 引用确实对应仓库里的测试函数。命令支持稳定退出码；底层工具也提供 JSON 输出，适合 Codex、
Claude Code、Kimi Code 和 Hermes 直接调用。

## 这次修掉的真实漂移

- Memo 和 Tic-tac-toe 的 Manifest 原本不符合当前 Schema，静态验证失败。
- Chatlog 与 QRFix 的 `headless_evidence` 曾引用不存在的测试名；现已指向真实测试。
- Tic-tac-toe 原来没有对应的无头证据测试；现用同一状态依次执行读盘、X/O 落子和重置。
- 六个 GUI 与处理器原来重复保存 Action ID；任何一处改名都可能静默漂移。
- Dock 后端测试原来没有先生成 Tauri 必需的 MCP sidecar，干净环境会在编译期失败；CI 现已补上构建步骤。

## 尚未证明的部分

当前达到的是：Manifest 静态有效、生成绑定无漂移、11 个动作有真实无头测试入口。
这不等于 GUI、CLI、MCP 的端到端绑定已经全部执行。因此 ActionParity 报告仍诚实显示
`declared evidence`，不会把非空测试字符串当成“已验证”。下一阶段要在宿主记录
`execution_id`，分别从 GUI、CLI、MCP 触发同一动作，再由 `action-parity verify` 生成带
commit、哈希、命令、退出码和环境信息的可复现证据包。

## 对 ShadowCore 的结论

第一项值得推广的采用指标不是规范覆盖率，而是：**动作修改后需要人工同步的字符串副本数**。
这次从 26 降到 0，并且把漂移变成 CI 失败；它已经是开发者和 AI 工具愿意采用的直接收益。
