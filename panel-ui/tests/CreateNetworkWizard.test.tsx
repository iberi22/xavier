import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { CreateNetworkWizard } from "../src/components/Mesh/CreateNetworkWizard";

describe("CreateNetworkWizard", () => {
  it("does not render when isOpen is false", () => {
    const { container } = render(
      <CreateNetworkWizard isOpen={false} onClose={vi.fn()} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders Step 1 with template choices and network name input", () => {
    render(<CreateNetworkWizard isOpen={true} onClose={vi.fn()} />);

    expect(screen.getByText("Create New Mesh Network")).toBeInTheDocument();
    expect(screen.getByText("Step 1 of 3: Name & Template")).toBeInTheDocument();
    expect(screen.getByText("Enterprise Brain")).toBeInTheDocument();
    expect(screen.getByText("SWAL DAO")).toBeInTheDocument();
    expect(screen.getByText("Family Health")).toBeInTheDocument();
  });

  it("navigates through steps 1, 2, and 3 correctly", () => {
    const handleComplete = vi.fn();
    const handleClose = vi.fn();

    render(
      <CreateNetworkWizard
        isOpen={true}
        onClose={handleClose}
        onComplete={handleComplete}
      />
    );

    // Step 1: Next should be disabled when network name is empty
    const nextBtn = screen.getByRole("button", { name: /next/i });
    expect(nextBtn).toBeDisabled();

    // Type network name
    const nameInput = screen.getByLabelText(/mesh network name/i);
    fireEvent.change(nameInput, { target: { value: "Alpha Network" } });
    expect(nextBtn).toBeEnabled();

    // Select SWAL DAO template
    const daoTemplateBtn = screen.getByRole("button", { name: /swal dao/i });
    fireEvent.click(daoTemplateBtn);

    // Move to Step 2
    fireEvent.click(nextBtn);
    expect(screen.getByText("Step 2 of 3: Node Mode & Relay")).toBeInTheDocument();

    // Step 2: Toggle to Join Bootstrap Relay mode
    const clientModeBtn = screen.getByRole("button", { name: /connect to existing bootstrap relay/i });
    fireEvent.click(clientModeBtn);

    const relayInput = screen.getByLabelText(/bootstrap relay url/i);
    fireEvent.change(relayInput, { target: { value: "https://relay.test.mesh" } });

    // Move to Step 3
    fireEvent.click(screen.getByRole("button", { name: /next/i }));
    expect(screen.getByText("Step 3 of 3: Sync & Invite")).toBeInTheDocument();
    expect(screen.getByText("Initial Sync Policies")).toBeInTheDocument();
    expect(screen.getByText("Generated Invite Code & QR")).toBeInTheDocument();

    // Finish wizard
    const finishBtn = screen.getByRole("button", { name: /finish & deploy network/i });
    fireEvent.click(finishBtn);

    expect(handleComplete).toHaveBeenCalledTimes(1);
    expect(handleComplete).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Alpha Network",
        template: "dao",
        isMasterHost: false,
        bootstrapRelayUrl: "https://relay.test.mesh",
        syncPolicy: expect.objectContaining({
          autoSync: true,
          allowPeerRelay: true,
        }),
      })
    );
    expect(handleClose).toHaveBeenCalledTimes(1);
  });

  it("copies invite code on button click in step 3", () => {
    // Mock navigator.clipboard
    const writeTextMock = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, {
      clipboard: {
        writeText: writeTextMock,
      },
    });

    render(<CreateNetworkWizard isOpen={true} onClose={vi.fn()} />);

    // Enter name & move to step 3
    fireEvent.change(screen.getByLabelText(/mesh network name/i), {
      target: { value: "SecureMesh" },
    });
    fireEvent.click(screen.getByRole("button", { name: /next/i }));
    fireEvent.click(screen.getByRole("button", { name: /next/i }));

    const copyBtn = screen.getByRole("button", { name: /copy invite code/i });
    fireEvent.click(copyBtn);

    expect(writeTextMock).toHaveBeenCalledTimes(1);
    expect(writeTextMock).toHaveBeenCalledWith(expect.stringContaining("XAVIER-MESH-ENT-SECURE-8F92"));
  });
});
