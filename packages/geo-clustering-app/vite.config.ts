import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  optimizeDeps: {
    exclude: ["@moenarch/geo-clustering-wasm"],
  },
  plugins: [react()],
});
