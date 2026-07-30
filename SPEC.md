# PodApp Specification Map

> Status: navigation document. This file is informative, not a second copy of the protocol.

PodApp deliberately separates philosophy, product guidance, normative contracts, and reference
implementation. This page tells contributors which source owns each decision.

## Sources of truth

| Concern | Source of truth |
|---|---|
| AI GUI values and direction | [`MANIFESTO.md`](./MANIFESTO.md) / [`MANIFESTO.zh-CN.md`](./MANIFESTO.zh-CN.md) |
| Practical product and engineering guidance | [`PRINCIPLES.md`](./PRINCIPLES.md) / [`PRINCIPLES.zh-CN.md`](./PRINCIPLES.zh-CN.md) |
| `.pod` package format and `podapp.json` schema | [podapp-protocol](https://github.com/dongsheng123132/podapp-protocol) |
| Action IDs, schemas, effects, execution, and parity | [ActionParity](https://github.com/dongsheng123132/action-parity) and each Pod's `action-parity.json` |
| Authoring workflow for this repository | [`docs/POD-DEVELOPMENT.md`](./docs/POD-DEVELOPMENT.md) |
| Runtime behavior | `crates/podapp-runtime/` and its tests |
| Desktop surface behavior | `apps/podapp-dock/` |

If this navigation page conflicts with a normative schema, the normative schema wins. Fix this page
in the same change so the conflict does not persist.

## Anatomy of a Pod in this repository

```text
pods/<slug>/
├── podapp.json          identity, UI entry, permissions, package metadata
├── action-parity.json   stable actions, input/output schemas, effects
└── web/
    ├── index.html       human-facing surface
    └── actions.mjs      headless business-action implementation
```

The GUI calls the same action declared for headless, CLI, and MCP callers. DOM behavior remains in
the surface; business behavior remains in the action module or a registered host capability.

## Contract boundaries

- `podapp.json` owns package identity, resources, UI entry points, and requested permissions.
- `action-parity.json` owns stable Action IDs, schemas, effects, and execution semantics.
- `web/actions.mjs` implements Pod-local business actions without depending on the DOM.
- `web/index.html` presents state and invokes actions; it is not a second action implementation.
- Large outputs travel as artifact references rather than inline pixel or binary payloads.
- Permissions are enforced by the runtime below GUI, headless, CLI, and MCP entry points.

## Compatibility and change process

1. Discuss new required fields or breaking semantics in the
   [podapp-protocol repository](https://github.com/dongsheng123132/podapp-protocol) before changing
   implementations.
2. Add schema and parser support before making a new field required.
3. Preserve older packages where the declared protocol version promises compatibility.
4. Add or update round-trip fixtures whenever two package dialects must represent the same action.
5. Never resolve a protocol conflict by silently accepting two meanings for one field.

## Validation

For a runtime or contract change, run:

```powershell
cargo test --workspace
cargo run --example selftest -p podapp-runtime
```

For a Pod package, follow the validation and packaging steps in
[`docs/POD-DEVELOPMENT.md`](./docs/POD-DEVELOPMENT.md). The protocol CLI and JSON Schema live in the
separate `podapp-protocol` repository so that other hosts can implement the format without pulling
in the PodApp desktop application.

---

## 中文说明

本页只是“规范地图”，不复制协议内容。这样可以避免运行时仓库、官网和协议仓库分别维护一份
格式说明，最终互相漂移。

- 宣言定义方向，实践原则定义设计纪律。
- `.pod` 包格式与 `podapp.json` Schema 以
  [podapp-protocol](https://github.com/dongsheng123132/podapp-protocol) 为准。
- Action ID、输入输出、影响与执行语义以
  [ActionParity](https://github.com/dongsheng123132/action-parity) 和每个 Pod 的
  `action-parity.json` 为准。
- 本仓库的 `crates/podapp-runtime/` 是参考实现，不另行发明协议。

如本页与规范 Schema 冲突，以 Schema 为准，并应在同一个改动中修正本页。
