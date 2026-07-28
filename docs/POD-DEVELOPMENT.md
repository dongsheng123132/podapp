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
