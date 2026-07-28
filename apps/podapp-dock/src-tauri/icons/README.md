# 图标

**这里的 PNG / ICO 全是生成物，别手改。** 真相源是
`apps/podapp-dock/icons-src/podapp-logo.svg`，改完这样重新生成：

```bash
cd apps/podapp-dock
node icons-src/render.mjs                                   # SVG → 1024px PNG + 32px 预览
npx @tauri-apps/cli@2 icon icons-src/podapp-logo.png -o /tmp/icons-out
cp /tmp/icons-out/{32x32,64x64,128x128,128x128@2x,icon}.png /tmp/icons-out/icon.ico src-tauri/icons/
```

`tauri icon` 还会吐一堆 `Square*Logo`（微软商店）、`icon.icns`（macOS）和安卓/iOS 的图，
**只拷上面这六个** —— 现在只发 Windows，多出来的进仓库就是没人维护的死文件。

配色取自内置的「泊舟」皮肤（`src/skins/boat.dock-skin.json`）：
深炭底 `#1b2024`→`#12161a`、青绿船身 `#27b58b`、近白主帆。改色两边一起改，
否则图标和界面会慢慢变成两个产品。

形要在 **32×32** 上还认得出来 —— 那是任务栏和资源管理器最常出现的尺寸，
也是这个图案只用三块实心色、桅杆靠两帆之间的缝去暗示的原因。
改完务必看一眼 `icons-src/preview-32-zoom.png`（`render.mjs` 会顺带生成）。
