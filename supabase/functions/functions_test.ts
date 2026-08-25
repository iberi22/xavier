import { assertEquals, assertExists } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { ed25519 } from "npm:@noble/curves@1.4.0/ed25519";
import * as jose from "npm:jose@5";
import { handleAuthChallenge } from "./auth-challenge/index.ts";
import { handleAuthVerify } from "./auth-verify/index.ts";

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

Deno.test("auth-challenge: valid signature returns 200 with JWT containing tenant_id claim", async () => {
  const privKey = ed25519.utils.randomPrivateKey();
  const pubKeyBytes = ed25519.getPublicKey(privKey);
  const pubKeyHex = bytesToHex(pubKeyBytes);

  const tenantId = "tenant-test-123";
  Deno.env.set("TENANT_REGISTRY", JSON.stringify({ [pubKeyHex]: tenantId }));

  const challenge = "test-challenge-nonce-123456";
  const msgBytes = new TextEncoder().encode(challenge);
  const sigBytes = ed25519.sign(msgBytes, privKey);
  const sigHex = bytesToHex(sigBytes);

  const req = new Request("http://localhost/functions/v1/auth-challenge", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      public_key: pubKeyHex,
      signature: sigHex,
      challenge,
    }),
  });

  const res = await handleAuthChallenge(req);
  assertEquals(res.status, 200);

  const json = await res.json();
  assertEquals(json.tenant_id, tenantId);
  assertExists(json.token);
  assertExists(json.expires_at);

  // Verify JWT claims
  const secretStr = Deno.env.get("JWT_SECRET") || Deno.env.get("SUPABASE_JWT_SECRET") || "default-secret-key-change-in-production-32bytes!";
  const secret = new TextEncoder().encode(secretStr);
  const { payload } = await jose.jwtVerify(json.token, secret);

  assertEquals(payload["request.tenant_id"], tenantId);
  assertEquals(payload.sub, pubKeyHex);
});

Deno.test("auth-challenge: invalid signature returns 401", async () => {
  const privKey = ed25519.utils.randomPrivateKey();
  const pubKeyBytes = ed25519.getPublicKey(privKey);
  const pubKeyHex = bytesToHex(pubKeyBytes);

  const tenantId = "tenant-test-456";
  Deno.env.set("TENANT_REGISTRY", JSON.stringify({ [pubKeyHex]: tenantId }));

  const challenge = "test-challenge-nonce-123456";
  const wrongPrivKey = ed25519.utils.randomPrivateKey();
  const sigBytes = ed25519.sign(new TextEncoder().encode(challenge), wrongPrivKey);
  const sigHex = bytesToHex(sigBytes);

  const req = new Request("http://localhost/functions/v1/auth-challenge", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      public_key: pubKeyHex,
      signature: sigHex,
      challenge,
    }),
  });

  const res = await handleAuthChallenge(req);
  assertEquals(res.status, 401);

  const json = await res.json();
  assertEquals(json.error, "Invalid signature");
});

Deno.test("auth-challenge: unregistered public key returns 401", async () => {
  const privKey = ed25519.utils.randomPrivateKey();
  const pubKeyHex = bytesToHex(ed25519.getPublicKey(privKey));

  Deno.env.set("TENANT_REGISTRY", JSON.stringify({}));

  const challenge = "test-challenge";
  const sigHex = bytesToHex(ed25519.sign(new TextEncoder().encode(challenge), privKey));

  const req = new Request("http://localhost/functions/v1/auth-challenge", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      public_key: pubKeyHex,
      signature: sigHex,
      challenge,
    }),
  });

  const res = await handleAuthChallenge(req);
  assertEquals(res.status, 401);
});

Deno.test("auth-verify: valid token returns 200 with refreshed token", async () => {
  const secretStr = Deno.env.get("JWT_SECRET") || Deno.env.get("SUPABASE_JWT_SECRET") || "default-secret-key-change-in-production-32bytes!";
  const secret = new TextEncoder().encode(secretStr);

  const tenantId = "tenant-verify-789";
  const token = await new jose.SignJWT({
    "request.tenant_id": tenantId,
    sub: "pubkey-123",
  })
    .setProtectedHeader({ alg: "HS256" })
    .setIssuedAt()
    .setExpirationTime("1h")
    .sign(secret);

  const req = new Request("http://localhost/functions/v1/auth-verify", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
  });

  const res = await handleAuthVerify(req);
  assertEquals(res.status, 200);

  const json = await res.json();
  assertEquals(json.tenant_id, tenantId);
  assertExists(json.token);

  // Verify refreshed token
  const { payload } = await jose.jwtVerify(json.token, secret);
  assertEquals(payload["request.tenant_id"], tenantId);
});

Deno.test("auth-verify: expired token returns 401", async () => {
  const secretStr = Deno.env.get("JWT_SECRET") || Deno.env.get("SUPABASE_JWT_SECRET") || "default-secret-key-change-in-production-32bytes!";
  const secret = new TextEncoder().encode(secretStr);

  // Expired 10 seconds ago
  const now = Math.floor(Date.now() / 1000);
  const token = await new jose.SignJWT({
    "request.tenant_id": "tenant-expired",
    sub: "pubkey-123",
  })
    .setProtectedHeader({ alg: "HS256" })
    .setIssuedAt(now - 3600)
    .setExpirationTime(now - 10)
    .sign(secret);

  const req = new Request("http://localhost/functions/v1/auth-verify", {
    method: "GET",
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  const res = await handleAuthVerify(req);
  assertEquals(res.status, 401);

  const json = await res.json();
  assertEquals(json.error, "Invalid or expired token");
});
