# PodApp

[简体中文](./README.md) | English

[![CI](https://github.com/dongsheng123132/podapp/actions/workflows/ci.yml/badge.svg)](https://github.com/dongsheng123132/podapp/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)

> AI generates. PodApp finishes.

PodApp is a desktop runtime for AI mini apps. A Pod turns an uncertain AI output into a
deterministic workflow with typed input, preview, confirmation, execution, verification,
and structured output.

It behaves like a desktop extension system, but every meaningful action is available to
the GUI, AI agents, CLI clients, and remote callers through the same implementation.

## Dock

- Attach to the Codex host window and follow it.
- Drag anywhere and remain in free-floating mode.
- Snap to any screen edge or corner.
- Restore the free position and snapped edge after restart.
- Reattach to the host with one command.
- Switch between built-in Boat, Orange Cat, and Minimal skins.
- Import safe declarative skin JSON without executing third-party CSS or JavaScript.

## Build

```powershell
cargo test --workspace

cd apps/podapp-dock
pnpm install
pnpm build

cd src-tauri
cargo test
cargo build
```

## Make a Pod

The dock can copy a complete AI-ready build prompt. The protocol, manifest example,
validation commands, and packaging steps are also in
[`docs/POD-DEVELOPMENT.md`](./docs/POD-DEVELOPMENT.md).

The built-in [`pods/memo`](./pods/memo) is a complete reference implementation.

## Customize

Create a skin with a single JSON file using
[`docs/SKIN-DEVELOPMENT.md`](./docs/SKIN-DEVELOPMENT.md). Skins may define a short mark,
colors, and corner radius. URLs, scripts, and arbitrary CSS are intentionally unsupported.

## Contributing

Code is not the only contribution path. Pods, skins, translations, reproducible bug
reports, documentation, and distribution packages are all useful. Start with
[`CONTRIBUTING.md`](./CONTRIBUTING.md).

See the [ecosystem roadmap](./docs/ECOSYSTEM.md), [showcase kit](./docs/SHOWCASE-KIT.md),
[issues](https://github.com/dongsheng123132/podapp/issues), and
[releases](https://github.com/dongsheng123132/podapp/releases).

## License

The runtime is Apache-2.0. The PodApp name and boat logo are covered separately by
[`TRADEMARK.md`](./TRADEMARK.md).
