import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import React from "react";
import { PasswordInput } from "../src/components/PasswordInput";
import { TwoFactorInput } from "../src/components/TwoFactorInput";

describe("Input components unit tests (PasswordInput & TwoFactorInput)", () => {
  describe("PasswordInput component", () => {
    it("renders password input without label by default", () => {
      render(<PasswordInput id="pwd" placeholder="Enter password" />);
      const input = screen.getByPlaceholderText("Enter password");
      expect(input).toHaveAttribute("type", "password");
      expect(screen.queryByText("Label")).not.toBeInTheDocument();
    });

    it("renders label when label prop is provided", () => {
      render(<PasswordInput id="pwd" label="Password Label" placeholder="Enter password" />);
      expect(screen.getByText("Password Label")).toBeInTheDocument();
    });

    it("toggles password visibility when eye icon button is clicked", () => {
      render(<PasswordInput id="pwd" placeholder="Enter password" />);
      const input = screen.getByPlaceholderText("Enter password");
      const toggleBtn = screen.getByRole("button", { name: "Show password" });

      expect(input).toHaveAttribute("type", "password");
      expect(toggleBtn).toHaveAttribute("aria-pressed", "false");

      fireEvent.click(toggleBtn);

      expect(input).toHaveAttribute("type", "text");
      const hideBtn = screen.getByRole("button", { name: "Hide password" });
      expect(hideBtn).toHaveAttribute("aria-pressed", "true");

      fireEvent.click(hideBtn);
      expect(input).toHaveAttribute("type", "password");
    });
  });

  describe("TwoFactorInput component", () => {
    it("renders 6 digit boxes and updates correctly", () => {
      const onChange = vi.fn();
      render(<TwoFactorInput value="123" onChange={onChange} />);

      const boxes = screen.getAllByText(/^[0-9]?$/).filter(
        (el) => el.className.includes("w-10 h-12")
      );
      expect(boxes.length).toBe(6);
      expect(boxes[0].textContent).toBe("1");
      expect(boxes[1].textContent).toBe("2");
      expect(boxes[2].textContent).toBe("3");
      expect(boxes[3].textContent).toBe("");

      const hiddenInput = screen.getByRole("textbox");
      fireEvent.change(hiddenInput, { target: { value: "123456" } });
      expect(onChange).toHaveBeenCalledWith("123456");
    });

    it("filters out non-digits and truncates to 6 characters", () => {
      const onChange = vi.fn();
      render(<TwoFactorInput value="" onChange={onChange} />);

      const hiddenInput = screen.getByRole("textbox");
      fireEvent.change(hiddenInput, { target: { value: "abc123xyz789" } });
      expect(onChange).toHaveBeenCalledWith("123789");
    });
  });
});
