import { expect, test } from "@playwright/test";

/**
 * End-to-End Test Suite for WebAuthn PRF Hardware Bridge & Node Identity Fallback.
 * Validates hardware-backed PRF extension capabilities detection, CDP virtual authenticators,
 * deterministic seed generation, cancellation rejection handling, and Ed25519 software fallback.
 */

test.describe("WebAuthn PRF Hardware Bridge E2E Suite", () => {
  test.beforeEach(async ({ page }) => {
    // Intercept standard routes so page loads smoothly without network errors
    await page.route("**/health*", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "ok", system: { cpu_usage: 5, ram_usage_percent: 15 } }),
      });
    });

    await page.route("**/v1/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ pagination: { total: 0 }, data: [] }),
      });
    });

    await page.route("**/panel/api/**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    // Set local storage onboarding flags
    await page.addInitScript(() => {
      window.localStorage.setItem("xavier_onboarding_completed", "true");
      window.localStorage.setItem("xavier_token", "mock-e2e-auth-token");
    });

    await page.goto("/");
  });

  test("isWebAuthnPrfSupported evaluates to true when CDP virtual authenticator with PRF extension is enabled", async ({
    page,
  }) => {
    // Attach CDP session and enable virtual WebAuthn CTAP2 authenticator with PRF support
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("WebAuthn.enable");
    await cdp.send("WebAuthn.addVirtualAuthenticator", {
      options: {
        protocol: "ctap2",
        transport: "internal",
        hasPrf: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    });

    // Evaluate client capability detection in browser runtime
    const isSupported = await page.evaluate(async () => {
      const { isWebAuthnPrfSupported } = await import("/src/auth/webauthnPrf.ts");
      return await isWebAuthnPrfSupported();
    });

    expect(isSupported).toBe(true);
  });

  test("deriveHardwareSeedWithPrf returns deterministic 32-byte seed and credentialId with active virtual authenticator", async ({
    page,
  }) => {
    // Configure CDP virtual authenticator with CTAP2 PRF support
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("WebAuthn.enable");
    await cdp.send("WebAuthn.addVirtualAuthenticator", {
      options: {
        protocol: "ctap2",
        transport: "internal",
        hasPrf: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    });

    // Derive hardware seed via WebAuthn PRF extension payload
    const firstResult = await page.evaluate(async () => {
      const { deriveHardwareSeedWithPrf, DEFAULT_XAVIER_PRF_SALT } = await import(
        "/src/auth/webauthnPrf.ts"
      );
      const res = await deriveHardwareSeedWithPrf(undefined, DEFAULT_XAVIER_PRF_SALT);
      if (!res) return null;
      return {
        seedLength: res.seed.length,
        seedHex: Array.from(res.seed)
          .map((b) => b.toString(16).padStart(2, "0"))
          .join(""),
        credentialId: res.credentialId,
      };
    });

    expect(firstResult).not.toBeNull();
    expect(firstResult?.seedLength).toBe(32);
    expect(firstResult?.credentialId).toBeTruthy();

    // Re-derive with the same salt to verify deterministic output key matching
    const secondResult = await page.evaluate(async () => {
      const { deriveHardwareSeedWithPrf, DEFAULT_XAVIER_PRF_SALT } = await import(
        "/src/auth/webauthnPrf.ts"
      );
      const res = await deriveHardwareSeedWithPrf(undefined, DEFAULT_XAVIER_PRF_SALT);
      if (!res) return null;
      return {
        seedLength: res.seed.length,
        seedHex: Array.from(res.seed)
          .map((b) => b.toString(16).padStart(2, "0"))
          .join(""),
        credentialId: res.credentialId,
      };
    });

    expect(secondResult).not.toBeNull();
    expect(secondResult?.seedLength).toBe(32);
    expect(secondResult?.seedHex).toEqual(firstResult?.seedHex);
  });

  test("deriveHardwareSeedWithPrf handles rejection or removed authenticator gracefully returning null", async ({
    page,
  }) => {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("WebAuthn.enable");

    // Add authenticator then immediately remove it to simulate hardware error / user cancellation
    const authRes = await cdp.send("WebAuthn.addVirtualAuthenticator", {
      options: {
        protocol: "ctap2",
        transport: "internal",
        hasPrf: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    });

    await cdp.send("WebAuthn.removeVirtualAuthenticator", {
      authenticatorId: authRes.authenticatorId,
    });

    // Execute deriveHardwareSeedWithPrf and confirm it returns null without unhandled errors
    const result = await page.evaluate(async () => {
      const { deriveHardwareSeedWithPrf } = await import("/src/auth/webauthnPrf.ts");
      try {
        return await deriveHardwareSeedWithPrf();
      } catch (err) {
        return "THREW_EXCEPTION";
      }
    });

    expect(result).toBeNull();
  });

  test("getOrCreateNodeIdentity provides software Ed25519 keypair fallback when hardware PRF fails or is bypassed", async ({
    page,
  }) => {
    // Clear any existing stored identity
    await page.evaluate(() => {
      window.localStorage.removeItem("swal_node_keypair_v1");
    });

    // Invoke getOrCreateNodeIdentity which uses software WebCrypto Ed25519 key generation fallback
    const identityResult = await page.evaluate(async () => {
      const { getOrCreateNodeIdentity } = await import("/src/auth/nodeIdentity.ts");
      const { identity, privateKey, publicKey } = await getOrCreateNodeIdentity();
      return {
        nodeId: identity.nodeId,
        publicKeyHex: identity.publicKeyHex,
        hasPrivKey: Boolean(privateKey),
        hasPubKey: Boolean(publicKey),
        keyAlgorithm: privateKey?.algorithm?.name,
      };
    });

    expect(identityResult.nodeId).toMatch(/^node_[a-f0-9]{16}$/);
    expect(identityResult.publicKeyHex).toHaveLength(64);
    expect(identityResult.hasPrivKey).toBe(true);
    expect(identityResult.hasPubKey).toBe(true);
    expect(identityResult.keyAlgorithm).toBe("Ed25519");
  });
});
