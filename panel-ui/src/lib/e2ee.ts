/**
 * @file e2ee.ts
 * @description Zero-Knowledge End-to-End Encryption (E2EE) module for Xavier Legal Enterprise.
 *
 * Implements client-side AES-256-GCM and key derivation using Web Crypto API.
 * Ensures attorney-client privilege and statutory tax secrecy:
 * all document drafts, sensitive tax calculations, and notes are encrypted on client
 * before persisting or synchronizing to the mesh/cloud.
 */

export interface EncryptedPayload {
  v: number;
  alg: "AES-256-GCM";
  iv: string; // Base64
  ciphertext: string; // Base64
  tagLength: number;
  fileId?: string;
}

export class E2EEService {
  private static readonly ALGORITHM = "AES-GCM";
  private static readonly KEY_LENGTH = 256;
  private static readonly IV_LENGTH = 12; // 96-bit recommended for GCM

  /**
   * Generates a random 256-bit symmetric key for a document (DEK).
   */
  static async generateKey(): Promise<CryptoKey> {
    return window.crypto.subtle.generateKey(
      {
        name: this.ALGORITHM,
        length: this.KEY_LENGTH,
      },
      true,
      ["encrypt", "decrypt"]
    );
  }

  /**
   * Derives a Key Encryption Key (KEK) from a firm master passphrase and salt.
   */
  static async deriveKeyFromPassphrase(passphrase: string, salt: Uint8Array): Promise<CryptoKey> {
    const enc = new TextEncoder();
    const keyMaterial = await window.crypto.subtle.importKey(
      "raw",
      enc.encode(passphrase),
      { name: "PBKDF2" },
      false,
      ["deriveKey"]
    );

    return window.crypto.subtle.deriveKey(
      {
        name: "PBKDF2",
        salt: salt as BufferSource,
        iterations: 100000,
        hash: "SHA-256",
      },
      keyMaterial,
      { name: this.ALGORITHM, length: this.KEY_LENGTH },
      false,
      ["encrypt", "decrypt"]
    );
  }

  /**
   * Encrypts plaintext string using AES-256-GCM.
   */
  static async encrypt(plaintext: string, key: CryptoKey, fileId?: string): Promise<EncryptedPayload> {
    const enc = new TextEncoder();
    const iv = window.crypto.getRandomValues(new Uint8Array(this.IV_LENGTH));
    const encodedData = enc.encode(plaintext);

    const ciphertextBuffer = await window.crypto.subtle.encrypt(
      {
        name: this.ALGORITHM,
        iv: iv as BufferSource,
        additionalData: fileId ? enc.encode(fileId) : undefined,
      },
      key,
      encodedData
    );

    return {
      v: 1,
      alg: "AES-256-GCM",
      iv: this.bufferToBase64(iv),
      ciphertext: this.bufferToBase64(new Uint8Array(ciphertextBuffer)),
      tagLength: 128,
      fileId,
    };
  }

  /**
   * Decrypts an EncryptedPayload into plaintext string.
   */
  static async decrypt(payload: EncryptedPayload, key: CryptoKey): Promise<string> {
    const iv = this.base64ToBuffer(payload.iv);
    const ciphertext = this.base64ToBuffer(payload.ciphertext);
    const enc = new TextEncoder();

    const decryptedBuffer = await window.crypto.subtle.decrypt(
      {
        name: this.ALGORITHM,
        iv: iv as BufferSource,
        additionalData: payload.fileId ? enc.encode(payload.fileId) : undefined,
      },
      key,
      ciphertext as BufferSource
    );

    const dec = new TextDecoder();
    return dec.decode(decryptedBuffer);
  }

  private static bufferToBase64(buffer: Uint8Array): string {
    let binary = "";
    for (let i = 0; i < buffer.byteLength; i++) {
      binary += String.fromCharCode(buffer[i]);
    }
    return window.btoa(binary);
  }

  private static base64ToBuffer(base64: string): Uint8Array {
    const binary = window.atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }
}
