"""造一只**测试用**的宠物，验浮舱那条宠物通路能不能真跑起来。

不是美术资源，是**测量工具**：每一格画一个能一眼认出行列的图形，
所以「贴错格子」「行号算错」「方向反了」这些错在屏幕上是可见的，
而不是「看起来动了，但动的是哪一帧不知道」。

Codex 宠物契约：1536×1872，8 列 × 9 行，每格 192×208，透明底，
行末没用到的格子必须全透明。这里照抄，不发明第二份。

只用标准库（zlib + struct）—— 这台机器没有 PIL，而为了一个测试工具
去装图像库，是把验证的门槛抬高给下一个人。

用法：
    python scripts/make-test-pet.py <输出目录>
"""

import struct
import sys
import zlib
from pathlib import Path

COLS, ROWS = 8, 9
CELL_W, CELL_H = 192, 208
W, H = COLS * CELL_W, ROWS * CELL_H

# 契约里每行用到的帧数（references/animation-rows.md）。
# 用到几帧就画几帧，后面的必须留空 —— 多画一格，播放时就会多闪一下，
# 而那正是这只测试宠物要能暴露的错。
USED = [6, 8, 8, 4, 5, 8, 6, 6, 6]

# 九行各一个颜色，肉眼一看就知道现在播的是哪一行
ROW_COLOR = [
    (0x4A, 0x9E, 0xFF),  # 0 idle
    (0x3F, 0xC9, 0x7A),  # 1 running-right
    (0x2E, 0x9A, 0x5C),  # 2 running-left
    (0xFF, 0xC2, 0x4A),  # 3 waving
    (0xB4, 0x7A, 0xFF),  # 4 jumping
    (0xFF, 0x5A, 0x5A),  # 5 failed
    (0xFF, 0x93, 0x3D),  # 6 waiting
    (0x4A, 0xD9, 0xD9),  # 7 running
    (0xD9, 0x8A, 0xFF),  # 8 review
]


def blank():
    return [[(0, 0, 0, 0)] * W for _ in range(H)]


def fill(px, x0, y0, w, h, rgba):
    for y in range(y0, min(y0 + h, H)):
        for x in range(x0, min(x0 + w, W)):
            if 0 <= x < W and 0 <= y < H:
                px[y][x] = rgba


def draw_cell(px, col, row):
    """一格 = 一个方块 + col 个刻度条。

    方块**逐帧上下走**，所以静止和播放一眼能分辨；
    刻度条数出列号，所以「跳帧」「顺序反了」也是可见的。
    """
    ox, oy = col * CELL_W, row * CELL_H
    r, g, b = ROW_COLOR[row]

    # 安全边距：契约要求每帧都装得进自己的格子，边上留白才验得出越界
    pad = 16
    used = USED[row]
    # 上下摆动：第一帧和最后一帧回到同一高度，循环时不会「啪」地跳
    phase = col / max(used - 1, 1)
    bob = int(18 * abs(1 - 2 * phase))

    fill(px, ox + pad, oy + pad + bob, CELL_W - 2 * pad, CELL_H - 2 * pad - 24, (r, g, b, 255))
    # 列号刻度：左下角画 col+1 根竖条
    for i in range(col + 1):
        fill(px, ox + pad + i * 14, oy + CELL_H - pad - 14, 9, 12, (255, 255, 255, 255))
    # 行号刻度：右上角画 row+1 个点
    for i in range(row + 1):
        fill(px, ox + CELL_W - pad - 10, oy + pad + i * 12, 7, 7, (0, 0, 0, 255))


def png_bytes(px):
    raw = bytearray()
    for row in px:
        raw.append(0)  # 每行的 filter 类型：0 = None
        for r, g, b, a in row:
            raw += bytes((r, g, b, a))

    def chunk(tag, data):
        out = struct.pack(">I", len(data)) + tag + data
        return out + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 6))
        + chunk(b"IEND", b"")
    )


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    out.mkdir(parents=True, exist_ok=True)

    px = blank()
    for row in range(ROWS):
        for col in range(USED[row]):
            draw_cell(px, col, row)

    (out / "spritesheet.png").write_bytes(png_bytes(px))
    (out / "pet.json").write_text(
        '{\n'
        '  "id": "testpet",\n'
        '  "displayName": "测量宠物",\n'
        '  "description": "每格标着行列号的验证图集，不是美术资源",\n'
        '  "spritesheetPath": "spritesheet.png"\n'
        '}\n',
        encoding="utf-8",
    )
    print(f"{out / 'spritesheet.png'} {W}x{H}")


if __name__ == "__main__":
    main()
