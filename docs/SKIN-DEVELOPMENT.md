# 请制作一套 PodApp 浮舱皮肤

把这一行改成具体需求，例如：做一只简洁的黑白猫咪，保留高对比度，适合长时间使用。

你正在为“泊舟 PodApp”制作一套可导入皮肤。请直接生成一个
`<name>.dock-skin.json` 文件，不要创建 CSS、JavaScript、图片链接或安装脚本。

## 格式

当前规范是 `podapp/dock-skin@0.1`：

```json
{
  "spec": "podapp/dock-skin@0.1",
  "id": "org.example.skin.cat",
  "name": "黑白猫",
  "author": "Your Name",
  "version": "0.1.0",
  "mark": "🐈",
  "colors": {
    "background": "#171819",
    "surface": "#232527",
    "foreground": "#f5f6f7",
    "muted": "#9ca2a7",
    "border": "#34383b",
    "accent": "#4eb78d",
    "success": "#67cf9e",
    "markBackground": "#2f3532"
  },
  "radius": 12
}
```

## 字段约束

- `id`：3-81 个小写字母、数字、点或连字符，发布后保持稳定。
- `name`：1-32 个字符；`author`：1-40 个字符。
- `version`：语义化版本，如 `0.1.0`。
- `mark`：最多 4 个可见字符，可用单个字母或 Emoji。
- 所有颜色必须是 6 位十六进制 `#RRGGBB`，不接受透明色、渐变、函数或 CSS 变量。
- `radius`：0-16 的数字。
- 未声明字段会被忽略；缺少必填字段会拒绝导入。

## 安全边界

皮肤只改变视觉令牌。规范明确不支持：

- JavaScript、HTML、任意 CSS；
- 本地文件路径、HTTP URL、base64 或 SVG；
- 字体下载、声音、网络请求；
- 改写按钮、动作、权限或窗口行为。

这个限制让用户可以放心导入陌生作者的皮肤，也让规范能跨 Windows、macOS 和 Linux 实现。
要制作带动画的桌面角色，应作为独立 Pod 提案，不要塞进皮肤格式。

## 设计检查

1. `foreground` 对 `background`、`surface` 保持清晰可读。
2. `muted` 仍需能读，不要只靠颜色表达选中或失败。
3. `accent` 与 `success` 应可区分。
4. 小标记在 26px 和 34px 方框中都能识别。
5. 不使用容易侵权的商业角色、品牌 Logo 或他人作品。

完成后只返回 JSON 文件路径、皮肤 ID、版本和一行设计说明。用户可在浮舱的“皮肤”面板中
选择“导入 JSON”立即预览。
