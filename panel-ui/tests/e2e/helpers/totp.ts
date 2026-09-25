import { createHmac } from "node:crypto";

const BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

function base32Decode(input: string): Buffer {
  const clean = input.toUpperCase().replace(/[^A-Z2-7]/g, "");
  let bits = "";
  for (const char of clean) {
    const idx = BASE32_ALPHABET.indexOf(char);
    if (idx === -1) continue;
    bits += idx.toString(2).padStart(5, "0");
  }
  const bytes: number[] = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) {
    bytes.push(Number.parseInt(bits.slice(i, i + 8), 2));
  }
  return Buffer.from(bytes);
}

/**
 * RFC 6238 TOTP — SHA1, 6 digits, 30s step. Mirrors the backend's `build_totp`
 * (apps/xavier/src/auth2/mod.rs): Algorithm::SHA1, digits=6, skew=1, step=30.
 * Implemented with Node's built-in `node:crypto` instead of pulling in a new
 * dependency (otplib/etc.) just for one test file.
 */
export function generateTotp(
  base32Secret: string,
  timeMs: number = Date.now(),
  step = 30,
  digits = 6,
): string {
  const key = base32Decode(base32Secret);
  const counter = Math.floor(timeMs / 1000 / step);
  const counterBuf = Buffer.alloc(8);
  counterBuf.writeBigUInt64BE(BigInt(counter));
  const hmac = createHmac("sha1", key).update(counterBuf).digest();
  const offset = hmac[hmac.length - 1] & 0x0f;
  const binCode =
    ((hmac[offset] & 0x7f) << 24) |
    ((hmac[offset + 1] & 0xff) << 16) |
    ((hmac[offset + 2] & 0xff) << 8) |
    (hmac[offset + 3] & 0xff);
  return (binCode % 10 ** digits).toString().padStart(digits, "0");
}
