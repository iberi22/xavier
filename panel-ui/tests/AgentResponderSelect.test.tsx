import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
	type AgentResponderOption,
	AgentResponderSelect,
} from "../src/components/Telecom/AgentResponderSelect";

describe("AgentResponderSelect", () => {
	it("renders with default label, active status badge, and initial agent selection", () => {
		render(<AgentResponderSelect />);

		expect(
			screen.getByText("Automated Cognitive Responder"),
		).toBeInTheDocument();
		expect(screen.getByText("Active")).toBeInTheDocument();
		expect(screen.getByText("Legal & Compliance Sentinel")).toBeInTheDocument();
		expect(screen.getByText("claude-3-5-sonnet")).toBeInTheDocument();
		expect(screen.getByText("Active Capabilities")).toBeInTheDocument();
		expect(screen.getByText("Contracts")).toBeInTheDocument();
	});

	it("toggles automated responder master switch on and off", () => {
		const handleToggleEnabled = vi.fn();
		render(
			<AgentResponderSelect
				enabled={true}
				onToggleEnabled={handleToggleEnabled}
			/>,
		);

		const masterSwitch = screen.getByRole("switch", {
			name: /toggle automated bot response/i,
		});
		expect(masterSwitch).toHaveAttribute("aria-checked", "true");

		fireEvent.click(masterSwitch);
		expect(handleToggleEnabled).toHaveBeenCalledWith(false);
	});

	it("opens dropdown menu on trigger click and lists all local agents", () => {
		render(<AgentResponderSelect />);

		const triggerButton = screen.getByRole("button", {
			name: /select cognitive agent responder/i,
		});
		expect(triggerButton).toHaveAttribute("aria-expanded", "false");

		fireEvent.click(triggerButton);
		expect(triggerButton).toHaveAttribute("aria-expanded", "true");

		expect(
			screen.getByRole("listbox", { name: /available cognitive agents/i }),
		).toBeInTheDocument();

		expect(screen.getByText("Sentinel Threat Monitor")).toBeInTheDocument();
		expect(
			screen.getByText("Research & Synthesis Copilot"),
		).toBeInTheDocument();
		expect(screen.getByText("Ops & Infrastructure Agent")).toBeInTheDocument();
	});

	it("selects a different agent from dropdown and fires onSelectAgent callback", () => {
		const handleSelectAgent = vi.fn();
		render(<AgentResponderSelect onSelectAgent={handleSelectAgent} />);

		const triggerButton = screen.getByRole("button", {
			name: /select cognitive agent responder/i,
		});
		fireEvent.click(triggerButton);

		const sentinelOption = screen.getByText("Sentinel Threat Monitor");
		fireEvent.click(sentinelOption);

		expect(handleSelectAgent).toHaveBeenCalledWith(
			expect.objectContaining({
				id: "sentinel-ai",
				name: "Sentinel Threat Monitor",
			}),
		);

		expect(triggerButton).toHaveAttribute("aria-expanded", "false");
		expect(screen.getByText("Threat Detection")).toBeInTheDocument();
	});

	it("allows selecting response dispatch policy and calls onChangeDispatchPolicy", () => {
		const handleChangeDispatchPolicy = vi.fn();
		render(
			<AgentResponderSelect
				onChangeDispatchPolicy={handleChangeDispatchPolicy}
			/>,
		);

		const mentionPolicyBtn = screen.getByRole("button", {
			name: /@mention only responds only when explicitly tagged/i,
		});
		fireEvent.click(mentionPolicyBtn);

		expect(handleChangeDispatchPolicy).toHaveBeenCalledWith("MentionOnly");
	});

	it("closes dropdown when Escape key is pressed", () => {
		render(<AgentResponderSelect />);

		const triggerButton = screen.getByRole("button", {
			name: /select cognitive agent responder/i,
		});
		fireEvent.click(triggerButton);
		expect(triggerButton).toHaveAttribute("aria-expanded", "true");

		fireEvent.keyDown(triggerButton, { key: "Escape" });
		expect(triggerButton).toHaveAttribute("aria-expanded", "false");
	});

	it("handles custom agents list correctly", () => {
		const customAgents: AgentResponderOption[] = [
			{
				id: "custom-1",
				name: "Custom Audit Bot",
				role: "Auditor",
				model: "custom-llm",
				description: "Custom internal auditor",
				capabilities: ["Custom Check"],
			},
		];

		render(
			<AgentResponderSelect agents={customAgents} selectedAgentId="custom-1" />,
		);

		expect(screen.getByText("Custom Audit Bot")).toBeInTheDocument();
		expect(screen.getByText("Custom Check")).toBeInTheDocument();
	});

	it("disables component when disabled prop is true", () => {
		render(<AgentResponderSelect disabled={true} />);

		const masterSwitch = screen.getByRole("switch");
		expect(masterSwitch).toBeDisabled();

		const triggerButton = screen.getByRole("button", {
			name: /select cognitive agent responder/i,
		});
		expect(triggerButton).toBeDisabled();
	});
});
