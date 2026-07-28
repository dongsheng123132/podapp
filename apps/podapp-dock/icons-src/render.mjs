// podapp-logo.svg → podapp-logo.png（1024px，交给 `tauri icon` 派生全套尺寸）
//                 → preview-32-zoom.png（32px 实际渲染后放大 8 倍，用来检查小尺寸可读性）
//
// 用无头浏览器光栅化，不自己写取样器：抗锯齿和渐变要跟设计稿一致，
// 手写的在帆的斜边上会啃出锯齿，而那恰好是 32px 下最先糊掉的地方。
//
// 需要 playwright。仓库没把它列成依赖（为一个偶尔跑一次的脚本背一个浏览器不值），
// 靠 Node 往上层目录找 node_modules 解析；找不到就 `npm i -g playwright`。
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

let chromium;
try {
  ({ chromium } = await import("playwright"));
} catch {
  console.error("找不到 playwright。装一个：npm i -g playwright && npx playwright install chromium");
  process.exit(1);
}

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "podapp-logo.svg"), "utf8");

/** 换掉 svg 根节点上的尺寸；viewBox 不动，所以是等比放缩。 */
const at = (size) => source.replace('width="512" height="512"', `width="${size}" height="${size}"`);

const browser = await chromium.launch();

async function shoot(size) {
  // 视口给到至少 64，否则 32px 的页面里 Chromium 会加上滚动条布局，截出来偏一像素
  const page = await browser.newPage({
    viewport: { width: Math.max(size, 64), height: Math.max(size, 64) },
    deviceScaleFactor: 1,
  });
  await page.setContent(`<!doctype html><html><body style="margin:0">${at(size)}</body></html>`);
  const png = await page.locator("svg").screenshot({ omitBackground: true });
  await page.close();
  return png;
}

const full = await shoot(1024);
writeFileSync(join(here, "podapp-logo.png"), full);
console.log(`ok  1024×1024 → icons-src/podapp-logo.png (${full.length} B)`);

// 小尺寸预览：先真的渲染成 32px（把缩小造成的糊都留下），再用 pixelated 放大看
const small = await shoot(32);
const zoom = await browser.newPage({ viewport: { width: 256, height: 256 } });
await zoom.setContent(
  `<body style="margin:0;background:#8a8a8a"><img src="data:image/png;base64,${small.toString(
    "base64",
  )}" style="width:256px;height:256px;image-rendering:pixelated">`,
);
writeFileSync(join(here, "preview-32-zoom.png"), await zoom.screenshot());
console.log("ok  32×32 放大 8 倍 → icons-src/preview-32-zoom.png");

await browser.close();
