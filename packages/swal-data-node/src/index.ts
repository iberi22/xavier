import {
  importHkdfKey,
  deriveEncryptionKey,
  deriveTenantId,
  encryptData,
  decryptData,
} from "./crypto.js";

export interface SwalDataNodeOptions {
  endpoint: string;
  secret: string | Uint8Array;
  tableName?: string;
  headers?: Record<string, string>;
  fetch?: typeof fetch;
}

export interface EncryptedRecord {
  tenant_id: string;
  kind: string;
  id: string;
  ciphertext: string;
  iv: string;
  created_at?: string;
  updated_at?: string;
  [key: string]: unknown;
}

export class SwalDataNode {
  private endpoint: string;
  private tableName: string;
  private headers: Record<string, string>;
  private customFetch: typeof fetch;
  private secret: string | Uint8Array;
  private encryptionKeyPromise: Promise<CryptoKey> | null = null;
  private tenantIdPromise: Promise<string> | null = null;

  constructor(options: SwalDataNodeOptions) {
    if (!options.endpoint) {
      throw new Error("SwalDataNode requires an endpoint URL.");
    }
    if (!options.secret) {
      throw new Error("SwalDataNode requires a secret (wallet signature or key material).");
    }

    // Normalize endpoint URL (remove trailing slash)
    this.endpoint = options.endpoint.replace(/\/+$/, "");
    this.tableName = options.tableName ?? "app_data_enc";
    this.secret = options.secret;
    this.headers = options.headers ?? {};
    this.customFetch = options.fetch ?? (typeof globalThis.fetch === "function" ? globalThis.fetch.bind(globalThis) : fetch);
  }

  private async getKeys(): Promise<{ encryptionKey: CryptoKey; tenantId: string }> {
    if (!this.encryptionKeyPromise || !this.tenantIdPromise) {
      const hkdfKeyPromise = importHkdfKey(this.secret);
      this.encryptionKeyPromise = hkdfKeyPromise.then((hkdfKey) => deriveEncryptionKey(hkdfKey));
      this.tenantIdPromise = hkdfKeyPromise.then((hkdfKey) => deriveTenantId(hkdfKey));
    }
    const [encryptionKey, tenantId] = await Promise.all([
      this.encryptionKeyPromise,
      this.tenantIdPromise,
    ]);
    return { encryptionKey, tenantId };
  }

  private getTableUrl(): string {
    if (this.endpoint.endsWith(`/${this.tableName}`)) {
      return this.endpoint;
    }
    if (this.endpoint.includes("/rest/v1")) {
      return `${this.endpoint}/${this.tableName}`;
    }
    return `${this.endpoint}/${this.tableName}`;
  }

  public async put<T = unknown>(kind: string, id: string, data: T): Promise<EncryptedRecord> {
    const { encryptionKey, tenantId } = await this.getKeys();
    const { ciphertext, iv } = await encryptData(data, encryptionKey);

    const recordPayload = {
      tenant_id: tenantId,
      kind,
      id,
      ciphertext,
      iv,
      updated_at: new Date().toISOString(),
    };

    const tableUrl = this.getTableUrl();
    const reqHeaders: Record<string, string> = {
      "Content-Type": "application/json",
      Prefer: "resolution=merge-duplicates,return=representation",
      ...this.headers,
    };

    const response = await this.customFetch(tableUrl, {
      method: "POST",
      headers: reqHeaders,
      body: JSON.stringify(recordPayload),
    });

    if (!response.ok) {
      const errText = await response.text().catch(() => "");
      throw new Error(`SwalDataNode put failed (${response.status}): ${errText}`);
    }

    return recordPayload;
  }

  public async get<T = unknown>(kind: string, id: string): Promise<T | null> {
    const { encryptionKey, tenantId } = await this.getKeys();
    const tableUrl = this.getTableUrl();

    const queryParams = new URLSearchParams({
      tenant_id: `eq.${tenantId}`,
      kind: `eq.${kind}`,
      id: `eq.${id}`,
      select: "*",
    });

    const url = `${tableUrl}?${queryParams.toString()}`;
    const reqHeaders: Record<string, string> = {
      Accept: "application/json",
      ...this.headers,
    };

    const response = await this.customFetch(url, {
      method: "GET",
      headers: reqHeaders,
    });

    if (response.status === 404) {
      return null;
    }

    if (!response.ok) {
      const errText = await response.text().catch(() => "");
      throw new Error(`SwalDataNode get failed (${response.status}): ${errText}`);
    }

    const records = (await response.json()) as EncryptedRecord[];
    if (!Array.isArray(records) || records.length === 0) {
      return null;
    }

    const record = records[0];
    return decryptData<T>({ ciphertext: record.ciphertext, iv: record.iv }, encryptionKey);
  }

  public async list<T = unknown>(kind: string): Promise<Array<{ id: string; data: T; created_at?: string; updated_at?: string }>> {
    const { encryptionKey, tenantId } = await this.getKeys();
    const tableUrl = this.getTableUrl();

    const queryParams = new URLSearchParams({
      tenant_id: `eq.${tenantId}`,
      kind: `eq.${kind}`,
      select: "*",
    });

    const url = `${tableUrl}?${queryParams.toString()}`;
    const reqHeaders: Record<string, string> = {
      Accept: "application/json",
      ...this.headers,
    };

    const response = await this.customFetch(url, {
      method: "GET",
      headers: reqHeaders,
    });

    if (!response.ok) {
      const errText = await response.text().catch(() => "");
      throw new Error(`SwalDataNode list failed (${response.status}): ${errText}`);
    }

    const records = (await response.json()) as EncryptedRecord[];
    if (!Array.isArray(records)) {
      return [];
    }

    const decryptedList = await Promise.all(
      records.map(async (record) => {
        const data = await decryptData<T>(
          { ciphertext: record.ciphertext, iv: record.iv },
          encryptionKey
        );
        return {
          id: record.id,
          data,
          created_at: record.created_at,
          updated_at: record.updated_at,
        };
      })
    );

    return decryptedList;
  }

  public async delete(kind: string, id: string): Promise<boolean> {
    const { tenantId } = await this.getKeys();
    const tableUrl = this.getTableUrl();

    const queryParams = new URLSearchParams({
      tenant_id: `eq.${tenantId}`,
      kind: `eq.${kind}`,
      id: `eq.${id}`,
    });

    const url = `${tableUrl}?${queryParams.toString()}`;
    const reqHeaders: Record<string, string> = {
      ...this.headers,
    };

    const response = await this.customFetch(url, {
      method: "DELETE",
      headers: reqHeaders,
    });

    if (!response.ok) {
      const errText = await response.text().catch(() => "");
      throw new Error(`SwalDataNode delete failed (${response.status}): ${errText}`);
    }

    return true;
  }
}

export * from "./crypto.js";
