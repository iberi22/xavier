import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ChatHistory from "../src/components/ChatHistory";
import type { PanelMessage } from "../src/types";

describe("ChatHistory with OpenUI", () => {
	it("renders RenderSurface for assistant message with valid openui_lang", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-1",
				role: "assistant",
				plain_text: "Here is the structured memory response.",
				openui_lang: `
          <SectionBlock title="Internal Diagnostics" description="Memory engine status">
            <Card title="Status Card">
              <TextContent content="All nodes active." />
            </Card>
          </SectionBlock>
        `,
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		// Check RenderSurface exists
		const renderSurface = screen.getByTestId("openui-render-surface");
		expect(renderSurface).toBeInTheDocument();
		expect(screen.getByText("OpenUI Render Surface")).toBeInTheDocument();
		expect(screen.getByText("Internal Diagnostics")).toBeInTheDocument();
		expect(screen.getByText("All nodes active.")).toBeInTheDocument();

		// Check message-card and assistant-card classes
		const assistantCard = document.querySelector(".assistant-card");
		expect(assistantCard).toBeInTheDocument();

		// Check plain-text container
		const plainText = document.querySelector(".plain-text");
		expect(plainText).toBeInTheDocument();
		expect(plainText?.textContent).toContain(
			"Here is the structured memory response.",
		);
	});

	it("falls back to plain_text when openui_lang is null or undefined", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-2",
				role: "assistant",
				plain_text: "Standard plain text answer without OpenUI.",
				openui_lang: null,
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		// No RenderSurface
		expect(screen.queryByTestId("openui-render-surface")).toBeNull();

		// Plain text is rendered
		expect(
			screen.getByText("Standard plain text answer without OpenUI."),
		).toBeInTheDocument();
		const assistantCard = document.querySelector(".assistant-card");
		expect(assistantCard).toBeInTheDocument();
	});

	it("falls back to plain_text when openui_lang is malformed", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-3",
				role: "assistant",
				plain_text: "Fallback due to malformed XML.",
				openui_lang: "<SectionBlock><Card>unclosed tag without ending",
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		// Malformed openui_lang fails safe to null, no RenderSurface
		expect(screen.queryByTestId("openui-render-surface")).toBeNull();

		// Plain text fallback is rendered
		expect(
			screen.getByText("Fallback due to malformed XML."),
		).toBeInTheDocument();
	});

	it("renders user messages without RenderSurface", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-4",
				role: "user",
				plain_text: "Explain how memory works",
				openui_lang:
					'<Card title="Ignored"><TextContent content="Ignored" /></Card>',
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		expect(screen.queryByTestId("openui-render-surface")).toBeNull();
		expect(screen.getByText("Explain how memory works")).toBeInTheDocument();
		const userCard = document.querySelector(".user-card");
		expect(userCard).toBeInTheDocument();
	});

	it("renders without crashing when JSON item fields contain non-string values (coerced)", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-coerced",
				role: "assistant",
				plain_text: "Summary of signals",
				openui_lang: `
          <ListBlock items='[{"title":{"x":1},"description":999,"meta":true}]' />
        `,
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		expect(screen.getByTestId("openui-render-surface")).toBeInTheDocument();
		expect(screen.getByText("[object Object]")).toBeInTheDocument();
		expect(screen.getByText("999")).toBeInTheDocument();
		expect(screen.getByText("true")).toBeInTheDocument();
	});

	it("shows streaming plain-text fallback for assistant message while streaming is active", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-streaming",
				role: "assistant",
				plain_text: "Streaming text message in progress...",
				openui_lang: `
          <SectionBlock title="Streaming Section">
            <TextContent content="Should be hidden while streaming." />
          </SectionBlock>
        `,
				created_at: new Date().toISOString(),
			},
		];

		render(
			<ChatHistory
				messages={messages}
				streamingMessageId="msg-streaming"
			/>,
		);

		// RenderSurface should not be present while streaming
		expect(screen.queryByTestId("openui-render-surface")).toBeNull();
		expect(
			screen.queryByText("Should be hidden while streaming."),
		).toBeNull();
		// Plain text / streaming renderer is visible
		expect(document.querySelector(".plain-text")).toBeInTheDocument();
	});

	it("falls back to plain text when openui_lang has no visible nodes", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-empty-list",
				role: "assistant",
				plain_text: "Fallback for empty ListBlock",
				openui_lang: '<ListBlock items="[]" />',
				created_at: new Date().toISOString(),
			},
			{
				id: "msg-empty-accordion",
				role: "assistant",
				plain_text: "Fallback for empty Accordion",
				openui_lang: '<Accordion items="[]" />',
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		expect(screen.queryByTestId("openui-render-surface")).toBeNull();
		expect(
			screen.getByText("Fallback for empty ListBlock"),
		).toBeInTheDocument();
		expect(
			screen.getByText("Fallback for empty Accordion"),
		).toBeInTheDocument();
	});

	it("falls back to plain text when openui_lang has unescaped '<' in text", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-unescaped-lt",
				role: "assistant",
				plain_text: "Plain answer with comparison: 5 < 10",
				openui_lang: '<Card title="Comparison">5 < 10 without closing</Card>',
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		expect(screen.queryByTestId("openui-render-surface")).toBeNull();
		expect(
			screen.getByText("Plain answer with comparison: 5 < 10"),
		).toBeInTheDocument();
	});

	it("safely handles unescaped '&' in text and renders surface", () => {
		const messages: PanelMessage[] = [
			{
				id: "msg-unescaped-amp",
				role: "assistant",
				plain_text: "Plain text with & ampersands",
				openui_lang: `
          <Card title="Fish & Chips">
            <TextContent content="Tom & Jerry & Co" />
          </Card>
        `,
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		expect(screen.getByTestId("openui-render-surface")).toBeInTheDocument();
		expect(screen.getByText("Fish & Chips")).toBeInTheDocument();
		expect(screen.getByText("Tom & Jerry & Co")).toBeInTheDocument();
	});

	it("falls back to plain-text renderer when RenderSurface throws an error", async () => {
		const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
		const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

		const RenderSurfaceModule = await import(
			"../src/components/OpenUi/RenderSurface"
		);
		const spy = vi
			.spyOn(RenderSurfaceModule, "RenderSurface")
			.mockImplementation(() => {
				throw new Error("RenderSurface crashed unexpectedly");
			});

		const messages: PanelMessage[] = [
			{
				id: "msg-crash",
				role: "assistant",
				plain_text: "Plain text fallback after RenderSurface crash",
				openui_lang:
					'<Card title="Will Crash"><TextContent content="Crash me" /></Card>',
				created_at: new Date().toISOString(),
			},
		];

		render(<ChatHistory messages={messages} streamingMessageId={null} />);

		// RenderSurface should not be visible
		expect(screen.queryByTestId("openui-render-surface")).toBeNull();
		// Plain text fallback should be shown
		expect(
			screen.getByText("Plain text fallback after RenderSurface crash"),
		).toBeInTheDocument();

		spy.mockRestore();
		consoleSpy.mockRestore();
		warnSpy.mockRestore();
	});
});
