import { useCallback, useEffect, useRef, useState } from "react";
import { cloudRequestOtp, cloudStatus, cloudVerifyOtp } from "@/shared/ipc/cloud";

type Phase = "loading" | "email" | "code" | "unavailable";

interface CloudSignInProps {
    /** Called once a valid session exists (after verify, or if already signed in). */
    onSignedIn: (email: string | null) => void;
    /** Rendered as a button when this build has no Supabase backend configured. */
    onUnavailable?: () => void;
    unavailableLabel?: string;
}

/**
 * Email one-time-code sign-in for the Continuum cloud (Supabase). Reusable in
 * the onboarding "Account" step and the app-level auth gate. Renders inner card
 * content only; the caller supplies the surrounding card/overlay.
 */
export function CloudSignIn({ onSignedIn, onUnavailable, unavailableLabel }: CloudSignInProps) {
    const [phase, setPhase] = useState<Phase>("loading");
    const [email, setEmail] = useState("");
    const [code, setCode] = useState("");
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // Mirror the latest onSignedIn into a ref so the mount-time status check
    // below does not depend on it. A parent that passes an inline callback
    // (e.g. `onSignedIn={() => setCloudGateOk(true)}`) hands us a new function
    // reference on every render; if that re-ran the effect, a not-signed-in
    // status would reset `phase` to "email" and bounce the user out of the
    // code-entry step they just reached.
    const onSignedInRef = useRef(onSignedIn);
    useEffect(() => {
        onSignedInRef.current = onSignedIn;
    }, [onSignedIn]);

    // Check the stored session exactly once on mount. Only transition out of
    // the initial "loading" phase, so a late resolve can never clobber a step
    // the user has already advanced to (e.g. "code").
    useEffect(() => {
        let cancelled = false;
        cloudStatus()
            .then((status) => {
                if (cancelled) return;
                if (status.signed_in) {
                    onSignedInRef.current(status.email);
                    return;
                }
                setPhase((prev) =>
                    prev === "loading" ? (status.configured ? "email" : "unavailable") : prev,
                );
            })
            .catch(() => {
                if (!cancelled) {
                    setPhase((prev) => (prev === "loading" ? "unavailable" : prev));
                }
            });
        return () => {
            cancelled = true;
        };
    }, []);

    const sendCode = useCallback(async () => {
        const trimmed = email.trim();
        if (!trimmed) {
            setError("Please enter your email.");
            return;
        }
        setBusy(true);
        setError(null);
        try {
            await cloudRequestOtp(trimmed);
            setPhase("code");
        } catch (e) {
            setError(String(e));
        } finally {
            setBusy(false);
        }
    }, [email]);

    const verify = useCallback(async () => {
        const trimmedCode = code.trim();
        if (!trimmedCode) {
            setError("Enter the code we emailed you.");
            return;
        }
        setBusy(true);
        setError(null);
        try {
            const status = await cloudVerifyOtp(email.trim(), trimmedCode);
            onSignedIn(status.email);
        } catch (e) {
            setError(String(e));
        } finally {
            setBusy(false);
        }
    }, [code, email, onSignedIn]);

    if (phase === "loading") {
        return (
            <>
                <span className="ob-icon pulse">☁️</span>
                <h1 className="ob-title">Connecting…</h1>
            </>
        );
    }

    if (phase === "unavailable") {
        return (
            <>
                <span className="ob-icon">☁️</span>
                <h1 className="ob-title">Cloud sync isn&apos;t set up</h1>
                <p className="ob-subtitle">
                    This build has no Continuum cloud backend configured, so team sync is
                    unavailable. Everything still works locally on your Mac.
                </p>
                {onUnavailable && (
                    <button className="ob-btn-primary" onClick={onUnavailable}>
                        {unavailableLabel ?? "Continue"}
                    </button>
                )}
            </>
        );
    }

    if (phase === "code") {
        return (
            <>
                <span className="ob-icon">✉️</span>
                <h1 className="ob-title">Enter your code</h1>
                <p className="ob-subtitle">
                    We emailed a 6-digit code to <strong>{email.trim()}</strong>. Enter it
                    below to finish signing in.
                </p>
                {error && <div className="ob-error-box">{error}</div>}
                <input
                    className="ob-name-input"
                    type="text"
                    inputMode="numeric"
                    autoComplete="one-time-code"
                    value={code}
                    placeholder="123456"
                    onChange={(e) => setCode(e.target.value)}
                    onKeyDown={(e) => {
                        if (e.key === "Enter" && !busy) verify();
                    }}
                />
                <button className="ob-btn-primary" onClick={verify} disabled={busy}>
                    {busy ? "Verifying…" : "Verify & continue"}
                </button>
                <button
                    className="ob-btn-ghost"
                    onClick={() => {
                        setCode("");
                        setError(null);
                        setPhase("email");
                    }}
                    disabled={busy}
                >
                    Use a different email
                </button>
            </>
        );
    }

    // phase === "email"
    return (
        <>
            <span className="ob-icon">☁️</span>
            <h1 className="ob-title">Sign in to Continuum</h1>
            <p className="ob-subtitle">
                Continuum stays local-first. Signing in lets a privacy-filtered subset of
                your work join your team&apos;s shared knowledge graph — raw screen content,
                screenshots, and embeddings never leave your Mac.
            </p>
            {error && <div className="ob-error-box">{error}</div>}
            <input
                className="ob-name-input"
                type="email"
                autoComplete="email"
                value={email}
                placeholder="you@company.com"
                onChange={(e) => setEmail(e.target.value)}
                onKeyDown={(e) => {
                    if (e.key === "Enter" && !busy) sendCode();
                }}
            />
            <button className="ob-btn-primary" onClick={sendCode} disabled={busy}>
                {busy ? "Sending…" : "Email me a code"}
            </button>
        </>
    );
}
