import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ExplainTab, buildExplainRequest, EMPTY_EXPLAIN_FORM } from "./ExplainTab";

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

const OPENUI =
  '<SectionBlock title="What it does"><TextContent content="Validates the explain request body."></TextContent></SectionBlock>';

const success = {
  plain_text: "Plain fallback text",
  openui_lang: OPENUI,
  cited_refs: ["src/server/explain_route.rs:31-35"],
  unknown_refs: [],
  skipped_sources: [],
};

function fillAndSubmit() {
  fireEvent.change(screen.getByLabelText("File path"), { target: { value: "src/server/explain_route.rs" } });
  fireEvent.change(screen.getByLabelText("Start line"), { target: { value: "31" } });
  fireEvent.change(screen.getByLabelText("End line"), { target: { value: "35" } });
  fireEvent.click(screen.getByRole("button", { name: "Explain" }));
}

describe("ExplainTab", () => {
  it("posts the form to /v1/explain and renders the OpenUI output with cited refs", async () => {
    const fetchMock = vi.fn(async () => json(success));
    vi.stubGlobal("fetch", fetchMock);
    render(<ExplainTab />);

    fillAndSubmit();

    expect(await screen.findByText("What it does")).toBeTruthy();
    expect(screen.getByText("Validates the explain request body.")).toBeTruthy();
    expect(screen.getByRole("list", { name: "Cited references" })).toBeTruthy();
    expect(screen.getByText("src/server/explain_route.rs:31-35")).toBeTruthy();

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toMatch(/\/v1\/explain$/);
    expect(init.method).toBe("POST");
    expect(JSON.parse(String(init.body))).toEqual({
      file: "src/server/explain_route.rs",
      start_line: 31,
      end_line: 35,
      audience: "newcomer",
    });
  });

  it("falls back to plain_text when the OpenUI markup has nothing to render", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ ...success, openui_lang: "" })));
    render(<ExplainTab />);
    fillAndSubmit();
    expect(await screen.findByTestId("explain-plain-text")).toBeTruthy();
    expect(screen.getByText("Plain fallback text")).toBeTruthy();
  });

  it("shows the not-backed-by-evidence warning for unknown refs", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => json({ ...success, unknown_refs: ["src/ghost.rs:1-2"] })),
    );
    render(<ExplainTab />);
    fillAndSubmit();

    const warning = await screen.findByRole("alert");
    expect(warning.textContent).toContain("Not backed by evidence");
    expect(screen.getByText("src/ghost.rs:1-2")).toBeTruthy();
  });

  it("shows skipped sources as muted notes", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => json({ ...success, skipped_sources: [["git-log", "no repo history"]] })),
    );
    render(<ExplainTab />);
    fillAndSubmit();
    expect(await screen.findByText("Skipped git-log: no repo history")).toBeTruthy();
  });

  it("maps a 400 to the server's validation message", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ error: "invalid line range 35-31" }, 400)));
    render(<ExplainTab />);
    fillAndSubmit();
    expect(await screen.findByText("invalid line range 35-31")).toBeTruthy();
  });

  it("maps a 502 to the model unavailable message", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ error: "upstream timeout" }, 502)));
    render(<ExplainTab />);
    fillAndSubmit();
    expect(await screen.findByText(/Model unavailable/)).toBeTruthy();
  });

  it("maps a 404 to the not-available-on-this-daemon message", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("", { status: 404 })));
    render(<ExplainTab />);
    fillAndSubmit();
    expect(await screen.findByText(/not available on this daemon/)).toBeTruthy();
    await waitFor(() => expect(screen.queryByRole("list", { name: "Cited references" })).toBeNull());
  });
});

describe("buildExplainRequest", () => {
  it("returns null until file and both line numbers are set", () => {
    expect(buildExplainRequest(EMPTY_EXPLAIN_FORM)).toBeNull();
    expect(
      buildExplainRequest({ ...EMPTY_EXPLAIN_FORM, file: "a.rs", startLine: "1" }),
    ).toBeNull();
  });

  it("omits an empty symbol and keeps a trimmed one", () => {
    const base = { ...EMPTY_EXPLAIN_FORM, file: " a.rs ", startLine: "1", endLine: "2" };
    expect(buildExplainRequest(base)).not.toHaveProperty("symbol");
    expect(buildExplainRequest({ ...base, symbol: "  run  " })).toMatchObject({ file: "a.rs", symbol: "run" });
  });
});
