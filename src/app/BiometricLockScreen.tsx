import { useCallback, useEffect, useRef, useState } from "react";
import { requestBiometricAuth } from "@/shared/ipc/onboarding";

interface BiometricLockScreenProps {
    onUnlock: () => void;
    onDisableBiometricLock: () => Promise<void>;
}

export function BiometricLockScreen({
    onUnlock,
    onDisableBiometricLock,
}: BiometricLockScreenProps) {
    const [error, setError] = useState<string | null>(null);
    const [loading, setLoading] = useState(false);
    const [disabling, setDisabling] = useState(false);
    const [attemptCount, setAttemptCount] = useState(0);
    const autoPromptedRef = useRef(false);

    const authenticate = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            const ok = await requestBiometricAuth("Unlock Continuum - your private screen history");
            if (ok) {
                onUnlock();
            } else {
                setError("Authentication failed. Tap to try again.");
                setAttemptCount((count) => count + 1);
            }
        } catch {
            setError("Touch ID is unavailable right now. You can retry or continue without lock.");
            setAttemptCount((count) => count + 1);
        } finally {
            setLoading(false);
        }
    }, [onUnlock]);

    useEffect(() => {
        if (autoPromptedRef.current) {
            return;
        }
        autoPromptedRef.current = true;
        void authenticate();
    }, [authenticate]);

    return (
        <div className="biometric-lock-overlay">
            <div className="biometric-lock-card">
                <div className="biometric-lock-icon">Continuum</div>
                <h1 className="biometric-lock-title">Continuum is Locked</h1>
                <p className="biometric-lock-subtitle">
                    Authenticate with Touch ID or your system password to access your memories.
                </p>
                {error && <div className="biometric-lock-error">{error}</div>}
                <button
                    className="biometric-lock-btn"
                    onClick={() => void authenticate()}
                    disabled={loading}
                >
                    {loading ? "Authenticating..." : "Unlock with Touch ID"}
                </button>
                {attemptCount > 0 && (
                    <button
                        className="biometric-lock-btn"
                        onClick={() => {
                            setDisabling(true);
                            void onDisableBiometricLock().finally(() => setDisabling(false));
                        }}
                        disabled={loading || disabling}
                    >
                        {disabling ? "Unlocking..." : "Continue without biometric lock"}
                    </button>
                )}
            </div>
        </div>
    );
}
