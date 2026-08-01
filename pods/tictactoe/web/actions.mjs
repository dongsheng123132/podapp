/**
 * 井字棋 —— **棋盘只有一份**。
 *
 * 你在浮舱里点格子，走的是 `app.tictactoe.game.move`。
 * 旁边的 Codex 通过 MCP 调的，是**同一个** `app.tictactoe.game.move`。
 * 不是「AI 版」和「人版」两套判胜负逻辑 —— 那样第一次改规则就会分叉，
 * 而分叉之后界面看着还是对的，AI 那条路悄悄坏掉。
 *
 * 所以这份文件里没有任何「谁在调我」的分支。它只知道棋盘和规则。
 */

import { ACTION, defineActions } from "./action-parity.generated.mjs";

const KEY = "board";

/** 八条连线。写死是因为规则不会变，算出来反而更难读。 */
const LINES = [
  [0, 1, 2], [3, 4, 5], [6, 7, 8],
  [0, 3, 6], [1, 4, 7], [2, 5, 8],
  [0, 4, 8], [2, 4, 6],
];

const empty = () => Array(9).fill(null);

async function load(ctx) {
  const saved = await ctx.pod.storage.get(KEY);
  // 存坏了、或者格式变过，都退回空盘而不是抛错 ——
  // 一局棋的价值配不上「打不开」这种失败
  return Array.isArray(saved) && saved.length === 9 ? saved : empty();
}

/** 谁赢了。返回 "X" / "O" / null。 */
function winnerOf(board) {
  for (const [a, b, c] of LINES) {
    if (board[a] && board[a] === board[b] && board[a] === board[c]) return board[a];
  }
  return null;
}

/** 该谁走。X 先手，所以数一数就知道。 */
function turnOf(board) {
  const x = board.filter((c) => c === "X").length;
  const o = board.filter((c) => c === "O").length;
  return x <= o ? "X" : "O";
}

function view(board) {
  const winner = winnerOf(board);
  const moves = board.filter(Boolean).length;
  const over = Boolean(winner) || moves === 9;
  return {
    ok: true,
    board,
    turn: over ? null : turnOf(board),
    winner,
    over,
    moves,
    message: winner
      ? `${winner} 赢了`
      : over
        ? "平局"
        : `该 ${turnOf(board)} 走（还剩 ${9 - moves} 格）`,
  };
}

export const actions = defineActions({
  [ACTION.APP_TICTACTOE_GAME_STATE]: async (_inv, ctx) => view(await load(ctx)),

  [ACTION.APP_TICTACTOE_GAME_MOVE]: async (input, ctx) => {
    const board = await load(ctx);
    const cell = Number(input.cell);

    // 三条拒绝，**都要说清是哪一条** —— AI 会瞎试，含糊的错会让它一直试同一步
    if (!Number.isInteger(cell) || cell < 0 || cell > 8) {
      return { ok: false, message: `格子要 0-8，收到 ${input.cell}` };
    }
    const state = view(board);
    if (state.over) {
      return { ...state, ok: false, message: `这局已经结束了（${state.message}）。要再来就 reset。` };
    }
    if (board[cell]) {
      return { ...state, ok: false, message: `第 ${cell} 格已经是 ${board[cell]} 了` };
    }
    const want = input.as || state.turn;
    if (want !== state.turn) {
      return { ...state, ok: false, message: `现在该 ${state.turn} 走，不是 ${want}` };
    }

    board[cell] = want;
    await ctx.pod.storage.set(KEY, board);
    const after = view(board);
    return { ...after, message: `${want} 落在第 ${cell} 格 · ${after.message}` };
  },

  [ACTION.APP_TICTACTOE_GAME_RESET]: async (_inv, ctx) => {
    await ctx.pod.storage.set(KEY, empty());
    return { ...view(empty()), message: "新开一局，X 先走" };
  },
});

export default actions;
