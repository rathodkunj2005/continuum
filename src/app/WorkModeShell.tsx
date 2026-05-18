import App from "./App";

/**
 * Work-mode shell — the existing productive layout. Today this simply
 * renders <App />, which owns onboarding / biometric gating and the
 * sidebar+search+panels surface. Refactoring App.tsx into smaller
 * pieces under this shell is out of scope for the foundation slice.
 */
export function WorkModeShell() {
    return <App />;
}

export default WorkModeShell;
