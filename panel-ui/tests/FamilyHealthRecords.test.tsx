import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { describe, expect, it } from "vitest";
import FamilyHealthRecords, {
  AccessToken,
  MedicalEpisode,
} from "../src/components/Mesh/FamilyHealthRecords";

const MOCK_EPISODES: MedicalEpisode[] = [
  {
    id: "ep-test-1",
    familyMember: "Alice Smith",
    date: "2026-03-01",
    diagnosis: "Acute Bronchitis",
    doctor: "Dr. Evelyn Reed",
    severity: "medium",
    notes: "Prescribed 7-day amoxicillin course.",
    attachments: [
      { id: "att-1", name: "Chest_XRay.pdf", fileSize: "2.4 MB", type: "application/pdf" },
    ],
  },
  {
    id: "ep-test-2",
    familyMember: "Bob Smith",
    date: "2026-02-14",
    diagnosis: "Hypertension Checkup",
    doctor: "Dr. Marcus Vance",
    severity: "low",
    notes: "BP stable at 122/80.",
    attachments: [
      { id: "att-2", name: "ECG_Telemetry.pdf", fileSize: "3.8 MB", type: "application/pdf" },
    ],
  },
];

const MOCK_TOKENS: AccessToken[] = [
  {
    id: "tok-test-1",
    token: "pass_live_12345",
    recipientDoctor: "Dr. Evelyn Reed",
    passType: "1-hour",
    scope: "Alice Smith - All Records",
    createdAt: "2026-03-06 10:00:00",
    expiresAt: "2026-03-06 11:00:00",
    status: "active",
  },
];

describe("FamilyHealthRecords Component", () => {
  it("renders header, security warning, and family medical records", () => {
    render(
      <FamilyHealthRecords
        initialEpisodes={MOCK_EPISODES}
        initialTokens={MOCK_TOKENS}
      />
    );

    expect(
      screen.getByText("Family Health Records Manager")
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Security & Compliance Warning: Sensitive Medical Information/i)
    ).toBeInTheDocument();
    expect(screen.getByText("Acute Bronchitis")).toBeInTheDocument();
    expect(screen.getByText("Hypertension Checkup")).toBeInTheDocument();
    expect(screen.getByText("Chest_XRay.pdf")).toBeInTheDocument();
  });

  it("filters medical records by family member", () => {
    render(
      <FamilyHealthRecords
        initialEpisodes={MOCK_EPISODES}
        initialTokens={MOCK_TOKENS}
      />
    );

    // Click on 'Alice Smith' filter tab
    const aliceButton = screen.getByRole("button", { name: "Alice Smith" });
    fireEvent.click(aliceButton);

    expect(screen.getByText("Acute Bronchitis")).toBeInTheDocument();
    expect(screen.queryByText("Hypertension Checkup")).not.toBeInTheDocument();

    // Reset filter to 'All'
    const allButton = screen.getByRole("button", { name: "All" });
    fireEvent.click(allButton);

    expect(screen.getByText("Acute Bronchitis")).toBeInTheDocument();
    expect(screen.getByText("Hypertension Checkup")).toBeInTheDocument();
  });

  it("opens share modal with security warning notice", () => {
    render(
      <FamilyHealthRecords
        initialEpisodes={MOCK_EPISODES}
        initialTokens={MOCK_TOKENS}
      />
    );

    const shareButton = screen.getByRole("button", { name: "Share with Doctor" });
    fireEvent.click(shareButton);

    expect(screen.getByText("Generate Doctor Share Pass")).toBeInTheDocument();
    expect(
      screen.getByText(/SECURITY WARNING: PHI Data Transfer/i)
    ).toBeInTheDocument();
    expect(screen.getByLabelText(/Attending Physician \/ Doctor Name:/i)).toBeInTheDocument();
  });

  it("generates a 1-hour time-locked doctor share pass and QR code", async () => {
    render(
      <FamilyHealthRecords
        initialEpisodes={MOCK_EPISODES}
        initialTokens={MOCK_TOKENS}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Share with Doctor" }));

    const doctorInput = screen.getByLabelText(/Attending Physician \/ Doctor Name:/i);
    fireEvent.change(doctorInput, { target: { value: "Dr. Gregory House" } });

    const form = doctorInput.closest("form");
    expect(form).not.toBeNull();
    if (form) {
      fireEvent.submit(form);
    }

    await waitFor(() => {
      expect(screen.getByText("Pass Created Successfully")).toBeInTheDocument();
      expect(screen.getAllByText(/Dr. Gregory House/i).length).toBeGreaterThan(0);
    });

    // Close modal
    fireEvent.click(screen.getByRole("button", { name: "Done" }));

    expect(screen.getByText("Dr. Gregory House")).toBeInTheDocument();
  });

  it("generates read-once pass and revokes access token", async () => {
    render(
      <FamilyHealthRecords
        initialEpisodes={MOCK_EPISODES}
        initialTokens={MOCK_TOKENS}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Share with Doctor" }));

    const doctorInput = screen.getByLabelText(/Attending Physician \/ Doctor Name:/i);
    fireEvent.change(doctorInput, { target: { value: "Dr. John Watson" } });

    fireEvent.click(screen.getByRole("button", { name: /Read-Once Pass/i }));

    const form = doctorInput.closest("form");
    expect(form).not.toBeNull();
    if (form) {
      fireEvent.submit(form);
    }

    await waitFor(() => {
      expect(screen.getAllByText(/Dr. John Watson/i).length).toBeGreaterThan(0);
    });

    fireEvent.click(screen.getByRole("button", { name: "Done" }));

    // Revoke token pass_live_12345
    const revokeBtn = screen.getByRole("button", { name: "Revoke token pass_live_12345" });
    fireEvent.click(revokeBtn);

    expect(screen.getByText("Revocation & Expiration History (1)")).toBeInTheDocument();
  });
});
