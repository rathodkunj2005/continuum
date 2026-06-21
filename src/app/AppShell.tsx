import { MotionWallpaper } from "@/shared/components/MotionWallpaper";
import { useActiveCinematicPalette } from "@/shared/hooks/useActiveCinematicPalette";
import { useActiveWallpaper } from "@/shared/hooks/useActiveWallpaper";
import { WorkModeShell } from "./WorkModeShell";
import "./styles/wallpaper.css";

/**
 * Root shell: interactive motion wallpaper + the main productive UI.
 * Immersive scroll mode was merged into the default home experience (App).
 */
export function AppShell() {
    const { aurora } = useActiveCinematicPalette();
    const wallpaperId = useActiveWallpaper();

    return (
        <>
            <div className="continuum-wallpaper-layer" aria-hidden>
                <MotionWallpaper
                    wallpaperId={wallpaperId}
                    auroraBg={aurora.bg}
                    aurMid={aurora.mid}
                    aurAcc={aurora.acc}
                />
            </div>
            <div className="continuum-app-chrome">
                <WorkModeShell />
            </div>
        </>
    );
}

export default AppShell;
