import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RenderSurface } from "../src/components/OpenUi/RenderSurface";
import type { OpenUiNode } from "../src/utils/openuiParser";

describe("RenderSurface", () => {
	it("renders container with render-surface class and data-testid='openui-render-surface'", () => {
		const nodes: OpenUiNode[] = [
			{
				id: "n-1",
				type: "TextContent",
				content: "Hello RenderSurface",
			},
		];

		render(<RenderSurface nodes={nodes} />);

		const surface = screen.getByTestId("openui-render-surface");
		expect(surface).toBeInTheDocument();
		expect(surface.className).toContain("render-surface");
		expect(screen.getByText("OpenUI Render Surface")).toBeVisible();
		expect(screen.getByText("Hello RenderSurface")).toBeInTheDocument();
	});

	it("returns null for empty or null nodes", () => {
		const { container: c1 } = render(<RenderSurface nodes={[]} />);
		expect(c1.firstChild).toBeNull();

		// @ts-expect-error testing invalid prop
		const { container: c2 } = render(<RenderSurface nodes={null} />);
		expect(c2.firstChild).toBeNull();
	});

	it("renders SectionBlock and Card hierarchy", () => {
		const nodes: OpenUiNode[] = [
			{
				id: "sec-1",
				type: "SectionBlock",
				title: "Test Section",
				description: "Section Description",
				children: [
					{
						id: "card-1",
						type: "Card",
						title: "Inner Card",
						description: "Card Description",
						children: [
							{
								id: "text-1",
								type: "TextContent",
								content: "Content inside card",
							},
						],
					},
				],
			},
		];

		render(<RenderSurface nodes={nodes} />);

		expect(screen.getByText("Test Section")).toBeInTheDocument();
		expect(screen.getByText("Section Description")).toBeInTheDocument();
		expect(screen.getByText("Inner Card")).toBeInTheDocument();
		expect(screen.getByText("Card Description")).toBeInTheDocument();
		expect(screen.getByText("Content inside card")).toBeInTheDocument();
	});

	it("renders ListBlock with items and meta tags", () => {
		const nodes: OpenUiNode[] = [
			{
				id: "list-1",
				type: "ListBlock",
				items: [
					{
						id: "item-1",
						title: "System Signal 1",
						description: "Latency normal",
						meta: "24ms",
					},
					{
						id: "item-2",
						title: "System Signal 2",
						description: "Confidence high",
						meta: "98%",
					},
				],
			},
		];

		render(<RenderSurface nodes={nodes} />);

		expect(screen.getByText("System Signal 1")).toBeInTheDocument();
		expect(screen.getByText("Latency normal")).toBeInTheDocument();
		expect(screen.getByText("24ms")).toBeInTheDocument();
		expect(screen.getByText("System Signal 2")).toBeInTheDocument();
		expect(screen.getByText("98%")).toBeInTheDocument();
	});

	it("renders Accordion and toggles content on user click", () => {
		const nodes: OpenUiNode[] = [
			{
				id: "acc-1",
				type: "Accordion",
				items: [
					{
						id: "acc-item-1",
						title: "Memory Document 1",
						content: "Detailed body of document 1",
					},
				],
			},
		];

		render(<RenderSurface nodes={nodes} />);

		const toggleBtn = screen.getByText("Memory Document 1");
		expect(toggleBtn).toBeInTheDocument();

		// Initially collapsed
		expect(screen.queryByText("Detailed body of document 1")).toBeNull();

		// Click to expand
		fireEvent.click(toggleBtn);
		expect(screen.getByText("Detailed body of document 1")).toBeVisible();

		// Click to collapse
		fireEvent.click(toggleBtn);
		expect(screen.queryByText("Detailed body of document 1")).toBeNull();
	});

	it("renders specialized cards (QuestionCard, DecisionCard, ProjectCard, InfoCard)", () => {
		const nodes: OpenUiNode[] = [
			{
				id: "qc-1",
				type: "QuestionCard",
				question: "Is hybrid search enabled?",
			},
			{
				id: "dc-1",
				type: "DecisionCard",
				adr: "ADR-012: SQLite Vec extension",
			},
			{
				id: "pc-1",
				type: "ProjectCard",
				project: "xavier-agent-core",
			},
			{
				id: "ic-1",
				type: "InfoCard",
				title: "Active State",
				value: "Operational",
			},
		];

		render(<RenderSurface nodes={nodes} />);

		expect(screen.getByText("Is hybrid search enabled?")).toBeInTheDocument();
		expect(
			screen.getByText("ADR-012: SQLite Vec extension"),
		).toBeInTheDocument();
		expect(screen.getByText("xavier-agent-core")).toBeInTheDocument();
		expect(screen.getByText("Active State")).toBeInTheDocument();
		expect(screen.getByText("Operational")).toBeInTheDocument();
	});

	it("renders TextCallout with different variants", () => {
		const nodes: OpenUiNode[] = [
			{
				id: "tc-1",
				type: "TextCallout",
				variant: "warning",
				title: "Warning Title",
				content: "Warning description text",
			},
			{
				id: "tc-2",
				type: "TextCallout",
				variant: "success",
				title: "Success Title",
				content: "Success description text",
			},
		];

		render(<RenderSurface nodes={nodes} />);

		expect(screen.getByText("Warning Title")).toBeInTheDocument();
		expect(screen.getByText("Warning description text")).toBeInTheDocument();
		expect(screen.getByText("Success Title")).toBeInTheDocument();
		expect(screen.getByText("Success description text")).toBeInTheDocument();
	});

	it("renders script injection attempt safely as inert text with no script element in DOM", () => {
		const nodes: OpenUiNode[] = [
			{
				id: "inj-1",
				type: "text",
				content: "<script>window.__injected = true;</script>",
			},
		];

		const { container } = render(<RenderSurface nodes={nodes} />);

		expect(
			screen.getByText("<script>window.__injected = true;</script>"),
		).toBeInTheDocument();
		expect(container.querySelector("script")).toBeNull();
		expect(document.querySelector("script")).toBeNull();
		expect(
			(window as unknown as Record<string, unknown>).__injected,
		).toBeUndefined();
	});
});
