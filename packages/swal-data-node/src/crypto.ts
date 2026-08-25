const encoder = new TextEncoder();
const decoder = new TextDecoder();

declare const globalThis: {
  Buffer?: {
    from(data: Uint8Array | string, encoding?: string): { toString(enc: string): string };
  };
};

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const len = bytes.byteLength;
  for (let i = 0; i < len; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  if (typeof btoa !== "undefined") {
    return btoa(binary);
  }
  if (globalThis.Buffer) {
    return globalThis.Buffer.from(bytes).toString("base64");
  }
  throw new Error("No base64 encoding implementation available in environment.");
}

export function base64ToBytes(base64: string): Uint8Array {
  if (typeof atob !== "undefined") {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }
  if (globalThis.Buffer) {
    return new Uint8Array(globalThis.Buffer.from(base64, "base64") as unknown as ArrayBufferLike);
  }
  throw new Error("No base64 decoding implementation available in environment.");
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substring(i, i + 2), 16);
  }
  return bytes;
}

export const DEFAULT_SALT = encoder.encode("swal-data-node-salt-v1");
export const DEFAULT_INFO_KEY = encoder.encode("swal-data-node-encryption-key");
export const DEFAULT_INFO_TENANT = encoder.encode("swal-data-node-tenant-id");

export async function importHkdfKey(secret: string | Uint8Array): Promise<CryptoKey> {
  const secretBytes = typeof secret === "string" ? encoder.encode(secret) : secret;
  return crypto.subtle.importKey(
    "raw",
    secretBytes as BufferSource,
    { name: "HKDF" },
    false,
    ["deriveKey", "deriveBits"]
  );
}

export async function deriveEncryptionKey(
  hkdfKey: CryptoKey,
  salt: Uint8Array = DEFAULT_SALT,
  info: Uint8Array = DEFAULT_INFO_KEY
): Promise<CryptoKey> {
  return crypto.subtle.deriveKey(
    {
      name: "HKDF",
      hash: "SHA-256",
      salt: salt as BufferSource,
      info: info as BufferSource,
    },
    hkdfKey,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"]
  );
}

export async function deriveTenantId(
  hkdfKey: CryptoKey,
  salt: Uint8Array = DEFAULT_SALT,
  info: Uint8Array = DEFAULT_INFO_TENANT
): Promise<string> {
  const bits = await crypto.subtle.deriveBits(
    {
      name: "HKDF",
      hash: "SHA-256",
      salt: salt as BufferSource,
      info: info as BufferSource,
    },
    hkdfKey,
    256
  );
  return bytesToHex(new Uint8Array(bits));
}

export async function encryptData(
  data: unknown,
  key: CryptoKey
): Promise<{ ciphertext: string; iv: string }> {
  const jsonString = JSON.stringify(data);
  const plaintext = encoder.encode(jsonString);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ciphertextBuffer = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv: iv as BufferSource },
    key,
    plaintext as BufferSource
  );
  return {
    ciphertext: bytesToBase64(new Uint8Array(ciphertextBuffer)),
    iv: bytesToBase64(iv),
  };
}

export async function decryptData<T = unknown>(
  encrypted: { ciphertext: string; iv: string },
  key: CryptoKey
): Promise<T> {
  const ciphertextBytes = base64ToBytes(encrypted.ciphertext);
  const ivBytes = base64ToBytes(encrypted.iv);
  const decryptedBuffer = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv: ivBytes as BufferSource },
    key,
    ciphertextBytes as BufferSource
  );
  const jsonString = decoder.decode(decryptedBuffer);
  return JSON.parse(jsonString) as T;
}
