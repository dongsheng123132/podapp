# 发版

> 这份是从 **U-King 的发版经验**抄来的，加上泊舟自己已经踩到的坑。
> U-King 那些教训是用真客户换的，别重新踩一遍。

## 一句话先说清

**代码进仓库 ≠ 到用户手上。** 泊舟现在的状态就是活例子：
0.1.1 是 2026-07-28 发的，而在那之后加的 MCP 桥、宠物、float 容器、流程、CLI 面
**一个都没到用户手上** —— 因为没发新版。写完不发，等于没写。

## 版本号在三处，有测试盯着

| 文件 | 谁读它 |
|---|---|
| `src-tauri/tauri.conf.json` | Tauri 打包 |
| `src-tauri/Cargo.toml` | Cargo（也是程序自报的版本）|
| `package.json` | npm 工具链 |

三处删不掉（三个生态各要一份），所以让它们**必须**一样：
`src-tauri/tests/version_sync.rs` 会在不一致时红。

> U-King 的 `AGENTS.md` 里写着「版本号四处同步，发版必同时改」—— 写得很清楚，
> **然后还是漂了**（同一份文档里一处说四处、一处说三处）。
> 泊舟也已经漂过一次：升 0.2.0 时 `package.json` 留在 0.1.0。
> **提醒挡不住漂移，红色能。**

## 步骤（别跳第 1 步）

```bash
# 1. 图标或 icons/ 动过 → 先 touch，否则旧图标留在 exe 里而日志全绿
#    前端产物动过也一样（tauri-build 对两者都没声明 rerun-if-changed）
touch apps/podapp-dock/src-tauri/build.rs

# 2. 打包。beforeBuildCommand 会先编 MCP sidecar 再跑前端构建
cd apps/podapp-dock && npx @tauri-apps/cli@2 build
#    需要环境变量 TAURI_SIGNING_PRIVATE_KEY（createUpdaterArtifacts 要签名）
#    ⚠️ 私钥只在你手上，不在仓库里也不该在 —— 这一步 AI 替你做不了

# 3. 装出来真跑一遍（隔离目录 + 隔离家目录，别碰自己的 ~/.podapp）
#    要看的是：内置 Pod 装进去了没、WebView2 有没有白屏、
#    podapp-mcp.exe 在不在安装目录里
MSYS2_ARG_CONV_EXCL='*' "<setup.exe>" /S "/D=C:\Temp\podapp-check"
ls "C:/Temp/podapp-check"        # 该看到 podapp-dock.exe + podapp-mcp.exe + pods/

# 4. 推代码 → 打 tag → 建 release
git push origin main && git tag -a v0.2.0 -m "..." && git push origin v0.2.0
gh release create v0.2.0 "<安装包>#中文标签" --title "..." --notes-file notes.md

# 5. 从公开直链**真下载一次**，比对 sha256 —— 传上去 ≠ 用户拿得到
```

## 自升级：三个端点，国内那个必须活着

```
1. https://u-claw.org.cn/podapp/latest.json   ← 国内入口
2. https://podapp.net/latest.json             ← Vercel
3. https://github.com/.../latest.json         ← 兜底
```

**⚠️ 目前第 1 个是 404 —— 国内用户升级不到。** 这是 0.2.0 的发版阻塞项。

为什么它必须存在：U-King 的文档写着「**u-king.org（Vercel）国内直连不通！**」，
而 `podapp.net` 响应头就是 `Server: Vercel`，GitHub 在国内也常不通。
只留 2 和 3 等于国内用户永远收不到更新，**而这件事在开发机上永远看不出来**
——开发机有代理（宪法第 4 条）。

我曾经因为它 404 就把它删了。**404 是「没上线」，不是「不该存在」。**
`src-tauri/tests/version_sync.rs` 现在会拦住这种删除。

上线它要做的事（照 U-King 的做法）：把 `latest.json` 和安装包一起同步到
`u-claw.org.cn`（新加坡服务器 nginx 静态目录），发版脚本一次做完，别手动只传一半。

## ⚠️ 顺序：先建 release，再推 main

`podapp.net` 的 `latest.json` 是**构建时从 GitHub Release 镜像**的
（`scripts/build-site.mjs` 去拉 `releases/latest/download/latest.json`）。

所以推 main 会触发 Vercel 构建，而**那一刻如果 release 还没建，镜像到的就是旧版**。
0.2.0 就是这么翻车的：push main 在 14:07:30，建 release 在 14:07:58 ——
差 28 秒，于是 podapp.net 一直发着 0.1.1，而其余三路都已经是 0.2.0。

**正确顺序**：打 tag → 建 release（GitHub 上 Latest 变成新版）→ **再推 main**。
或者推完 main 之后再手动触发一次 Vercel 重新部署。

## ⚠️ GitHub 资产名：ASCII 在文件名，中文在 `#` 后

```bash
# 对：文件本身是 ASCII 名
gh release create v0.2.0 "PodApp-0.2.0-x64-setup.exe#泊舟 AI 小程序 0.2.0 安装包"

# 错：中文在文件名上 —— gh 用的是**文件名**，`#` 后只是显示标签
gh release create v0.2.0 "泊舟 AI 小程序_0.2.0_x64-setup.exe#PodApp-0.2.0-x64-setup.exe"
```

写反了 GitHub 会把中文字符剥掉，资产变成 `AI._0.2.0_x64-setup.exe`，
而按 ASCII 名去下的直链**全部 404**。0.2.0 发的时候我就写反了，事后换的资产。

## 发布后必验（U-King 的教训）

U-King 出过「点了一键升级但重启后仍不是新版」的半升级状态，原因是
`version.json`、OSS 主源、下载目录三者只同步了一部分。所以：

1. 三路 `latest.json` 的 `version` 字段一致
2. 三路安装包 `Content-Length` 一致
3. 抽样下载一份，确认 `sha256` 和 release 页上写的一样

## 不是所有事都要发版

U-King 的 bug 巡视优先「改服务器下发的 skill 清单热下发，不发版」。
泊舟对应的是：**内置 Pod 的更新不需要发版**——`.pod` 可以单独分发，
用户拖进浮舱即可。只有运行时 / 浮舱 / 能力 crate 改了才需要发版。
