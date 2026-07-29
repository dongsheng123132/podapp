/**
 * 浮舱上的宠物 —— 按 Codex 的宠物契约播一张图集。
 *
 * # 为什么不自己定一份动画格式
 *
 * Codex 的契约已经定死了：8 列 × 9 行、每格 192×208、透明底，九行分别是
 * idle / running-right / running-left / waving / jumping / failed / waiting /
 * running / review，每行用几帧、每帧多久也是定死的。
 *
 * 自己再定一份，同一只宠物就会在 Codex app 里和浮舱里动得不一样 ——
 * 而用户只会觉得是浮舱坏了。**照抄那份契约，`hatch-pet` 产出的宠物直接能用。**
 *
 * 所以下面那张表是**抄来的，不是设计出来的**；它要改，只能是因为上游改了。
 * 皮肤 JSON 里也刻意不给覆盖这张表的口子 —— 那等于让契约有两份定义。
 *
 * # 为什么用 JS 定时器而不是 CSS 动画
 *
 * 每帧时长不一样（idle 是 280/110/110/140/140/320），`steps()` 只会均分。
 * 拿 CSS 播出来的呼吸节奏是匀速的，看着就是"机械"而不是"活的"，
 * 而这种差别在录屏里根本看不出来，只有盯着它的人能感觉到。
 */

/** 契约：格子尺寸与网格。 */
export const COLS = 8;
export const ROWS = 9;

export type PetState =
  | "idle"
  | "running-right"
  | "running-left"
  | "waving"
  | "jumping"
  | "failed"
  | "waiting"
  | "running"
  | "review";

type Track = { row: number; durations: number[] };

/** 每帧 `each` 毫秒，最后一帧 `last` 毫秒 —— 契约里大多数行都是这个形状。 */
const run = (n: number, each: number, last: number): number[] =>
  Array.from({ length: n }, (_, i) => (i === n - 1 ? last : each));

/**
 * 行号 + 每帧时长。照抄 `hatch-pet/references/animation-rows.md`。
 *
 * 行末没用到的格子在契约里必须是全透明的，所以帧数不能多播 ——
 * 多播一帧就是宠物凭空消失一下，而这种"偶尔闪一下"最难被当成 bug 报上来。
 */
const TIMELINE: Record<PetState, Track> = {
  idle: { row: 0, durations: [280, 110, 110, 140, 140, 320] },
  "running-right": { row: 1, durations: run(8, 120, 220) },
  "running-left": { row: 2, durations: run(8, 120, 220) },
  waving: { row: 3, durations: run(4, 140, 280) },
  jumping: { row: 4, durations: run(5, 140, 280) },
  failed: { row: 5, durations: run(8, 140, 240) },
  waiting: { row: 6, durations: run(6, 150, 260) },
  running: { row: 7, durations: run(6, 120, 220) },
  review: { row: 8, durations: run(6, 150, 280) },
};

/** 播一轮就回到常驻状态的那几个。其余是常驻状态，切进去就一直循环。 */
const ONE_SHOT: ReadonlySet<PetState> = new Set(["waving", "jumping", "failed"]);

/** 背景定位：把第 `col` 列第 `row` 行那一格挪到视口里。 */
function positionOf(col: number, row: number): string {
  return `${(col / (COLS - 1)) * 100}% ${(row / (ROWS - 1)) * 100}%`;
}

/**
 * 图集地址。
 *
 * **Windows 上自定义 scheme 是 `http://<scheme>.localhost/...`，别的平台才是
 * `<scheme>://localhost/...`。** 写死一种，另一半平台上就是图不出来 ——
 * 而 CSS 的 `url()` 加载失败是静默的：没有报错、没有控制台警告，
 * 只有一个不动的空格子，看起来跟"这只宠物图集是坏的"一模一样。
 *
 * 不用 `convertFileSrc`：它会把整条路径 `encodeURIComponent`，斜杠变成 `%2F`，
 * 协议处理器那边就拆不出 `<id>` 和 `sprite` 了。
 */
export function spriteUrl(petId: string): string {
  const base = navigator.userAgent.includes("Windows")
    ? "http://podapp.localhost"
    : "podapp://localhost";
  return `${base}/pet/${encodeURIComponent(petId)}/sprite`;
}

export class Pet {
  private hosts: HTMLElement[] = [];
  private petId: string | null = null;
  /** 常驻状态。一次性动作播完回到它，而不是一律回 idle —— 拖动时招个手，
   *  手放下还得接着跑，回 idle 会看起来像"卡了一下"。 */
  private resting: PetState = "idle";
  private current: PetState = "idle";
  private frame = 0;
  private timer = 0;
  private reduced = false;

  constructor() {
    // 减少动态效果是无障碍设置，不是偏好开关：开了就只留 idle 第一帧当静态宠物
    // （契约里那一帧本来就是按"能当静态图用"画的）。
    const query = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    this.reduced = query?.matches ?? false;
    query?.addEventListener?.("change", (e) => {
      this.reduced = e.matches;
      this.restart();
    });
    // 窗口不可见时别烧 CPU。浮舱是常驻程序，一只在后台空转的宠物
    // 会实实在在出现在用户的任务管理器里。
    document.addEventListener("visibilitychange", () => this.restart());
  }

  /** 把宠物贴到这些元素上。`petId` 为 null 就是没宠物，元素恢复原样。 */
  mount(hosts: HTMLElement[], petId: string | null) {
    this.stop();
    for (const host of this.hosts) {
      host.classList.remove("pet");
      host.style.backgroundImage = "";
      host.style.backgroundSize = "";
      host.style.backgroundPosition = "";
    }
    this.hosts = petId ? hosts : [];
    this.petId = petId;
    if (!petId) return;

    for (const host of this.hosts) {
      host.classList.add("pet");
      // 图集走自定义 scheme —— WebView 读不了 file://，而把几 MB 的图 base64
      // 塞进样式表会让首屏多等一大截。
      host.style.backgroundImage = `url("${spriteUrl(petId)}")`;
      host.style.backgroundSize = `${COLS * 100}% ${ROWS * 100}%`;
    }
    this.resting = "idle";
    this.play("idle");
  }

  get mounted(): boolean {
    return this.petId !== null;
  }

  /** 切到一个常驻状态。 */
  set(state: PetState) {
    if (ONE_SHOT.has(state)) return this.once(state);
    this.resting = state;
    this.play(state);
  }

  /** 播一轮一次性动作，播完回到常驻状态。 */
  once(state: PetState) {
    if (!this.mounted) return;
    this.play(state);
  }

  /** 回到常驻状态（拖动结束、动作跑完那类）。 */
  rest() {
    this.set("idle");
  }

  private play(state: PetState) {
    if (!this.mounted) return;
    this.current = state;
    this.frame = 0;
    this.draw();
    this.restart();
  }

  private restart() {
    this.stop();
    if (!this.mounted) return;
    // 静态宠物：停在当前状态第一帧，不再往下走
    if (this.reduced || document.hidden) {
      this.frame = 0;
      this.draw();
      return;
    }
    this.schedule();
  }

  private schedule() {
    const track = TIMELINE[this.current];
    this.timer = window.setTimeout(() => {
      const next = this.frame + 1;
      if (next >= track.durations.length) {
        // 一轮播完：一次性动作交棒给常驻状态，常驻状态自己循环
        if (ONE_SHOT.has(this.current)) {
          this.play(this.resting);
          return;
        }
        this.frame = 0;
      } else {
        this.frame = next;
      }
      this.draw();
      this.schedule();
    }, track.durations[this.frame]);
  }

  private draw() {
    const position = positionOf(this.frame, TIMELINE[this.current].row);
    for (const host of this.hosts) host.style.backgroundPosition = position;
  }

  private stop() {
    if (this.timer) window.clearTimeout(this.timer);
    this.timer = 0;
  }
}

/** 给测试用：契约表本身也该能被断言，不然抄错一行没人会发现。 */
export const __timeline = TIMELINE;
