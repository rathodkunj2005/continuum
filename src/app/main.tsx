import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { STORAGE_KEYS } from "@/shared/utils/config";
import { applyPalette, isPaletteKey, type PaletteMode } from "@/shared/theme/cinematic-palettes";
import "./styles/index.css";

const storedTheme = localStorage.getItem(STORAGE_KEYS.theme) as PaletteMode | null;
const theme = storedTheme === "light" ? "light" : "dark";
const storedPalette = localStorage.getItem(STORAGE_KEYS.palette);

document.documentElement.setAttribute("data-theme", theme);
applyPalette(isPaletteKey(storedPalette) ? storedPalette : "film", theme);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
        <App />
    </React.StrictMode>
);
