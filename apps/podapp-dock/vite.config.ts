import { defineConfig } from "vite";

// 端口写死：Tauri 的 devUrl 必须和它对得上，让 vite 自动挑端口会在
// 「上一个 dev server 没退干净」时静默换端口，然后 Tauri 打开一个白屏。
export default defineConfig({
  clearScreen: false,
  server: { port: 5180, strictPort: true },
  build: { target: "chrome110", emptyOutDir: true },
});
