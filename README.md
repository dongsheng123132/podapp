# PodApp — AI 程序舱

> AI 负责生成，PodApp 负责完成。
>
> *AI generates. PodApp finishes.*

AI 会画一张好看的海报，但那个二维码扫不出来。AI 会写一个网页，但你说不清「上面那个标题」是哪一个。
PodApp 就干这一件事：**把 AI 生成的不确定结果，交给确定的动作去加工。**

一个**程序舱（Pod）**是一个有输入、参数、预览、确认、执行、验证、输出的确定性动作包。
把 Codex 生成的图拖进浮舱，选一个动作，看预览，确认，拿到结果。

## 现在能跑什么

```bash
cargo test                      # 单元 + 防漂移 + 向后兼容 + 窗口跟随，65 项
cargo run --example selftest    # 端到端：装 → 列 → 跑 → 卸，两种方言各一遍
```

自检会真的解包、真的原子换入、真的起 Node 子进程，并**真的验证沙箱拦得住越狱** ——
夹具里那个动作模块会尝试读用户目录和起子进程，两次都必须被拒。

## 仓库结构

```
crates/podapp-runtime/     运行时：清单 / 安装 / 动作分发 / 资源服务 / 权限 / 无头执行
apps/podapp-dock/          浮舱壳（Tauri 2）—— 尚未开始
pods/                      官方程序舱源码 —— 尚未开始
```

## 它是开放的吗

**可插拔。** 桥上的动词由能力注册表提供，不是写死的 `match`：

```rust
struct QrScan;
impl Capability for QrScan {
    fn name(&self) -> &'static str { "qr" }
    fn handles(&self, v: &str) -> bool { v == "qr.scan" }
    fn required(&self, _: &str) -> Option<Cap> { None }   // 只声明，无权放行
    fn call(&self, ctx: &CapCtx, _: &str, a: &Value) -> Result<Value, String> { … }
}

let caps = Capabilities::builtin().with(QrScan);   // 不用改这个 crate
```

权限闸由 `Capabilities::dispatch` **统一执行一次**。能力声明自己要什么权限，
但没有放行的权力 —— 原来七个 `match` 分支各查一次权限，漏写一个就是一条敞开的路，
而且没有任何症状。

**它就是影核（ActionParity）的实现，不是「基于影核再造一层」。**
影核是 ActionParity 的中文名，不是它下面的子协议。每个程序舱带一份 `action-parity.json`，
`.pod` 和 `.ukapp` 共用它。规范里那几条硬要求都有落点：

| 影核 | 落在哪 |
|---|---|
| §5.1 一个动作一条实现路径 | `headless::invoke` —— GUI / CLI / 无头 / 影子同一条 |
| §10.3 乐观并发，不许默认「最后写入者获胜」 | `Invocation::expected_state_version` |
| §10.4 全链路同一个关联 ID | `Invocation::execution_id` |
| 宪法 16 远程写要幂等键 | `Invocation::idempotency_key` |

装一个程序舱 = 给这台设备的动作面扩容。影子重新拉一次 `action_specs()` 就发现了新能力，
用同一个 `action_id` 调即可 —— **影核那边不需要为程序舱做任何改动**。

**独立。** `Cargo.toml` 里只有 serde / serde_json / flate2 / tar，对 U-King 零代码依赖。
`Dialect::MiniApp` 是公开发表的剖面 `action-parity/miniapp@0.1`，不是某个产品的私有格式 ——
支持一份开放剖面，和依赖一个产品，是两件事。U-King 只是 `HostProfile` 的一个取值。

## 三条不肯让步的设计

**一份实现，两个面。** 人点图标、AI 无头调用、影核远端调用走的是同一条执行路径。
不是两边各写一遍然后祈祷它们一致 —— 那样第一次改需求就分叉，而且分叉后 GUI 看着还是对的，
AI 那条路悄悄坏掉，没人会发现。

**权限闸装在面之下。** 从 GUI、从无头模块、还是从 devtools 发起，走的是同一道门。
规范里写「模块 MUST NOT 碰 fs」是对作者的*要求*；`--experimental-permission` 是让它*做不到*。
两者差别是生死攸关的，不能只写在纸上。

**两份标准，一个语义。** PodApp Protocol（`podapp.json` / `.pod`）与 ActionParity MiniApp
Profile（`uking-app.json` / `.ukapp`）是两份独立标准，运行时内部只有一个模型。
`tests/roundtrip.rs` 每次构建都断言两者等价 —— **那条测试红了，就是两份标准开始分家了**，
必须当场修，不许 skip。

## 许可

运行时：Apache-2.0（见 [LICENSE](./LICENSE)）。
PodApp 名称与小船 Logo 属商标，代码许可不覆盖它们 —— 见 [TRADEMARK.md](./TRADEMARK.md)。
