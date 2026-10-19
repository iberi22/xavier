import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { MarketplaceView } from "./MarketplaceView";
import { MarketplaceStatusCard } from "../../components/MarketplaceStatusCard";
import MalocaView from "../MalocaView";

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

afterEach(() => vi.unstubAllGlobals());

describe("MarketplaceView and MarketplaceStatusCard", () => {
  const mockListing = {
    id: "list_bio_1",
    seller_id: "seller_node_alpha",
    title: "Biology Exam Questions",
    description: "Curated dataset for biology",
    price_credits: 25,
    license: "cc-by-4.0",
    terms_version: "v1.0",
    created_at_ms: 1000,
    revoked_at_ms: null,
  };

  const mockConsent = [
    {
      node_id: "local-node",
      state: "shared",
      terms_version: "v1.0",
      consent_version: "v1.0",
      updated_at_ms: 2000,
    },
  ];

  it("renders buyer catalogue and executes buy flow", async () => {
    const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes("/v2/marketplace/listings") && (!init || init.method === "GET" || !init.method)) {
        return json([mockListing]);
      }
      if (url.includes("/v2/marketplace/consent")) {
        return json(mockConsent);
      }
      if (url.includes("/v2/marketplace/receipts") && (!init || init.method === "GET" || !init.method)) {
        return json([]);
      }
      if (url.includes("/v2/marketplace/receipts") && init?.method === "POST") {
        return json(
          {
            id: "rcpt_999",
            listing_id: "list_bio_1",
            buyer_id: "local-node",
            amount_credits: 25,
            currency: "internal_credits",
            created_at_ms: 3000,
          },
          201,
        );
      }
      return json({}, 404);
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<MarketplaceView />);

    expect(await screen.findByText("Biology Exam Questions")).toBeTruthy();
    expect(screen.getByText("25 CREDITS")).toBeTruthy();

    const buyButton = screen.getByRole("button", { name: /Buy for 25 credits/i });
    fireEvent.click(buyButton);

    expect(await screen.findByText(/Purchase successful/i)).toBeTruthy();
  });

  it("switches to seller console and verifies signing disabled state", async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.includes("/v2/marketplace/listings")) return json([mockListing]);
      if (url.includes("/v2/marketplace/consent")) return json(mockConsent);
      if (url.includes("/v2/marketplace/receipts")) return json([]);
      return json({});
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<MarketplaceView />);

    // Switch to seller console
    const sellerTabBtn = await screen.findByRole("button", { name: /Seller Console/i });
    fireEvent.click(sellerTabBtn);
    expect(await screen.findByText("Publish New Dataset / Package")).toBeTruthy();

    // Verify Publish button and Revoke button are disabled due to signing required
    const publishBtn = screen.getByRole("button", { name: /Publish Listing/i });
    expect(publishBtn).toBeDisabled();
    expect(screen.getByText(/Signing required \(client-side Ed25519 signature\)/i)).toBeTruthy();

    const revokeBtn = screen.getByRole("button", { name: /Revoke/i });
    expect(revokeBtn).toBeDisabled();
  });

  it("submits consent form with expected method, URL and body", async () => {
    let consentPostCall: { url: string; method?: string; body?: string } | null = null;
    const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes("/v2/marketplace/listings")) return json([mockListing]);
      if (url.includes("/v2/marketplace/consent") && init?.method === "POST") {
        consentPostCall = { url, method: init.method, body: init.body as string };
        return json({
          node_id: "node_custom",
          state: "shared_sensitive",
          terms_version: "v2.0",
          consent_version: "v2.0",
          updated_at_ms: 5000,
        });
      }
      if (url.includes("/v2/marketplace/consent")) return json(mockConsent);
      if (url.includes("/v2/marketplace/receipts")) return json([]);
      return json({});
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<MarketplaceView />);

    // Switch to consent screen
    const consentTabBtn = await screen.findByRole("button", { name: /Consent Screen/i });
    fireEvent.click(consentTabBtn);
    expect(await screen.findByText("Data Node Participation Consent")).toBeTruthy();

    // Select Shared Sensitive tier
    const sharedSensBtn = screen.getByRole("button", { name: /Shared Sensitive/i });
    fireEvent.click(sharedSensBtn);

    // Update versions
    const termsInput = screen.getByLabelText("Terms Version");
    fireEvent.change(termsInput, { target: { value: "v2.0" } });

    // Submit consent form
    const applyBtn = screen.getByRole("button", { name: /Apply Node Consent/i });
    fireEvent.click(applyBtn);

    await waitFor(() => {
      expect(consentPostCall).not.toBeNull();
    });

    expect(consentPostCall!.url).toContain("/v2/marketplace/consent");
    expect(consentPostCall!.method).toBe("POST");
    const sentBody = JSON.parse(consentPostCall!.body!);
    expect(sentBody.state).toBe("shared_sensitive");
    expect(sentBody.node_id).toBe("local-node");
    expect(await screen.findByText(/Consent state updated to: SHARED_SENSITIVE/i)).toBeTruthy();
  });

  it("renders read-only status card in panel-ui with active listings and consent", async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.includes("/v2/marketplace/listings")) return json([mockListing]);
      if (url.includes("/v2/marketplace/consent")) return json(mockConsent);
      return json([]);
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<MarketplaceStatusCard />);

    expect(await screen.findByText("Marketplace v2")).toBeTruthy();
    const activeElem = await screen.findByTestId("card-active-listings");
    expect(activeElem.textContent).toBe("1");
    expect(await screen.findByText("shared")).toBeTruthy();
  });

  const mockPack = {
    generated_at: "2026-01-01T00:00:00Z",
    features_total: 10,
    features_draft: 2,
    gaps_zero_symbol_modules: [],
    decisions_count: 5,
    support_open: 1,
    inbox_open: 0,
  };

  it("hides Marketplace tab in MalocaView when v2 routes return 404", async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.includes("/maloca/pack")) return json(mockPack);
      if (url.includes("/v2/marketplace/listings")) return json({ error: "Not Found" }, 404);
      return json([]);
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<MalocaView />);
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /Marketplace/i })).toBeNull();
    });
  });

  it("shows Marketplace tab in MalocaView when v2 routes return 200", async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.includes("/maloca/pack")) return json(mockPack);
      if (url.includes("/v2/marketplace/listings")) return json([mockListing]);
      return json([]);
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<MalocaView />);
    expect(await screen.findByRole("button", { name: /Marketplace/i })).toBeTruthy();
  });
});
