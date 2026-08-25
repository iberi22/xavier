import * as jose from "npm:jose@5";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
};

export async function handleAuthVerify(req: Request): Promise<Response> {
  // Handle CORS preflight request
  if (req.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }

  try {
    let token: string | null = null;

    // 1. Try to extract Bearer token from Authorization header
    const authHeader = req.headers.get("Authorization") || req.headers.get("authorization");
    if (authHeader && authHeader.toLowerCase().startsWith("bearer ")) {
      token = authHeader.substring(7).trim();
    }

    // 2. If not in header and method is POST, check request body JSON
    if (!token && req.method === "POST") {
      try {
        const body = await req.json();
        if (body && typeof body.token === "string") {
          token = body.token;
        }
      } catch (_e) {
        // Ignored if JSON parsing fails
      }
    }

    if (!token) {
      return new Response(
        JSON.stringify({ error: "Missing authentication token in Authorization header or body" }),
        { status: 401, headers: { ...corsHeaders, "Content-Type": "application/json" } }
      );
    }

    const jwtSecretStr = Deno.env.get("JWT_SECRET") || Deno.env.get("SUPABASE_JWT_SECRET") || "default-secret-key-change-in-production-32bytes!";
    const secret = new TextEncoder().encode(jwtSecretStr);

    let payload: jose.JWTPayload;
    try {
      const verified = await jose.jwtVerify(token, secret);
      payload = verified.payload;
    } catch (_err) {
      return new Response(
        JSON.stringify({ error: "Invalid or expired token" }),
        { status: 401, headers: { ...corsHeaders, "Content-Type": "application/json" } }
      );
    }

    const tenantId = payload["request.tenant_id"] as string | undefined;
    if (!tenantId) {
      return new Response(
        JSON.stringify({ error: "Token missing request.tenant_id claim" }),
        { status: 401, headers: { ...corsHeaders, "Content-Type": "application/json" } }
      );
    }

    // Refresh token with new 24h expiration
    const now = Math.floor(Date.now() / 1000);
    const expiresAt = now + 24 * 60 * 60; // 24 hours

    const newToken = await new jose.SignJWT({
      "request.tenant_id": tenantId,
      sub: payload.sub,
      role: payload.role || "authenticated",
    })
      .setProtectedHeader({ alg: "HS256" })
      .setIssuedAt(now)
      .setExpirationTime(expiresAt)
      .sign(secret);

    return new Response(
      JSON.stringify({
        token: newToken,
        tenant_id: tenantId,
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
  Deno.serve(handleAuthVerify);
}
