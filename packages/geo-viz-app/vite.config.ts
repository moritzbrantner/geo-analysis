import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  optimizeDeps: {
    exclude: ["@moenarch/geo-viz-wasm"],
  },
  plugins: [react()],
});
