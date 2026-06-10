import { XavierClient } from "../src/client";

describe("XavierClient", () => {
  let client: XavierClient;

  beforeEach(() => {
    client = new XavierClient({
      baseUrl: "http://localhost:8080",
      token: "test-token",
    });
    // Mock global fetch
    (global as any).fetch = jest.fn();
  });

  afterEach(() => {
    jest.resetAllMocks();
  });

  it("should use token from environment if not provided", () => {
    process.env.XAVIER_TOKEN = "env-token";
    const c = new XavierClient();
    expect((c as any).token).toBe("env-token");
    delete process.env.XAVIER_TOKEN;
  });

  it("should add memory correctly", async () => {
    (fetch as jest.Mock).mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({ status: "ok", id: "123" }),
    });

    const result = await client.add({ content: "test" });
    expect(result.id).toBe("123");
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining("/memory/add"),
      expect.objectContaining({
        method: "POST",
        headers: expect.objectContaining({
          "X-Xavier-Token": "test-token",
        }),
      }),
    );
  });

  it("should search memory correctly", async () => {
    (fetch as jest.Mock).mockResolvedValue({
      ok: true,
      json: () =>
        Promise.resolve({
          status: "ok",
          query: "test query",
          results: [{ id: "1", content: "c1", path: "p1", metadata: {} }],
          count: 1,
        }),
    });

    const result = await client.search("test query");
    expect(result.results.length).toBe(1);
    expect(result.results[0].content).toBe("c1");
  });

  it("should handle errors", async () => {
    (fetch as jest.Mock).mockResolvedValue({
      ok: false,
      status: 401,
      statusText: "Unauthorized",
    });

    await expect(client.stats()).rejects.toThrow(
      "Xavier error: 401 Unauthorized",
    );
  });
});
