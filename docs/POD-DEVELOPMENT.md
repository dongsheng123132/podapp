# 请制作一个 PodApp AI 小程序

把这一行改成具体需求，例如：做一个小备忘录，能新建、自动保存、删除，AI 也可以增删查。

你正在为“泊舟 AI 小程序（PodApp）”制作一个可安装 Pod。请直接完成代码、校验和打包，不要只给教程。

## 交付物

创建一个独立目录，至少包含：

```text
<slug>/
  podapp.json
  action-parity.json
  web/
    index.html
    actions.mjs
```

- `podapp.json`：身份、Web 入口、窗口尺寸、权限。
- `action-parity.json`：所有可由人或 AI 执行的业务动作及 JSON Schema。
- `web/actions.mjs`：业务动作唯一实现，不读 DOM。
- `web/index.html`：只负责界面和交互，通过 `pod.action(actionId, input)` 调动作。

## 必须遵守

1. 动作 ID 使用 `app.<slug>.<domain>.<verb>`，发布后不可随意改名。
2. 增删改查等业务逻辑只写在 `actions.mjs`；GUI、AI、CLI 调同一个动作。
3. 每个动作声明 `input_schema`、`effects`、`execution` 和 `bindings`，并支持无界面执行。
4. 权限默认全关。只申请真实需要的权限：
   - 持久化数据：`permissions.fs.app_data: true`，用 `ctx.pod.storage.get/set`。
   - 生成用户产物：用 `ctx.pod.artifact.emit`。
   - 不要直接读用户目录、起子进程或把密钥放进前端。
5. 界面做真实工具，不做介绍型落地页；窗口按内容设置，常用轻工具建议 `420×560`。
6. 支持窄窗口，文字不能溢出，所有图标按钮有 `title` 或无障碍名称。
7. 失败要返回能照着修的错误，成功返回 `{ "ok": true, "message": "..." }` 和结构化结果。
8. 输出 UTF-8，界面至少提供简体中文；技术字段和 Action ID 保持英文。
9. **不要调 AI。** `pod.ai.*` 三条永远返回 `capability_denied`，这是设计决定不是待办：
   PodApp 不做 AI 能力接入，不带 SDK、不管密钥、不背计费。
   用户机器上已经有 AI 了（Codex / Claude Code 就在旁边），它们通过 MCP 调你的动作。
   **分工是：你负责确定性的那半（采集、转换、校验、落盘），AI 负责生成和理解。**
   要「AI 帮我改这张图」的效果，正确形态是让 AI 来调你的动作，而不是你去调模型。

## 最小清单示例

```json
{
  "profile": "podapp/pod@0.1",
  "pod": {
    "id": "org.example.my-tool",
    "slug": "my-tool",
    "name": "我的工具",
    "version": "0.1.0",
    "summary": "一句话说明它解决什么问题",
    "author": "Your Name",
    "license": "MIT",
    "locales": ["zh-CN"],
    "min_host_version": "0.1.0"
  },
  "action_parity": "./action-parity.json",
  "package": {
    "kind": "web",
    "web": { "root": "web", "entry": "index.html", "actions": "actions.mjs" }
  },
  "ui": {
    "icon": "lucide:wrench",
    "accent": "#59a879",
    "container": "window",
    "window": { "width": 420, "height": 560, "resizable": true },
    "home_dock": true
  },
  "permissions": {
    "//ai": "这四项永远填 false。PodApp 不接 AI 能力（见下），填 true 也调不通",
    "ai": {
      "image_generate": false,
      "image_edit": false,
      "chat": false,
      "video_generate": false,
      "max_calls_per_run": 0
    },
    "fs": { "app_data": false, "save_dialog": false, "open_dialog": false },
    "net": { "allow": [] },
    "host_actions": []
  }
}
```

## 两种容器：`window` 和 `float`

`ui.container` 决定它开成什么样：

| | `window`（默认） | `float` |
|---|---|---|
| 形态 | 普通窗口，有标题栏 | **无边框、透明、置顶、不进任务栏** |
| 默认尺寸 | 860×620 | 260×300（下限 64×64） |
| 适合 | 要摊开看的工具：编辑、批处理、报表 | 常驻在屏幕角落、随时瞥一眼的小件：状态灯、计时器、录制指示、宠物 |

`float` 没有标题栏，所以两件事必须自己安排，否则用户会关不掉也拖不动：

```html
<!-- 想让哪块能拖窗，就给它 data-podapp-drag。
     别标在整个 body 上 —— 那会把按钮和文字选择一起吃掉，
     而「按钮点不动」比「窗口拖不动」更难查。 -->
<div class="titlebar" data-podapp-drag>⠿ 我的小件</div>
<button onclick="pod.win.close()">关掉</button>
```

```css
/* 透明底靠页面自己让开。忘了这两行的话，无边框窗里是一整块白，
   看起来像 transparent 没生效，其实是页面画了白底。 */
html, body { margin: 0; height: 100%; background: transparent; }
```

`pod.win` 只有 `drag()` 和 `close()` 两条，而且**只能动自己那扇窗** ——
宿主按窗口标签核对归属，传别人的 id 会被 `window_denied` 拒掉。
窗口操作不进 `permissions`：申报「我要能移动我自己」是没有意义的一条。

## 完成标准

在 PodApp Protocol CLI 所在仓库执行：

```powershell
node ..\podapp-protocol\bin\podapp.mjs validate <Pod目录> --json
node ..\podapp-protocol\bin\podapp.mjs pack <Pod目录>
```

必须满足：

- `validate` 输出 `"ok": true`。
- 至少直接无头调用一次主要动作，验证真实返回值和持久化结果。
- 打出 `.pod` 文件；把它拖进泊舟浮舱即可安装。
- 最终回复列出动作 ID、申请的权限、验证结果和 `.pod` 文件路径。

参考完整成品：本仓库 `pods/memo`。它演示列表、保存、删除、自动保存和 AI 无头调用共用一套动作。
