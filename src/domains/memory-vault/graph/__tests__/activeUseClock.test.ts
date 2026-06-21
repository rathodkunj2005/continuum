import { describe, it, expect, beforeEach } from "vitest";
import { getActiveMillis, __setActiveMillis, __resetActiveClock } from "../activeUseClock";

describe("activeUseClock", () => {
    beforeEach(() => __resetActiveClock());

    it("starts near 0", () => {
        // Reset synchronously here so no real wall-clock time accrues between
        // resetting lastTick and reading; the beforeEach reset can be separated
        // from the test body by hundreds of ms under parallel load.
        __resetActiveClock();
        expect(getActiveMillis()).toBeLessThan(100);
    });

    it("returns the value set via the test-only setter", () => {
        __setActiveMillis(3_600_000);
        // getActiveMillis tick() can add a tiny delta in a real timer; allow that.
        expect(getActiveMillis()).toBeGreaterThanOrEqual(3_600_000);
        expect(getActiveMillis()).toBeLessThan(3_600_000 + 100);
    });

    it("monotonically advances after re-setting to a larger value", () => {
        __setActiveMillis(1_000);
        __setActiveMillis(2_000);
        expect(getActiveMillis()).toBeGreaterThanOrEqual(2_000);
    });
});
