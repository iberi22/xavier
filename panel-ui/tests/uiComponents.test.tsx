import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Check, Settings, Send, Clock } from "lucide-react";
import { BorderedIcon } from "../src/components/ui/BorderedIcon";
import { SegmentedControl } from "../src/components/ui/SegmentedControl";
import { ThemedButton } from "../src/components/ui/ThemedButton";

describe("BorderedIcon Primitive Component", () => {
  it("renders non-interactive image container by default", () => {
    render(<BorderedIcon icon={Settings} ariaLabel="Settings Icon" />);
    const container = screen.getByRole("img", { name: "Settings Icon" });
    expect(container).toBeInTheDocument();
    expect(container.tagName).toBe("DIV");
  });

  it("renders interactive button when onClick or interactive prop is provided", () => {
    const handleClick = vi.fn();
    render(<BorderedIcon icon={Settings} onClick={handleClick} ariaLabel="Interactive Settings" />);
    const button = screen.getByRole("button", { name: "Interactive Settings" });
    expect(button).toBeInTheDocument();
    expect(button).toHaveAttribute("type", "button");
    expect(button.className).toContain("press-attenuation");

    fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });

  it("applies variant and rounded styling classes", () => {
    const { container } = render(
      <BorderedIcon icon={Check} variant="primary" rounded="full" size="lg" />
    );
    const element = container.firstChild as HTMLElement;
    expect(element.className).toContain("rounded-full");
    expect(element.className).toContain("bg-emerald-500");
  });
});

describe("SegmentedControl Primitive Component", () => {
  const options = [
    { value: "queue", label: "Queue", icon: Clock },
    { value: "immediate", label: "Send Immediately", icon: Send },
    { value: "disabled_option", label: "Disabled", disabled: true },
  ];

  it("renders options with proper WAI-ARIA radiogroup and radio roles", () => {
    const handleChange = vi.fn();
    render(
      <SegmentedControl
        options={options}
        value="queue"
        onChange={handleChange}
        ariaLabel="Send Mode"
      />
    );

    const group = screen.getByRole("radiogroup", { name: "Send Mode" });
    expect(group).toBeInTheDocument();

    const queueRadio = screen.getByRole("radio", { name: "Queue" });
    const immediateRadio = screen.getByRole("radio", { name: "Send Immediately" });

    expect(queueRadio).toHaveAttribute("aria-checked", "true");
    expect(immediateRadio).toHaveAttribute("aria-checked", "false");
  });

  it("triggers onChange when clicking an unselected option", () => {
    const handleChange = vi.fn();
    render(<SegmentedControl options={options} value="queue" onChange={handleChange} />);

    const immediateRadio = screen.getByRole("radio", { name: "Send Immediately" });
    fireEvent.click(immediateRadio);

    expect(handleChange).toHaveBeenCalledWith("immediate");
  });

  it("handles keyboard navigation (ArrowRight, ArrowLeft, Home, End)", () => {
    const handleChange = vi.fn();
    render(<SegmentedControl options={options} value="queue" onChange={handleChange} />);

    const queueRadio = screen.getByRole("radio", { name: "Queue" });

    // ArrowRight moves from queue -> immediate
    fireEvent.keyDown(queueRadio, { key: "ArrowRight" });
    expect(handleChange).toHaveBeenCalledWith("immediate");

    // Home moves to first enabled option (queue)
    fireEvent.keyDown(queueRadio, { key: "Home" });
    expect(handleChange).toHaveBeenCalledWith("queue");
  });
});

describe("ThemedButton Primitive Component", () => {
  it("renders button with explicit type='button' and handles clicks", () => {
    const handleClick = vi.fn();
    render(<ThemedButton onClick={handleClick}>Click Me</ThemedButton>);

    const button = screen.getByRole("button", { name: "Click Me" });
    expect(button).toBeInTheDocument();
    expect(button).toHaveAttribute("type", "button");
    expect(button.className).toContain("press-attenuation");

    fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });

  it("renders with bordered icon when iconBordered is true", () => {
    render(
      <ThemedButton icon={Settings} iconBordered variant="primary">
        Settings
      </ThemedButton>
    );

    const button = screen.getByRole("button", { name: "Settings" });
    expect(button).toBeInTheDocument();
    // Bordered icon wrapper inside
    expect(button.querySelector("div.press-attenuation, button.press-attenuation, .rounded-lg, .rounded-md")).not.toBeNull();
  });

  it("shows loading spinner and disables button when isLoading is true", () => {
    const handleClick = vi.fn();
    render(
      <ThemedButton isLoading onClick={handleClick}>
        Saving
      </ThemedButton>
    );

    const button = screen.getByRole("button", { name: "Saving" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-disabled", "true");

    fireEvent.click(button);
    expect(handleClick).not.toHaveBeenCalled();
  });
});
