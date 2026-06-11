import { defineConfig } from "vite";
import { resolve } from "path";
import react from "@vitejs/plugin-react";

export default defineConfig({
    plugins: [react()],
    resolve: {
        alias: {
            "@": resolve(__dirname, "./src"),
        },
    },
    clearScreen: false,
    envPrefix: ["VITE_", "TAURI_"],
    build: {
        rollupOptions: {
            input: {
                main: resolve(__dirname, "index.html"),
                autofill: resolve(__dirname, "autofill.html"),
                omnibar: resolve(__dirname, "omnibar.html"),
            },
        },
    },
    server: {
        host: "127.0.0.1",
        port: 1420,
        strictPort: true,
        watch: {
            ignored: ["**/src-tauri/**"],
        },
    },
});
