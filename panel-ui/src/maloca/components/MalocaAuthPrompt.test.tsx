import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { malocaApi } from "../api";
import { MalocaAuthPrompt } from "./MalocaAuthPrompt";

const SECRET = "xav_super-secret-token-123";

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

describe("MalocaAuthPrompt on 401", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.unstubAllGlobals());

  it("shows the prompt, saves the token, retries with X-Xavier-Token and never logs it", async () => {
    const logs = [
      vi.spyOn(console, "log").mockImplementation(() => {}),
      vi.spyOn(console, "warn").mockImplementation(() => {}),
      vi.spyOn(console, "error").mockImplementation(() => {}),
      vi.spyOn(console, "info").mockImplementation(() => {}),
    ];
    const fetchFn = vi.fn(async (_url: string, init?: RequestInit) => {
      const h = (init?.headers || {}) as Record<string, string>;
      return h["X-Xavier-Token"] === SECRET ? json([{ id: "t1" }]) : json({ message: "Unauthorized" }, 401);
    });
    vi.stubGlobal("fetch", fetchFn);
    render(<MalocaAuthPrompt />);
    expect(screen.queryByRole("dialog")).toBeNull();

    const call = malocaApi.getSupportTickets();
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toBeTruthy();
    const input = screen.getByLabelText("Access token") as HTMLInputElement;
    expect(input.type).toBe("password");
    expect(screen.getByText(/stored only on this device/i)).toBeTruthy();

    fireEvent.change(input, { target: { value: SECRET } });
    fireEvent.click(screen.getByRole("button", { name: "Save and retry" }));

    await expect(call).resolves.toEqual([{ id: "t1" }]);
    expect(localStorage.getItem("auth_token")).toBe(SECRET);
    expect(fetchFn).toHaveBeenCalledTimes(2);
    expect(fetchFn.mock.calls[0][0]).toBe("/maloca/support");
    expect((fetchFn.mock.calls[1][1]?.headers as Record<string, string>)["X-Xavier-Token"]).toBe(SECRET);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    for (const spy of logs) {
      expect(JSON.stringify(spy.mock.calls)).not.toContain(SECRET);
      spy.mockRestore();
    }
  });

  it("cancel rejects with the 401 error and stores nothing", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ message: "Unauthorized" }, 401)));
    render(<MalocaAuthPrompt />);
    const call = malocaApi.getSupportTickets();
    const assertion = expect(call).rejects.toThrow("Unauthorized");
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await assertion;
    expect(localStorage.getItem("auth_token")).toBeNull();
  });
});
