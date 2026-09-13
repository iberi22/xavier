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
