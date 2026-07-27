// 替换二维码。不碰 DOM：界面和 AI 无头调用 import 的是同一个它。
//
// 这个程序舱只有一条铁律：**导出前必须扫得出来**。
// AI 画的二维码扫不出来是它存在的理由；如果它自己也产出一张扫不出来的图，
// 那就白做了 —— 所以验证不是可选项，是默认开着的闸门。

/** 二维码要贴的边长。取正方形短边：码是方的，硬拉成长方形就废了。 */
function squareSide(at) {
  return Math.max(21, Math.floor(Math.min(at.w, at.h)));
}

export default {
  "app.qrfix.code.replace": async (input, ctx) => {
    const pod = ctx.pod;
    const at = input.at;
    const side = squareSide(at);
    const verify = input.verify !== false; // 默认开

    if (!input.qr_text && !input.qr_image) {
      throw new Error("要贴什么码？给 qr_text（网址/文本，现生成）或 qr_image（已有的码图）");
    }

    const poster = await pod.image.decode(input.poster);
    if (at.x + side > poster.w || at.y + side > poster.h) {
      throw new Error(
        `贴的位置超出海报：海报 ${poster.w}×${poster.h}，要在 (${at.x}, ${at.y}) 贴 ${side}×${side}`,
      );
    }

    // 优先**按目标尺寸直接生成**，而不是生成完再缩放。
    // 缩放会插值，把二维码的硬边模块糊成灰阶，对比度一降扫描率就掉 ——
    // 这是这个程序舱最容易自己把自己搞砸的地方。
    let codeId;
    if (input.qr_text) {
      // qr.encode 的 scale 是每个模块占几像素；先按 1 生成一次量出模块数，
      // 再按目标边长挑一个整数倍 —— 非整数倍缩放同样会糊。
      const probeUrl = await pod.qr.encode(input.qr_text, { scale: 1 });
      const probe = await pod.image.decode(probeUrl);
      const scale = Math.max(1, Math.floor(side / probe.w));
      const url = await pod.qr.encode(input.qr_text, { scale });
      const made = await pod.image.decode(url);
      codeId = made.id;
      await pod.ui.progress(40, `生成 ${made.w}×${made.h} 的码（模块 ${scale}px）`);
    } else {
      const given = await pod.image.decode(input.qr_image);
      // 用户给的码图只能缩放。缩到目标尺寸后靠下面的验证兜底。
      codeId = given.w === side && given.h === side
        ? given.id
        : (await pod.image.resize(given.id, side, side)).id;
    }

    const code = await pod.image.decode(await pod.image.encode(codeId));

    // feather 给 0 —— 硬贴。羽化会把码的边缘模块糊掉，而那正是扫描器最先看的地方。
    //
    // 注意 sel 用的是**底图坐标**，不是贴图内坐标（宿主里是 px0 = sel.x - at.x）。
    // 传成 {0,0} 的话每个像素都落在选区外，透明度算出来是 0，
    // compositeFeather **一个像素都不画却照样返回成功** —— 海报原样出来。
    // 这个坑是导出前那道扫码验证抓到的：没有它，用户会拿一张没贴上码的图去印。
    const px = Math.floor(at.x);
    const py = Math.floor(at.y);
    const merged = await pod.image.compositeFeather(
      poster.id,
      code.id,
      { x: px, y: py },
      { x: px, y: py, w: code.w, h: code.h },
      0,
      null,
    );

    // 验证：在成品图上、贴码那一块里扫。扫成品而不是扫贴之前的码 ——
    // 中间任何一步（缩放、合成、编码）都可能把它弄坏，只有成品说了算。
    let scan = null;
    if (verify) {
      await pod.ui.progress(80, "扫一遍成品");
      const pad = Math.round(side * 0.15);
      scan = await pod.qr.scan(merged.id, {
        x: Math.max(0, at.x - pad),
        y: Math.max(0, at.y - pad),
        w: side + pad * 2,
        h: side + pad * 2,
      });
      if (scan.count === 0) {
        // **不产出**。产一张扫不出来的图比不产出更坏：用户会拿去印。
        throw new Error(
          `贴上去了但扫不出来（${side}×${side}）—— 多半是贴得太小或原图那块底色干扰。` +
            `把区域放大一点再试；真要跳过检查就传 verify: false。`,
        );
      }
    }

    const dataUrl = await pod.image.encode(merged.id);
    const art = await pod.artifact.emit({
      kind: "image",
      action: "app.qrfix.code.replace",
      data: dataUrl,
      message: `已贴真二维码 ${side}×${side}${scan ? " · 已验证可扫" : " · 未验证"}`,
    });

    return {
      ok: true,
      placed: { x: Math.floor(at.x), y: Math.floor(at.y), w: side, h: side },
      verified: !!scan,
      scanned_text: scan?.found?.[0]?.text ?? null,
      artifact: art,
      message: scan ? `成品扫得出来：${scan.found[0].text}` : "已合成（未验证可扫性）",
    };
  },
};
