import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { IntrospectionTab } from "./IntrospectionTab";

const challenge = {
  id: "hc_1",
  session_id: "s",
  challenge_type: "decision",
  description: "Pick a DB",
  raw_content: "raw",
  confidence_score: 0.8,
  status: "candidate",
  created_at: "2026-01-01T00:00:00Z",
};

const session = (turns: unknown[], status = "active") => ({
  id: "is_1",
  challenge_id: "hc_1",
  technique: "pre_mortem",
  turns,
  depth_score: 0.2,
  insights: status === "completed" ? ["insight one"] : [],
  status,
  started_at: "2026-01-01T00:00:00Z",
  completed_at: null,
});
const guide = { role: "llm_guide", content: "What could fail?", timestamp: "t" };
const human = { role: "human", content: "Latency", timestamp: "t" };
const guide2 = { role: "llm_guide", content: "Why latency?", timestamp: "t" };

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

afterEach(() => vi.unstubAllGlobals());

describe("IntrospectionTab", () => {
  it("shows the empty state when nothing is available", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ techniques: ["five_whys"] })));
    render(<IntrospectionTab />);
    expect(await screen.findByText(/No hay sesiones disponibles/)).toBeTruthy();
    expect(screen.getByText("5 Porqués")).toBeTruthy();
  });

  it("shows an error with retry when loading fails", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ error: "boom" }, 500)));
    render(<IntrospectionTab />);
    expect(await screen.findByText("boom")).toBeTruthy();
    expect(screen.getByText("Reintentar")).toBeTruthy();
  });

  it("runs start -> turn -> complete against the backend routes", async () => {
    const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/v1/maloca/introspection/available")
        return json({ techniques: ["pre_mortem"], challenges: [challenge] });
      if (url === "/v1/maloca/introspection/start") return json({ session: session([guide]) }, 201);
      if (url === "/v1/maloca/introspection/is_1/turn")
        return json({ session: session([guide, human, guide2]) });
      if (url === "/v1/maloca/introspection/is_1/complete")
        return json({ session: session([guide, human, guide2], "completed") });
      return json({ error: `unexpected ${url} ${init?.method}` }, 404);
    });
    vi.stubGlobal("fetch", fetchMock);
    window.HTMLElement.prototype.scrollIntoView = vi.fn();

    render(<IntrospectionTab />);
    fireEvent.click(await screen.findByText("Entrar"));
    expect(await screen.findByText("What could fail?")).toBeTruthy();

    fireEvent.change(screen.getByPlaceholderText(/Escribe tu respuesta/), { target: { value: "Latency" } });
    fireEvent.keyDown(screen.getByPlaceholderText(/Escribe tu respuesta/), { key: "Enter" });
    expect(await screen.findByText("Why latency?")).toBeTruthy();

    fireEvent.click(screen.getByText("Completar"));
    expect(await screen.findByText("insight one")).toBeTruthy();

    const startCall = fetchMock.mock.calls.find((c) => c[0].endsWith("/start"))!;
    expect(JSON.parse(startCall[1]!.body as string)).toEqual({
      challenge_id: "hc_1",
      challenge_type: "decision",
      description: "Pick a DB",
      technique: "pre_mortem",
    });
    const turnCall = fetchMock.mock.calls.find((c) => c[0].endsWith("/turn"))!;
    expect(JSON.parse(turnCall[1]!.body as string)).toEqual({
      human_input: "Latency",
      challenge_description: "Pick a DB",
    });
    await waitFor(() => expect(screen.getByText(/Sesión completada/)).toBeTruthy());
  });
});
