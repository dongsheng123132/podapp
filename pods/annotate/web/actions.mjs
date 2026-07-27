// 图片标注 → 结构化任务。不碰 DOM：界面和 AI 无头调用 import 的是同一个它。
//
// 这个程序舱的产出**不是图**，是一份让 agent 不必猜的任务书。图只是给人看的旁证。

/** 画一个矩形描边。宿主原语里只有填充矩形，描边就是四条细填充。 */
async function strokeRect(pod, id, r, color, thick) {
    const t = Math.max(1, Math.round(thick));
    const bars = [
        { x: r.x, y: r.y, w: r.w, h: t },                 // 上
        { x: r.x, y: r.y + r.h - t, w: r.w, h: t },       // 下
        { x: r.x, y: r.y, w: t, h: r.h },                 // 左
        { x: r.x + r.w - t, y: r.y, w: t, h: r.h },       // 右
    ];
    for (const b of bars) {
        // fillRect 是**就地**修改，返回的还是同一个句柄 —— 别拿返回值当新图用
        await pod.image.fillRect(id, b, color, {});
    }
}

/** 左上角的编号色块，让「第 2 处」在图上找得到。 */
async function badge(pod, id, r, color, size) {
    const s = Math.max(12, Math.round(size));
    await pod.image.fillRect(id, { x: r.x, y: Math.max(0, r.y - s), w: s, h: s }, color, {});
}

export default {
    "app.annotate.task.build": async (input, ctx) => {
        const pod = ctx.pod;
        const list = input.annotations ?? [];
        if (list.length === 0) {
            throw new Error("一个标注都没有 —— 先在图上框出要改的地方");
        }

        const src = await pod.image.decode(input.image);

        // 越界的框先夹回图内。不夹的话宿主会自己截断，而返回给 agent 的坐标
        // 就和实际画出来的对不上 —— 那正是这个程序舱要消灭的那种含糊。
        const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, Math.round(v)));
        const regions = list.map((a, i) => {
            const x = clamp(a.x, 0, src.w - 1);
            const y = clamp(a.y, 0, src.h - 1);
            return {
                index: i + 1,
                x,
                y,
                w: clamp(a.w, 1, src.w - x),
                h: clamp(a.h, 1, src.h - y),
                instruction: String(a.instruction).trim(),
            };
        });

        // 画标注图：红框 + 编号块
        const overlay = await pod.image.clone(src.id);
        const thick = Math.max(2, Math.round(Math.min(src.w, src.h) / 300));
        for (const r of regions) {
            await strokeRect(pod, overlay.id, r, "#ff3b30", thick);
            await badge(pod, overlay.id, r, "#ff3b30", thick * 8);
        }
        const overlayUrl = await pod.image.encode(overlay.id);
        const art = await pod.artifact.emit({
            kind: "image",
            action: "app.annotate.task.build",
            data: overlayUrl,
            message: `${regions.length} 处标注 · ${src.w}×${src.h}`,
        });

        // 给 agent 的任务书。坐标系说清楚 —— 不写明的话对面只能猜是原图还是显示尺寸，
        // 而猜错了框就偏了，偏的量还随窗口大小变。
        const task = {
            image: { w: src.w, h: src.h, coordinate_space: "source-pixels", origin: "top-left" },
            annotations: regions.map((r) => ({
                index: r.index,
                type: "rectangle",
                x: r.x,
                y: r.y,
                width: r.w,
                height: r.h,
                instruction: r.instruction,
            })),
            note: input.note ? String(input.note).trim() : undefined,
            overlay_artifact: art.id,
        };

        // 一段可以直接粘进 Codex 的话。结构化 JSON 是给程序读的，
        // 这段是给人贴的 —— 两者同源，不会说不一样的话。
        const prompt = [
            `这张图 ${src.w}×${src.h}，坐标以原图像素为准、左上角为原点。请按以下 ${regions.length} 处修改：`,
            ...regions.map(
                (r) => `${r.index}. 区域 (${r.x}, ${r.y}) 起 ${r.w}×${r.h}：${r.instruction}`,
            ),
            input.note ? `\n补充：${String(input.note).trim()}` : "",
            `\n标注图（画了红框和编号）：${art.path ?? art.id}`,
        ]
            .filter(Boolean)
            .join("\n");

        return {
            ok: true,
            count: regions.length,
            task,
            prompt,
            overlay: art,
            message: `${regions.length} 处标注已生成任务`,
        };
    },
};
