import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { MarketplaceView } from "./MarketplaceView";
import { MarketplaceStatusCard } from "../../components/MarketplaceStatusCard";

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

  it("switches to seller console and consent screen", async () => {
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

    // Switch to consent screen
    const consentTabBtn = screen.getByRole("button", { name: /Consent Screen/i });
    fireEvent.click(consentTabBtn);
    expect(await screen.findByText("Data Node Participation Consent")).toBeTruthy();
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
    expect(await screen.findByText("1")).toBeTruthy();
    expect(await screen.findByText("shared")).toBeTruthy();
  });
});
