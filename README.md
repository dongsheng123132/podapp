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
cargo test                      # 单元 + 防漂移 + 向后兼容，38 项
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
