import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { RoadmapTab } from "./RoadmapTab";

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

const feature = (id: string, status: string, progress_pct: number) => ({
  id,
  name: `Feature ${id}`,
  app_id: "xavier",
  status,
  progress_pct,
});

// Deliberately out of order: the tab must render waves ascending.
const roadmap = {
  waves: [
    {
      wave: 3,
      total: 2,
      stable: 1,
      in_progress: 1,
      planned: 0,
      progress_pct: 50,
      features: [feature("F-3a", "stable", 100), feature("F-3b", "in_progress", 0)],
    },
    {
      wave: 1,
      total: 1,
      stable: 1,
      in_progress: 0,
      planned: 0,
      progress_pct: 100,
      features: [feature("F-1a", "stable", 100)],
    },
  ],
  unscheduled: {
    total: 1,
    stable: 0,
    in_progress: 0,
    planned: 1,
    progress_pct: 0,
    features: [feature("F-u", "planned", 0)],
  },
};

describe("RoadmapTab", () => {
  it("renders waves in ascending order, then Unscheduled, with their counts", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json(roadmap)));
    render(<RoadmapTab />);

    await screen.findByRole("region", { name: "Wave 1" });
    const cards = screen.getAllByRole("region").map((el) => el.getAttribute("aria-label"));
    expect(cards).toEqual(["Wave 1", "Wave 3", "Unscheduled"]);

    const wave3 = screen.getByRole("region", { name: "Wave 3" });
    expect(within(wave3).getByText("2 total")).toBeTruthy();
    expect(within(wave3).getByText("1 stable")).toBeTruthy();
    expect(within(wave3).getByText("1 in progress")).toBeTruthy();
    expect(within(wave3).getByText("0 planned")).toBeTruthy();
    expect(within(wave3).getByText("50%")).toBeTruthy();
  });

  it("expands a wave to list its features", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json(roadmap)));
    render(<RoadmapTab />);

    const wave3 = await screen.findByRole("region", { name: "Wave 3" });
    expect(within(wave3).queryByText("Feature F-3b")).toBeNull();
    const toggle = within(wave3).getByRole("button", { name: "Show 2 features" });
    fireEvent.click(toggle);
    expect(within(wave3).getByText("Feature F-3b")).toBeTruthy();
  });

  it("shows the empty state when the roadmap has no features", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        json({
          waves: [],
          unscheduled: { total: 0, stable: 0, in_progress: 0, planned: 0, progress_pct: 0, features: [] },
        }),
      ),
    );
    render(<RoadmapTab />);
    expect(await screen.findByText("No roadmap features are available yet.")).toBeTruthy();
    expect(screen.queryByRole("region")).toBeNull();
  });

  it("shows an error message when loading fails", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ error: "backlog offline" }, 500)));
    render(<RoadmapTab />);
    expect(await screen.findByText(/Failed to fetch roadmap: backlog offline/)).toBeTruthy();
  });
});
