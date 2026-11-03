/**
 * Cloudflare Pages Function: Same-Origin API Proxy
 *
 * Proxies API requests (/panel/api, /health, /v1, /auth, /api, /notifications, /maloca)
 * to the backend Xavier Cloud Worker (https://xaviercloud.swal.network or fallback workers.dev).
 * Injects X-Xavier-Token server-side from environment variable XAVIER_API_TOKEN,
 * preventing any client-side bundle token leaks and eliminating CORS issues.
 */

interface Env {
  XAVIER_API_ORIGIN?: string;
  XAVIER_API_TOKEN?: string;
  TURNSTILE_SECRET_KEY?: string;
}

const API_PREFIXES = [
  "/health",
  "/panel/api",
  "/v1",
  "/auth",
  "/api",
  "/notifications",
  "/maloca",
];

const DEFAULT_ORIGIN = "https://xaviercloud.swal.network";
const FALLBACK_ORIGIN = "https://xaviercloud.iberi22.workers.dev";

export const onRequest = async (context: {
  request: Request;
  env: Env;
  next: () => Promise<Response>;
}): Promise<Response> => {
  const url = new URL(context.request.url);
  const isApi = API_PREFIXES.some(
    (p) => url.pathname === p || url.pathname.startsWith(`${p}/`),
  );

  if (!isApi) {
    return context.next();
  }

  // Security Gate: Cloudflare Turnstile CAPTCHA verification on authentication routes
  if (url.pathname === "/auth/magic-link" && context.request.method === "POST") {
    try {
      const cloned = context.request.clone();
      const body = (await cloned.json()) as { turnstile_token?: string };
      if (body.turnstile_token) {
        const secret =
          context.env.TURNSTILE_SECRET_KEY || "1x0000000000000000000000000000000AA";
        const verifyRes = await fetch(
          "https://challenges.cloudflare.com/turnstile/v0/siteverify",
          {
            method: "POST",
            headers: { "Content-Type": "application/x-www-form-urlencoded" },
            body: new URLSearchParams({
              secret,
              response: body.turnstile_token,
              remoteip: context.request.headers.get("CF-Connecting-IP") || "",
            }),
          },
        );
        const verifyData = (await verifyRes.json()) as { success: boolean };
        if (!verifyData.success) {
          return new Response(
            JSON.stringify({
              ok: false,
              error: "CAPTCHA verification failed. Security challenge invalid.",
            }),
            {
              status: 403,
              headers: { "Content-Type": "application/json" },
            },
          );
        }
      }
    } catch {}
  }

  const primaryOrigin = (context.env.XAVIER_API_ORIGIN || DEFAULT_ORIGIN).replace(/\/+$/, "");
  const fallbackOrigin = FALLBACK_ORIGIN.replace(/\/+$/, "");

  // Prepare upstream request headers
  const reqHeaders = new Headers(context.request.headers);
  reqHeaders.delete("host");

  // Server-side inject master token if configured and not present
  if (context.env.XAVIER_API_TOKEN && !reqHeaders.has("X-Xavier-Token")) {
    reqHeaders.set("X-Xavier-Token", context.env.XAVIER_API_TOKEN);
  }

  // Helper to proxy to a specific origin
  const proxyTo = async (origin: string): Promise<Response> => {
    const targetUrl = new URL(`${url.pathname}${url.search}`, origin);
    const init: RequestInit = {
      method: context.request.method,
      headers: reqHeaders,
      redirect: "manual",
    };

    if (context.request.method !== "GET" && context.request.method !== "HEAD") {
      init.body = context.request.body;
      // @ts-ignore
      init.duplex = "half";
    }

    return fetch(targetUrl.toString(), init);
  };

  try {
    const response = await proxyTo(primaryOrigin);
    if (response.status >= 520 && primaryOrigin !== fallbackOrigin) {
      return await proxyTo(fallbackOrigin);
    }
    return response;
  } catch (_err) {
    if (primaryOrigin !== fallbackOrigin) {
      try {
        return await proxyTo(fallbackOrigin);
      } catch (fallbackErr) {
        return new Response(
          JSON.stringify({
            error: "upstream_unavailable",
            message: "Unable to reach Xavier Cloud upstream",
            detail: String(fallbackErr),
          }),
          {
            status: 502,
            headers: { "Content-Type": "application/json" },
          },
        );
      }
    }

    return new Response(
      JSON.stringify({
        error: "upstream_unavailable",
        message: "Unable to reach Xavier Cloud upstream",
      }),
      {
        status: 502,
        headers: { "Content-Type": "application/json" },
      },
    );
  }
};
