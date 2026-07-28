// 九宫格切图的动作实现。
//
// **这个文件不碰 DOM。** 界面和 AI 无头调用 import 的是同一个它 —— 那正是
// 「一次实现，两个面」的落点。写成两份（界面一份、无头一份）第一次改需求就会分叉，
// 而分叉后界面看着还是对的，AI 那条路悄悄坏掉。
//
// 图像数学一律走宿主原语（pod.image.*）：Node 里没有 canvas，而且更要紧的是，
// 界面和无头两条路必须算出**同一个结果**。

/**
 * 均分切割的格子坐标。
 *
 * 刻意用「累积边界」而不是 `col * (cellW + gap)`：后者在宽度除不尽时，
 * 每格的取整误差会累加，最后一列会短掉好几像素，而前面几列看着都对 ——
 * 这种错在九张图拼回去之前根本发现不了。
 */
function tiles(w, h, rows, cols, gap) {
  const edge = (total, n, i) => Math.round(((total - gap * (n - 1)) * i) / n) + gap * i;
  const out = [];
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const x = edge(w, cols, c);
      const y = edge(h, rows, r);
      out.push({
        row: r + 1,
        col: c + 1,
        x,
        y,
        w: edge(w, cols, c + 1) - gap * (c + 1) - (x - gap * c),
        h: edge(h, rows, r + 1) - gap * (r + 1) - (y - gap * r),
      });
    }
  }
  return out;
}

export default {
  "app.nine-grid.image.split": async (input, ctx) => {
    const pod = ctx.pod;
    const rows = input.rows ?? 3;
    const cols = input.cols ?? 3;
    const gap = input.gap ?? 0;

    const src = await pod.image.decode(input.image);

    // 缝太宽会把格子吃成负数。先拦住，给一句能照做的话 ——
    // 让它跑出「宽度 -3」再报错，用户只会看到一个看不懂的失败。
    const need = { w: gap * (cols - 1) + cols, h: gap * (rows - 1) + rows };
    if (src.w < need.w || src.h < need.h) {
      throw new Error(
        `图太小或缝太宽：${src.w}×${src.h} 切 ${rows}×${cols}、缝 ${gap}px 至少需要 ${need.w}×${need.h}`,
      );
    }

    const plan = tiles(src.w, src.h, rows, cols, gap);
    const out = [];
    for (const t of plan) {
      const piece = await pod.image.crop(src.id, { x: t.x, y: t.y, w: t.w, h: t.h });
      const dataUrl = await pod.image.encode(piece.id);
      // 交产物拿引用，**不把像素塞进返回值** —— 无头调用方（Claude Code / MCP）
      // 拿到的应该是一行人话加一个路径，不是九张图的 base64 糊在终端里。
      const art = await pod.artifact.emit({
        kind: "image",
        action: "app.nine-grid.image.split",
        data: dataUrl,
        message: `第 ${t.row} 行第 ${t.col} 列 · ${t.w}×${t.h}`,
      });
      out.push({ row: t.row, col: t.col, w: t.w, h: t.h, artifact: art });
      await pod.ui.progress(Math.round((out.length / plan.length) * 100), `切第 ${out.length} 张`);
    }

    let zip = null;
    if (input.zip) {
      // 打包走**宿主动作**，不是桥上的能力：桥上的东西对所有程序舱一律开放，
      // 而打包要读别的产物，得在 permissions.host_actions 里申报，装包时列给用户看。
      //
      // 名字补零到两位。切到 10 行以上时，`1-1 / 10-1` 在资源管理器里会排在
      // `2-1` 前面 —— 解压出来顺序全乱，而这一步没人会去检查。
      const pad = (n) => String(n).padStart(2, "0");
      const packed = await pod.action("host.zip.pack", {
        artifacts: out.map((t) => t.artifact.id),
        names: out.map((t) => `${pad(t.row)}-${pad(t.col)}.png`),
        label: `九宫格切图 ${rows}×${cols}`,
      });
      zip = packed.artifact;
    }

    return {
      ok: true,
      count: out.length,
      source: { w: src.w, h: src.h },
      cell: { w: plan[0].w, h: plan[0].h },
      tiles: out,
      zip,
      message:
        `切成 ${rows}×${cols} 共 ${out.length} 张，每张约 ${plan[0].w}×${plan[0].h}` +
        (zip ? `，已打包成一个 zip` : ""),
    };
  },
};

// 供界面复用同一份切割计划来画预览线 —— 预览和实际切割必须来自同一个函数，
// 否则「预览看着对、切出来不对」是最难解释的一类 bug。
export { tiles };
