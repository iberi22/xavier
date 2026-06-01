import {
  AddMemoryRequest,
  ClientOptions,
  DeleteResponse,
  RetrieveResponse,
  SearchResponse,
  StatsResponse,
} from './types';

export class XavierClient {
  private baseUrl: string;
  private token: string;

  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl || 'http://localhost:8080').replace(/\/$/, '');
    this.token = options.token || process.env.XAVIER_TOKEN || 'dev-token';
  }

  private getHeaders(): Record<string, string> {
    return {
      'X-Xavier-Token': this.token,
      'Content-Type': 'application/json',
    };
  }

  /**
   * Add a document to memory.
   */
  async add(payload: AddMemoryRequest): Promise<any> {
    const response = await fetch(`${this.baseUrl}/memory/add`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(payload),
    });

    if (!response.ok) {
      throw new Error(`Xavier error: ${response.status} ${response.statusText}`);
    }

    return response.json();
  }

  /**
   * Search memory with semantic + lexical hybrid search.
   */
  async search(query: string, limit = 10, filters?: any): Promise<SearchResponse> {
    const response = await fetch(`${this.baseUrl}/memory/search`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify({ query, limit, filters }),
    });

    if (!response.ok) {
      throw new Error(`Xavier error: ${response.status} ${response.statusText}`);
    }

    return response.json() as Promise<SearchResponse>;
  }

  /**
   * Perform multi-layer memory retrieval.
   */
  async retrieve(query: string, limit = 10, options: any = {}): Promise<RetrieveResponse> {
    const response = await fetch(`${this.baseUrl}/memory/retrieve`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify({ query, limit, ...options }),
    });

    if (!response.ok) {
      throw new Error(`Xavier error: ${response.status} ${response.statusText}`);
    }

    return response.json() as Promise<RetrieveResponse>;
  }

  /**
   * Get memory statistics.
   */
  async stats(): Promise<StatsResponse> {
    const response = await fetch(`${this.baseUrl}/memory/stats`, {
      method: 'GET',
      headers: this.getHeaders(),
    });

    if (!response.ok) {
      throw new Error(`Xavier error: ${response.status} ${response.statusText}`);
    }

    return response.json() as Promise<StatsResponse>;
  }

  /**
   * Delete a memory entry by ID or path.
   */
  async delete(options: { id?: string; path?: string }): Promise<DeleteResponse> {
    const response = await fetch(`${this.baseUrl}/memory/delete`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(options),
    });

    if (!response.ok) {
      throw new Error(`Xavier error: ${response.status} ${response.statusText}`);
    }

    return response.json() as Promise<DeleteResponse>;
  }
}
