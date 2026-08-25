import { describe, it, expect, vi } from "vitest";
import { SwalDataNode, EncryptedRecord } from "../src/index.js";

describe("SwalDataNode Client E2E & Encryption Tests", () => {
  const secret = "0xdeadbeef1234567890abcdef1234567890abcdef1234567890abcdef12345678";
  const mockEndpoint = "https://auuhejigwpwdkqsoaoht.supabase.co/rest/v1";

  it("should encrypt plaintext on put, perform get(decrypt) roundtrip, and ensure wire payload contains no plaintext", async () => {
    const store = new Map<string, EncryptedRecord>();
    let interceptedRequestBody = "";

    const mockFetch = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      const urlStr = url.toString();

      if (init?.method === "POST") {
        interceptedRequestBody = init.body as string;
        const payload = JSON.parse(interceptedRequestBody) as EncryptedRecord;
        const key = `${payload.tenant_id}:${payload.kind}:${payload.id}`;
        store.set(key, payload);
        return new Response(JSON.stringify([payload]), { status: 201 });
      }

      if (init?.method === "GET") {
        const urlObj = new URL(urlStr);
        const tenantEq = urlObj.searchParams.get("tenant_id")?.replace("eq.", "");
        const kindEq = urlObj.searchParams.get("kind")?.replace("eq.", "");
        const idEq = urlObj.searchParams.get("id")?.replace("eq.", "");

        const results: EncryptedRecord[] = [];
        for (const record of store.values()) {
          if (
            (!tenantEq || record.tenant_id === tenantEq) &&
            (!kindEq || record.kind === kindEq) &&
            (!idEq || record.id === idEq)
          ) {
            results.push(record);
          }
        }
        return new Response(JSON.stringify(results), { status: 200 });
      }

      if (init?.method === "DELETE") {
        const urlObj = new URL(urlStr);
        const tenantEq = urlObj.searchParams.get("tenant_id")?.replace("eq.", "");
        const kindEq = urlObj.searchParams.get("kind")?.replace("eq.", "");
        const idEq = urlObj.searchParams.get("id")?.replace("eq.", "");

        for (const [key, record] of store.entries()) {
          if (
            (!tenantEq || record.tenant_id === tenantEq) &&
            (!kindEq || record.kind === kindEq) &&
            (!idEq || record.id === idEq)
          ) {
            store.delete(key);
          }
        }
        return new Response(null, { status: 204 });
      }

      return new Response("Not found", { status: 404 });
    });

    const node = new SwalDataNode({
      endpoint: mockEndpoint,
      secret,
      fetch: mockFetch as unknown as typeof fetch,
    });

    const sampleKind = "user_settings";
    const sampleId = "cfg_001";
    const sampleData = {
      theme: "dark",
      apiToken: "super-secret-token-value-999",
      nested: { foo: "bar", secretValue: "TOP_SECRET_PLAINTEXT" },
    };

    // 1. PUT (Encrypt)
    await node.put(sampleKind, sampleId, sampleData);

    // Assert wire request body contains NO raw secret plaintext
    expect(interceptedRequestBody).not.toContain("super-secret-token-value-999");
    expect(interceptedRequestBody).not.toContain("TOP_SECRET_PLAINTEXT");
    expect(interceptedRequestBody).not.toContain(secret);

    // 2. GET (Decrypt) roundtrip verification
    const decrypted = await node.get<typeof sampleData>(sampleKind, sampleId);
    expect(decrypted).toEqual(sampleData);

    // 3. LIST (Decrypt multiple)
    const sampleId2 = "cfg_002";
    const sampleData2 = { theme: "light", nested: { foo: "baz" } };
    await node.put(sampleKind, sampleId2, sampleData2);

    const listResult = await node.list<typeof sampleData>(sampleKind);
    expect(listResult).toHaveLength(2);
    const item1 = listResult.find((i) => i.id === sampleId);
    const item2 = listResult.find((i) => i.id === sampleId2);
    expect(item1?.data).toEqual(sampleData);
    expect(item2?.data).toEqual(sampleData2);

    // 4. DELETE
    const deleteSuccess = await node.delete(sampleKind, sampleId);
    expect(deleteSuccess).toBe(true);

    const getAfterDelete = await node.get(sampleKind, sampleId);
    expect(getAfterDelete).toBeNull();
  });
});
