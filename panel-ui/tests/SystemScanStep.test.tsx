import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SystemScanStep } from "../src/components/Onboarding/SystemScanStep";

/**
 * Regression test for a real bug found via GitHub Actions CI on PR #2549
 * (panel-ui e2e onboarding.spec.ts): SystemScanStep's scan effect used to depend on the
 * `onNext` callback identity (`useEffect(() => {...}, [onNext])`). OnboardingFlow passes an
 * inline `(info) => {...; handleNext();}` function, recreated on every render. SystemScanStep
 * stays mounted inside <App> while App's own background fetches (threads/bookmarks/widgets/
 * graph — see onboarding.spec.ts's mocks for exactly these) resolve and setState, re-rendering
 * App and therefore this whole subtree, each time handing SystemScanStep a *new* `onNext`
 * reference. With `onNext` in the effect's dependency array, each of those re-renders
 * restarted ANOTHER overlapping ~4.6s scan() timer chain; once several resolved in a burst,
 * `onNext` (and so `handleNext()`/`setStep`) fired multiple times in quick succession, skipping
 * the onboarding flow straight past the Hardware step. This reproduced deterministically in
 * real CI (never locally): every onboarding.spec.ts failure's captured page state was the
 * Integrations step (EXTERNAL_UPLINK) — two steps past the one the test was waiting on.
 *
 * Confirmed mechanism directly: re-rendering the OLD component with fresh `onNext` props (via
 * React Testing Library's `rerender`, the same "parent gave me a new callback" event a real
 * App re-render produces) re-invoked the scan (and its /health fetch) once per rerender — 5
 * fetches for 1 mount + 4 rerenders. The fix keeps the effect's dependency array stable (`[]`,
 * run once per mount) via a ref holding the latest `onNext`, so it fires exactly once
 * regardless of how many times the parent re-renders during the scan.
 */
describe("SystemScanStep — stable scan effect under parent re-renders", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("only scans (fetches /health) once, even when the parent re-renders with a fresh onNext mid-scan", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ system: { ram_usage_percent: 40, cpus: 8 } }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const onNextCalls: unknown[] = [];
    const { rerender } = render(
      <SystemScanStep onNext={(info) => onNextCalls.push(info)} />,
    );

    // Simulate App's separate background fetches (threads/bookmarks/widgets/graph) resolving
    // at different times during the scan, each producing a real re-render with a brand-new
    // inline `onNext` — exactly what OnboardingFlow.tsx hands this component on every render.
    for (const ms of [10, 20, 30, 600]) {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
      });
      rerender(<SystemScanStep onNext={(info) => onNextCalls.push(info)} />);
    }

    // Let the scan(s) run to completion (1000+800+800ms pacing + fetch + 2000ms post-success
    // delay from whichever mount(s) are still in flight).
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(onNextCalls).toHaveLength(1);
  });
});
