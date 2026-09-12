import { fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import { ThemeProvider } from "../src/lib/theme/theme-provider";
import AppearancePage from "../src/pages/Settings/Appearance";

describe("AppearancePage Component", () => {
	it("renders header and all section titles", () => {
		render(
			<ThemeProvider>
				<AppearancePage />
			</ThemeProvider>,
		);

		expect(screen.getByText(/Appearance \/ Aspecto/i)).toBeInTheDocument();
		expect(
			screen.getByText(/Customize theme, typography, and visual ergonomics/i),
		).toBeInTheDocument();
		expect(screen.getByText("Theme Preset")).toBeInTheDocument();
		expect(screen.getByText("Typography")).toBeInTheDocument();
		expect(screen.getByText("Micro-interactions & Borders")).toBeInTheDocument();
		expect(screen.getByText("Interactive Preview")).toBeInTheDocument();
	});

	it("renders all four theme preset cards", () => {
		render(
			<ThemeProvider>
				<AppearancePage />
			</ThemeProvider>,
		);

		expect(screen.getByText("Studio Dark")).toBeInTheDocument();
		expect(screen.getByText("Bone White")).toBeInTheDocument();
		expect(screen.getByText("Xavier Cyberpunk")).toBeInTheDocument();
		expect(screen.getByText("System")).toBeInTheDocument();
	});

	it("switches theme preset when card is clicked", () => {
		render(
			<ThemeProvider>
				<AppearancePage />
			</ThemeProvider>,
		);

		const boneCard = screen.getByText("Bone White").closest("button");
		expect(boneCard).not.toBeNull();
		if (boneCard) {
			fireEvent.click(boneCard);
			expect(localStorage.getItem("xavier_theme_preset")).toBe("studio-bone");
		}

		const cyberpunkCard = screen.getByText("Xavier Cyberpunk").closest("button");
		expect(cyberpunkCard).not.toBeNull();
		if (cyberpunkCard) {
			fireEvent.click(cyberpunkCard);
			expect(localStorage.getItem("xavier_theme_preset")).toBe("cyberpunk");
		}
	});

	it("toggles typography style", () => {
		render(
			<ThemeProvider>
				<AppearancePage />
			</ThemeProvider>,
		);

		const monoOption = screen.getByText("Monospace Typography").closest("button");
		expect(monoOption).not.toBeNull();
		if (monoOption) {
			fireEvent.click(monoOption);
			expect(localStorage.getItem("xavier_typography")).toBe("mono");
		}

		const sansOption = screen.getByText("Native System Font").closest("button");
		expect(sansOption).not.toBeNull();
		if (sansOption) {
			fireEvent.click(sansOption);
			expect(localStorage.getItem("xavier_typography")).toBe("sans");
		}
	});

	it("toggles micro-interaction switches", () => {
		render(
			<ThemeProvider>
				<AppearancePage />
			</ThemeProvider>,
		);

		const borderedIconsSwitch = screen.getByRole("switch", {
			name: /Toggle Outlined or Bordered Icons/i,
		});
		fireEvent.click(borderedIconsSwitch);
		expect(localStorage.getItem("xavier_bordered_icons")).toBe("false");

		const hoverSwitch = screen.getByRole("switch", {
			name: /Toggle Press and Hover Attenuation/i,
		});
		fireEvent.click(hoverSwitch);
		expect(localStorage.getItem("xavier_hover_attenuation")).toBe("true");
	});
});
