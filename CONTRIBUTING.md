# 参与 PodApp

PodApp 接受代码、Pod、皮肤、翻译、文档和可复现的 Bug 报告。请让一个 PR 只解决一件事，
便于验证、回滚和署名。

## 选择一条贡献路径

| 路径 | 改哪里 | 是否需要 Rust |
|---|---|---|
| 新皮肤 | `apps/podapp-dock/src/skins/` | 否 |
| 新 Pod | `pods/<slug>/` | 否 |
| 翻译与文档 | `README_*.md`、`docs/`、界面文案 | 否 |
| 浮舱界面 | `apps/podapp-dock/src/` | 否 |
| 协议与运行时 | `crates/` | 是 |

皮肤和文档修正可直接发 PR。新增 Pod、协议变更、权限模型和大型功能请先开 Issue，
写清使用场景、动作 ID、权限和兼容性影响，避免两边同时实现不同协议。

## 本地验证

```powershell
cargo test --workspace

cd apps/podapp-dock
pnpm install --frozen-lockfile
pnpm build

cd src-tauri
cargo test
```

改窗口几何时还要运行：

```powershell
cargo run -p podapp-win --example probe -- --verify
```

改 Pod 时按 [`docs/POD-DEVELOPMENT.md`](./docs/POD-DEVELOPMENT.md) 完成 `validate`、无头调用和
打包。改皮肤时按 [`docs/SKIN-DEVELOPMENT.md`](./docs/SKIN-DEVELOPMENT.md) 校验并在深色、浅色
界面各检查一次文字可读性。

## PR 要求

1. 说明用户能观察到的变化，不只写内部实现。
2. 列出实际运行过的验证命令与结果。
3. 新业务动作必须同时声明 schema、effects、execution 和稳定 Action ID。
4. 权限默认关闭；新增权限必须解释必要性。
5. 不提交密钥、真实用户数据、构建目录或个人机器路径。
6. 界面至少支持简体中文；新增英文文案时同步考虑 `README_EN.md`。
7. 视觉变化附一张清晰截图；不要用截图代替业务测试。

提交即表示贡献按仓库 Apache-2.0 许可发布。安全问题不要开公开 Issue，请按
[`SECURITY.md`](./SECURITY.md) 报告。
