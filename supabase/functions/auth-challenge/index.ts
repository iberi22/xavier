import { ed25519 } from "npm:@noble/curves@1.4.0/ed25519";
import * as jose from "npm:jose@5";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
};

/**
 * Converts a hex string to Uint8Array.
 */
function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) {
    throw new Error("Invalid hex string length");
  }
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substring(i, i + 2), 16);
  }
  return bytes;
}

/**
 * Returns tenant mapping registry from environment or fallback default.
 */
function getTenantRegistry(): Record<string, string> {
  const envRegistry = Deno.env.get("TENANT_REGISTRY");
  if (envRegistry) {
    try {
      return JSON.parse(envRegistry);
    } catch (_e) {
      console.warn("Failed to parse TENANT_REGISTRY env var JSON");
    }
  }
  return {};
}

export async function handleAuthChallenge(req: Request): Promise<Response> {
  // Handle CORS preflight request
  if (req.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }

  if (req.method !== "POST") {
    return new Response(JSON.stringify({ error: "Method not allowed" }), {
      status: 405,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }

  try {
    const body = await req.json();
    const { public_key, signature, challenge } = body;

    if (!public_key || !signature || !challenge) {
      return new Response(
        JSON.stringify({ error: "Missing required fields: public_key, signature, challenge" }),
        { status: 400, headers: { ...corsHeaders, "Content-Type": "application/json" } }
      );
    }

    const registry = getTenantRegistry();
    const cleanPubKey = public_key.toLowerCase();
    const tenant_id = registry[cleanPubKey] || registry[public_key];

    if (!tenant_id) {
      return new Response(
        JSON.stringify({ error: "Unregistered public key / tenant not found" }),
        { status: 401, headers: { ...corsHeaders, "Content-Type": "application/json" } }
      );
    }

    let pubKeyBytes: Uint8Array;
    let sigBytes: Uint8Array;
    let msgBytes: Uint8Array;

    try {
      pubKeyBytes = hexToBytes(public_key);
      sigBytes = hexToBytes(signature);
      msgBytes = new TextEncoder().encode(challenge);
    } catch (_e) {
      return new Response(
        JSON.stringify({ error: "Invalid hex format for public_key or signature" }),
        { status: 400, headers: { ...corsHeaders, "Content-Type": "application/json" } }
      );
    }

    const isValid = ed25519.verify(sigBytes, msgBytes, pubKeyBytes);
    if (!isValid) {
      return new Response(
        JSON.stringify({ error: "Invalid signature" }),
        { status: 401, headers: { ...corsHeaders, "Content-Type": "application/json" } }
      );
    }

    const jwtSecretStr = Deno.env.get("JWT_SECRET") || Deno.env.get("SUPABASE_JWT_SECRET") || "default-secret-key-change-in-production-32bytes!";
    const secret = new TextEncoder().encode(jwtSecretStr);

    const now = Math.floor(Date.now() / 1000);
    const expiresAt = now + 24 * 60 * 60; // 24 hours

    const token = await new jose.SignJWT({
      "request.tenant_id": tenant_id,
      sub: public_key,
      role: "authenticated",
    })
      .setProtectedHeader({ alg: "HS256" })
      .setIssuedAt(now)
      .setExpirationTime(expiresAt)
      .sign(secret);

    return new Response(
      JSON.stringify({
        token,
        tenant_id,
        expires_at: expiresAt,
      }),
      { status: 200, headers: { ...corsHeaders, "Content-Type": "application/json" } }
    );
  } catch (err) {
    return new Response(
      JSON.stringify({ error: err instanceof Error ? err.message : "Internal error" }),
      { status: 500, headers: { ...corsHeaders, "Content-Type": "application/json" } }
    );
  }
}

if (import.meta.main) {
  Deno.serve(handleAuthChallenge);
}
