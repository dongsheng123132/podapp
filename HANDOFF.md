# 交接给 Codex

给接手的 AI：**先读仓库外那份 `AGENTS.md`**（在仓库上一级目录，是踩过的坑和不肯让步的设计；
它不在版本控制里，因为它跨 podapp 和 podapp-protocol 两个仓库），
再读这一份（这里是「现在到哪了 / 下一步该干什么 / 为什么」）。

基线：**0.2.0 已发布**（2026-07-30），main 已推。
`cargo test` 172 项 + 浮舱 14 项全绿，`tsc` 干净。

---

## 一、0.2.0 已发布，老用户能收到

四路 `latest.json` 全报 0.2.0，三处下载体积一致，从 OSS 下回来 sha256 和本地
构建逐字节相同。**并且实机验过**：装真实线上 0.1.1 跑起来，界面顶部出现
「有新版 0.2.0 · 点此更新」—— 不是服务器返回值对，是客户端真的弹了。

发版流程固化在 skill `podapp-release` 和 `podapp/docs/RELEASE.md`。
两条最容易翻车的：**先建 release 再推 main**（否则网站镜像到旧版）、
**`gh` 资产名 ASCII 放文件名中文放 `#` 后**（写反了中文会被剥掉，直链全 404）。

⚠️ **`u-claw.org.cn/podapp/` 整个是死的**（latest.json 和 exe 都 404）。
它还在 0.1.x 用户的端点表里，只是让他们多跳一次。要么在 nginx 上转发到
`cloud.u-claw.org`，要么就这么放着。

⚠️ **端点是编译期烧进 exe 的**，所以"换域名"只能向前生效。
`cloud.u-claw.org` 是 0.1.x 用户唯一活着的国内路径，**至少保留两三个版本**。

---

## 二、这一段建成了什么（五个面通了）

一个 `.pod` 装上之后，同一批动作在五个地方可调，**走的是同一条
`headless::invoke`**：

| 面 | 入口 | 状态 |
|---|---|---|
| GUI | 浮舱点按钮 | ✅ |
| MCP | `podapp-mcp`（随包发布的 sidecar）| ✅ 2026-07-28 规范 |
| CLI | `podapp-run run/flow` | ✅ |
| 流程 | `podapp-flow`，浮舱里有入口 | ✅ |
| 定时 | 系统调度器调 CLI | ✅ 不自带定时器 |

闭环也通了：**泊舟产出 → `host.cli.codex` 喂给 Codex → 回答落回泊舟收件箱**。

新增的 crate（**都零第三方依赖**，`podapp-runtime` 仍是 4 个依赖）：
`podapp-host`（能力的唯一组装处）、`podapp-flow`、`podapp-cli`、`podapp-run`。

---

## 三、下一步：我会按这个顺序做

### 1. 发 0.2.0（人 + AI 配合）
见上。

### 2. ⚠️ 先修「证件照抠图」的 storage key ← **改一行的事，但现在功能是坏的**

**这条是实机跑出来的，不是推测。** 用户机器上装着一个不在本仓库的 Pod
`org.podapp.image.idcard-cutout`（证件照抠图 v0.1.0），它一个动作就同时做
抠图和换底色（`image` + `target_color` + `bg_color` + `tolerance` + `edge_feather`）
—— 也就是说「身份证抠图 → 换白底」这块砖**早就有了**，我之前说"一个都没有"是错的
（我只看了仓库里的 `pods/`）。

但它跑不起来：

```
第 1 步失败：invalid_input: 非法的 storage key
```

原因：它用 `const STORAGE_KEY = "idcard-cutout/history"` —— 而运行时**明确禁止
key 里出现 `/`**（key 会被拼成文件名，放行斜杠等于放行路径穿越，
见 `capability.rs` 的 `StorageCap`）。

**这不只是流程里坏，GUI 里点「保存历史」也会失败。** 改成 `"history"` 即可。
这个 Pod 不在本仓库，得找到它的源码或者让作者改。

**顺带一个该考虑的问题**：这条规则的错误信息只说「非法」，没说**哪里非法**。
AI 生成的 Pod 会一直踩，而且看不出该改什么。建议把错误改成
「key 不能含 `/ \ . `，收到的是 xxx」。

### 3. 补「图像基础操作」Pod

**注意：优先级比我原来写的低。** 证件照那块砖已经有了（见上），
所以缺的没有我以为的那么多。先修上面那条，再看还差什么。

四份官方案例里三份是 `nine-grid` 自己串自己，因为现有 Pod 的动作粒度对不上：

| Pod | 能当下一步输入吗 |
|---|---|
| nine-grid | ✅ 产出图片 |
| chatlog | ✅ 产出文件 |
| idcard-cutout（不在本仓库） | ✅ 抠图+换底一步到位，**但 storage key 是坏的，见上** |
| annotate | ⚠️ 产出 `overlay`，没验过是不是 artifact |
| qrfix | ⚠️ 要 `at` 框选坐标，AI 给不出来 |
| memo / tictactoe / alarm | ❌ 不产出产物 |

**用户机器上装的 Pod 比仓库里多**（还有番茄闹钟 v1.0.0）。
判断"缺什么砖"时**别只看 `pods/`**，去 `~/.podapp/apps/` 看一眼实际装了什么。

要做的：**裁切 / 换底色 / 缩放 / 加边距**，四个动词，
`org.podapp.image.basics`。每个**只产出一张**（这样 `$prev` 天然串得起来，
见下面「$prev 的定义」）。运行时的 `image.rs` 已经有 crop/resize/fillRect，
够用，不需要新能力。

做完之后案例库才真有东西可放，而那是小白能用起来的前提。

### 4. `git status` 常驻小窗（float + `host.cli.git`）

能力已就绪，差一个 float Pod。**贴在 AI CLI 旁边最该回答的问题是「它刚改了什么」**，
一天问几十次。这是「大软件忽视的小机会」最直白的一个。

### 5. 宠物的状态源接 Codex（可选）

现在宠物状态只来自浮舱自己的事件（拖动 / 开程序舱 / 出错 / 装 Pod / 悬停）。
`podapp-codex` 已能读 `~/.codex/sessions/**/rollout-*.jsonl`，
理论上能让宠物反映「Codex 在跑 / 在等你确认」。

**但先别做。** 那个目录是上游内部实现，U-King 为此删过一整块探测。
读历史读错了是少一条记录，读状态读错了是宠物永远卡在 running。
等有人真的要这个再说。

### 6. 不建议做的

- **纯游戏**（2048、扫雷）：在泊舟里只是「开在小窗里的网页」，
  没有动作、没有两个面，反而稀释定位。井字棋值得做是因为它**演示论点**
  （人点格子和 Codex 走 MCP 调同一个动作）。
- **`awesome-podapp` 第三方索引仓库**：等 ≥10 个第三方 Pod 再建。
  空的 awesome 列表等于公开宣告「没人用」。
- **录屏 / 麦克风走 WebView2 权限**：查过，`getUserMedia` 在 WebView2 里
  **挂住不返回**（不是被拒），要修得写 COM 权限处理器。而正确做法是走
  `host.cli.ffmpeg` —— 采集用用户自己装的工具，零 COM 代码。

---

## 四、给 Codex 的几条「这里和别处不一样」

1. **`$prev` 只在上一步恰好产出 1 个产物时有定义。** 多个就报错，不猜。
   要多个用 `$prev[]`。「产出了几个」以**产物账本**为准，不是数返回值字段
   ——各 Pod 的返回字段名都不一样，猜字段名这条路不成立（试过，`$prev` 一直是 null）。

2. **动作签名是 `async (input, ctx)`**，不是 `async ({ input }, ctx)`。
   写错不报错，只会让 `input` 变 `undefined`，然后报一句跟签名无关的错。
   （这个坑我在文档里写错过一次，AI 会照着抄。）

3. **能力和宿主动作只在 `podapp-host` 组装一处。** 别在浮舱 / MCP / CLI
   各拼一份 —— 那正是刚修掉的 parity 破口（MCP 曾经一个宿主动作都没有，
   于是 chatlog 和 nine-grid 在浮舱能用、AI 调就失败，**两边都不报错**）。

4. **`host.cli.*` 的形状**：意图在 Pod，argv 在宿主；程序白名单在宿主一侧；
   不经 shell。永远不传 `--dangerously-bypass-*`（有测试扫生产代码盯着）。

5. **速度基线在 `AGENTS.md`**：单步 `invoke` 57.7ms，其中 Node 启动占 68%。
   改 `invoke` 之前跑 `cargo run --release --example bench -p podapp-runtime`，
   退化超 20% 就是改错了。**别为了那 17ms 去缓存 `runner.mjs`**
   ——那会造出第三个「缓存和源码不一致」的坑。

6. **别写会花钱的测试。** 我写过一条真调四次 `codex exec` 的单元测试，
   一次 `cargo test` 41 秒**而且每次都在花用户的钱**。判断逻辑抽成纯函数就好了。

7. **验证脚本的坑集中在 `AGENTS.md` 19–21 条**（PowerShell 要 BOM、
   `FindWindow` 走 ANSI、量窗口前要 `SetProcessDPIAware`、
   「窗口不见了」可能是功能正常）。写自动化验证前先看一眼，能省两轮。

---

## 五、这一段留下的守卫测试（别删，它们都抓到过真问题）

| 测试 | 抓什么 |
|---|---|
| `podapp-host` 的 `every_shipped_pod_can_reach_the_host_actions_it_declares` | 加 Pod 忘了接能力 |
| `podapp-flow` 的 `example_flows` | 官方 Pod 改动作 ID 导致案例腐坏 |
| `podapp-cli` 的 `the_dangerous_flags_are_never_passed` | 有人给 codex 放开沙箱 |
| `podapp-dock` 的 `version_sync` | 三处版本号漂移 + 国内更新端点被删 |
| `podapp-runtime` 的 `script_exposes_both_names_and_no_generic_fetch` | 桥上出现通用 fetch 出口 |

**每一条都有两条护栏**：不仅验内容，还验「自己有没有失效」
（查到 0 个就断言失败）—— 因为绿着什么都没验比红更危险。
这一段里我撞见过两条这样的假测试。
