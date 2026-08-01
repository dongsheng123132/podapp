# AGENTS.md — PodApp 动作开发入口

本仓库是多 Pod 目录，不使用单一 `action-parity.config.json`。每个
`pods/<slug>/action-parity.json` 都是该 Pod 的动作唯一真相源。

## AI 编程工具的固定工作流

1. 只在 `action-parity.json` 新增或修改 Action ID、Schema、风险和绑定。
2. 运行 `npm run action-sdk:generate`。
3. `web/actions.mjs` 用生成的 `ACTION` 常量和 `defineActions({...})`；GUI 用同一份
   `ACTION` 常量调用 `pod.action()`。禁止在可执行代码里重复手写 Action ID。
4. 给业务行为补无头测试；测试必须走 `podapp_runtime::headless::run_action` 或对应宿主动作链，
   不用截图验证业务结果。
5. 完成前运行：

```powershell
npm ci --no-audit --no-fund
npm run check:actions
cargo test --workspace
pnpm --dir apps/podapp-dock build
npm run sidecar:build
cargo test --manifest-path apps/podapp-dock/src-tauri/Cargo.toml
```

## 生成物与证据边界

- `web/action-parity.generated.mjs` 是提交进 Git 的生成物，但绝不手改；修改 Manifest 后重生成。
- `npm run action-sdk:check` 会加载处理器模块，缺少、额外或非函数处理器都会失败，也会拒绝
  `actions.mjs` / `index.html` 里的 Action ID 字符串副本。
- `npm run action-parity:validate` 只证明 Schema 和声明静态有效，输出会明确标记为
  `declared evidence`；仓库包装命令还会拒绝不存在的 Rust 测试路径，但不会假装执行了测试。
- 只有实际执行的 Rust/JS 测试通过，才可以说核心行为已验证；只有观察到 GUI、CLI、MCP
  到达同一 `execution_id`，才可以说跨界面链路已验证。

完整制作规范见 `docs/POD-DEVELOPMENT.md`，试点数据见 `docs/ACTION-SDK-PILOT.md`。
