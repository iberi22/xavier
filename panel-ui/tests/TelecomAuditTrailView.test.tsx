import { fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import {
	AuditEvent,
	AuditEventItem,
	TelecomAuditTrailView,
} from "../src/components/Telecom/TelecomAuditTrailView";

const TEST_EVENTS: AuditEvent[] = [
	{
		id: "test-evt-01",
		timestamp: "2026-03-20T10:00:00Z",
		eventType: "wallet_signature_validation",
		severity: "info",
		participantId: "alice-wallet-pubkey",
		channelId: "room-alpha",
		clearanceLevel: "TopSecret",
		action: "verify_wallet_signature",
		outcome: "VERIFIED",
		details: "Signature check passed for challenge.",
		signaturePreview: "0x1234567890abcdef",
	},
	{
		id: "test-evt-02",
		timestamp: "2026-03-20T10:05:00Z",
		eventType: "blocked_attempt",
		severity: "blocked",
		participantId: "unauthorized-bob",
		channelId: "room-alpha",
		clearanceLevel: "Confidential",
		action: "authorize_join",
		outcome: "BLOCKED",
		details: "Join attempt blocked due to low clearance.",
	},
	{
		id: "test-evt-03",
		timestamp: "2026-03-20T10:10:00Z",
		eventType: "clearance_check",
		severity: "info",
		participantId: "charlie-agent",
		channelId: "room-beta",
		clearanceLevel: "Secret",
		action: "authorize_transmit",
		outcome: "GRANTED",
		details: "Transmission clearance approved.",
	},
];

describe("TelecomAuditTrailView", () => {
	it("renders header title and security metrics summary", () => {
		render(<TelecomAuditTrailView initialEvents={TEST_EVENTS} />);

		expect(
			screen.getByText(/Telecom Clearance & Security Audit Trail/i),
		).toBeInTheDocument();
		expect(screen.getByText("Total Events")).toBeInTheDocument();
		expect(screen.getAllByText("Clearance Checks").length).toBeGreaterThanOrEqual(1);
		expect(screen.getByText("Signature Validations")).toBeInTheDocument();
		expect(screen.getByText("Blocked Attempts")).toBeInTheDocument();
	});

	it("renders timeline with role='log' and aria-live='polite'", () => {
		render(<TelecomAuditTrailView initialEvents={TEST_EVENTS} />);

		const logContainer = screen.getByRole("log");
		expect(logContainer).toBeInTheDocument();
		expect(logContainer).toHaveAttribute("aria-live", "polite");
	});

	it("renders individual AuditEventItem with severity and outcome badge", () => {
		render(
			<AuditEventItem
				event={TEST_EVENTS[0]}
				onSelectEvent={vi.fn()}
			/>,
		);

		expect(screen.getByText("VERIFIED")).toBeInTheDocument();
		expect(screen.getByText("TopSecret")).toBeInTheDocument();
		expect(screen.getByText("verify_wallet_signature")).toBeInTheDocument();
		expect(screen.getByText("Signature check passed for challenge.")).toBeInTheDocument();
	});

	it("filters timeline events by category pill selection", () => {
		render(<TelecomAuditTrailView initialEvents={TEST_EVENTS} />);

		// Click Blocked pill
		const blockedPill = screen.getByRole("button", { name: /Blocked \(1\)/i });
		fireEvent.click(blockedPill);

		expect(screen.getByText("Join attempt blocked due to low clearance.")).toBeInTheDocument();
		expect(screen.queryByText("Signature check passed for challenge.")).not.toBeInTheDocument();
		expect(screen.queryByText("Transmission clearance approved.")).not.toBeInTheDocument();
	});

	it("filters timeline events by search input query", () => {
		render(<TelecomAuditTrailView initialEvents={TEST_EVENTS} />);

		const searchInput = screen.getByPlaceholderText(
			/Filter by participant, channel, action, or details/i,
		);
		fireEvent.change(searchInput, { target: { value: "charlie-agent" } });

		expect(screen.getByText("Transmission clearance approved.")).toBeInTheDocument();
		expect(screen.queryByText("Signature check passed for challenge.")).not.toBeInTheDocument();
	});

	it("opens and closes event inspector drawer upon selecting event", () => {
		render(<TelecomAuditTrailView initialEvents={TEST_EVENTS} />);

		const inspectButtons = screen.getAllByRole("button", {
			name: /View security audit event details/i,
		});
		fireEvent.click(inspectButtons[0]);

		expect(screen.getByText(/Event Inspector \(test-evt-01\)/i)).toBeInTheDocument();
		expect(screen.getByText("0x1234567890abcdef")).toBeInTheDocument();

		const closeButton = screen.getByRole("button", {
			name: /Close event inspector/i,
		});
		fireEvent.click(closeButton);

		expect(screen.queryByText(/Event Inspector \(test-evt-01\)/i)).not.toBeInTheDocument();
	});

	it("triggers onClose callback when back button is clicked", () => {
		const handleClose = vi.fn();
		render(
			<TelecomAuditTrailView
				initialEvents={TEST_EVENTS}
				onClose={handleClose}
			/>,
		);

		const backButton = screen.getByRole("button", {
			name: /Back to previous view/i,
		});
		fireEvent.click(backButton);

		expect(handleClose).toHaveBeenCalledTimes(1);
	});
});
