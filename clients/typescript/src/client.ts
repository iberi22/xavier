import type {
  AddMemoryRequest,
  AddMemoryResponse,
  ClientOptions,
  DeleteResponse,
  RetrieveResponse,
  SearchResponse,
  StatsResponse,
} from "./types";

export class XavierClient {
  private baseUrl: string;
  private token: string;
  private timeoutMs: number;

  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl || "http://localhost:8006").replace(
      /\/$/,
      "",
    );
    this.token = options.token || process.env.XAVIER_TOKEN || "";
    this.timeoutMs = options.timeoutMs ?? 30000;
    if (!this.token) {
      console.warn(
        "[xavier] No XAVIER_TOKEN set. Set the XAVIER_TOKEN environment variable or pass token in options.",
      );
    }
  }

  private getHeaders(): Record<string, string> {
    return {
      "X-Xavier-Token": this.token,
      "Content-Type": "application/json",
    };
  }

  /**
   * Fetch wrapper with AbortController timeout.
   */
  private async fetchWithTimeout(
    url: string,
    init: RequestInit,
  ): Promise<Response> {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      return await fetch(url, { ...init, signal: controller.signal });
    } finally {
      clearTimeout(timeout);
    }
  }

  /**
   * Universal error handler: throws on non-OK except 404 (handled by caller).
   */
  private async handleResponse(response: Response): Promise<any> {
    if (response.status === 404) {
      return response.json();
    }
    if (!response.ok) {
      throw new Error(
        `Xavier error: ${response.status} ${response.statusText}`,
      );
    }
    return response.json();
  }

  /**
   * Add a document to memory.
   */
  async add(payload: AddMemoryRequest): Promise<AddMemoryResponse> {
    const response = await this.fetchWithTimeout(`${this.baseUrl}/memory/add`, {
      method: "POST",
      headers: this.getHeaders(),
      body: JSON.stringify(payload),
    });
    return this.handleResponse(response) as Promise<AddMemoryResponse>;
  }

  /**
   * Search memory with semantic + lexical hybrid search.
   */
  async search(
    query: string,
    limit = 10,
    filters?: Record<string, any>,
  ): Promise<SearchResponse> {
    const response = await this.fetchWithTimeout(
      `${this.baseUrl}/memory/search`,
      {
        method: "POST",
        headers: this.getHeaders(),
        body: JSON.stringify({ query, limit, filters }),
      },
    );
    return this.handleResponse(response) as Promise<SearchResponse>;
  }

  /**
   * Perform multi-layer memory retrieval.
   */
  async retrieve(
    query: string,
    limit = 10,
    options: Record<string, any> = {},
  ): Promise<RetrieveResponse> {
    const response = await this.fetchWithTimeout(
      `${this.baseUrl}/memory/retrieve`,
      {
        method: "POST",
        headers: this.getHeaders(),
        body: JSON.stringify({ query, limit, ...options }),
      },
    );
    return this.handleResponse(response) as Promise<RetrieveResponse>;
  }

  /**
   * Get memory statistics.
   */
  async stats(): Promise<StatsResponse> {
    const response = await this.fetchWithTimeout(
      `${this.baseUrl}/memory/stats`,
      {
        method: "GET",
        headers: this.getHeaders(),
      },
    );
    return this.handleResponse(response) as Promise<StatsResponse>;
  }

  /**
   * Delete a memory entry by ID or path.
   */
  async delete(options: {
    id?: string;
    path?: string;
  }): Promise<DeleteResponse> {
    if (!options.id && !options.path) {
      throw new Error(
        "Xavier error: Either id or path must be provided for delete.",
      );
    }
    const response = await this.fetchWithTimeout(
      `${this.baseUrl}/memory/delete`,
      {
        method: "POST",
        headers: this.getHeaders(),
        body: JSON.stringify(options),
      },
    );
    return this.handleResponse(response) as Promise<DeleteResponse>;
  }
}
