import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMalocaView } from "./useMalocaView";
import { obtainDeviceKeyViaWebAuthn } from "./webauthn";

vi.mock("./webauthn", () => ({
  obtainDeviceKeyViaWebAuthn: vi.fn(),
}));

vi.mock("./api", () => ({
  probeMarketplaceAvailable: vi.fn(async () => false),
}));

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

describe("useMalocaView WebAuthn key acquisition", () => {
  beforeEach(() => {
    vi.mocked(obtainDeviceKeyViaWebAuthn).mockReset();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("keeps deviceKey null and sets the error when WebAuthn rejects (no fake key)", async () => {
    vi.mocked(obtainDeviceKeyViaWebAuthn).mockRejectedValue(new Error("WebAuthn not allowed"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});

    const { result } = renderHook(() => useMalocaView());

    await act(async () => {
      await result.current.handleObtainWebAuthnKey();
    });
    // Longer than the old 500 ms fallback timer, so a regression would surface here.
    await act(async () => {
      await sleep(700);
    });

    expect(obtainDeviceKeyViaWebAuthn).toHaveBeenCalledTimes(1);
    expect(result.current.deviceKey).toBeNull();
    expect(result.current.error).toBe("WebAuthn not allowed");
    expect(result.current.isWebAuthnLoading).toBe(false);
    expect(warn).not.toHaveBeenCalledWith(expect.stringContaining("mock key"));
  });

  it("stores the key returned by WebAuthn on success", async () => {
    const realKey = "a".repeat(64);
    vi.mocked(obtainDeviceKeyViaWebAuthn).mockResolvedValue(realKey);

    const { result } = renderHook(() => useMalocaView());

    await act(async () => {
      await result.current.handleObtainWebAuthnKey();
    });

    expect(result.current.deviceKey).toBe(realKey);
    expect(result.current.error).toBeNull();
  });
});
