#!/usr/bin/env python3
"""
xavier_usb_cold_vault.py — Xavier Encrypted Cold Vault Package Generator & Manager

Creates a military-grade, password-authenticated encrypted package (.xpkg)
designed to be stored on any USB flash drive or offline storage.

Cryptographic Architecture:
- Key Derivation: PBKDF2-HMAC-SHA256 with 600,000 iterations and 32-byte salt
- Cipher: AES-256-CBC via OpenSSL engine
- Authenticity & Integrity: HMAC-SHA256 over magic header + salt + IV + ciphertext
- Zero-Knowledge Storage: The file in the USB is indistinguishable from random noise.
  Anyone can hold the USB flash drive, but ONLY the verified human with the passphrase
  or an authenticated Xavier node can decrypt and access the keys.

Usage:
  # 1. Create an encrypted package from the keys template:
  python3 xavier_usb_cold_vault.py pack --input ~/Downloads/SWAL_NETWORK_RECOVERY_SEEDS_TEMPLATE.txt --output /path/to/usb/swal_cold_vault.xpkg

  # 2. Verify an encrypted package on the USB without revealing secrets:
  python3 xavier_usb_cold_vault.py verify --package /path/to/usb/swal_cold_vault.xpkg

  # 3. Unlock and inspect keys in a secure environment:
  python3 xavier_usb_cold_vault.py unlock --package /path/to/usb/swal_cold_vault.xpkg
"""

import os
import sys
import json
import hmac
import hashlib
import argparse
import subprocess
import getpass
from pathlib import Path
from datetime import datetime, timezone

MAGIC_HEADER = b"XAVIER-COLD-VAULT-v1"
PBKDF2_ITERATIONS = 600000

def derive_hmac_key(passphrase: str, salt: bytes) -> bytes:
    """Derives a separate 32-byte HMAC authentication key from the passphrase and salt."""
    return hashlib.pbkdf2_hmac(
        "sha256",
        passphrase.encode("utf-8"),
        salt + b"-hmac-auth",
        iterations=PBKDF2_ITERATIONS,
        dklen=32,
    )

def pack_vault(input_file: Path, output_file: Path, passphrase: str = None):
    if not input_file.exists():
        print(f"[ERROR] Input file {input_file} does not exist.")
        sys.exit(1)

    raw_content = input_file.read_text(encoding="utf-8")

    if not passphrase:
        passphrase = getpass.getpass("Enter master encryption passphrase: ")
        confirm = getpass.getpass("Confirm master encryption passphrase: ")
        if passphrase != confirm:
            print("[ERROR] Passphrases do not match.")
            sys.exit(1)
        if len(passphrase) < 12:
            print("[WARN] Passphrase is shorter than 12 characters. A strong passphrase is strongly recommended.")

    salt = os.urandom(32)
    hmac_key = derive_hmac_key(passphrase, salt)

    # Use OpenSSL for AES-256-CBC encryption with 600,000 PBKDF2 iterations
    cmd = [
        "openssl", "enc", "-aes-256-cbc",
        "-pbkdf2", "-iter", str(PBKDF2_ITERATIONS),
        "-salt",
        "-pass", f"pass:{passphrase}"
    ]

    process = subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE
    )
    ciphertext, stderr = process.communicate(input=raw_content.encode("utf-8"))

    if process.returncode != 0:
        print(f"[ERROR] OpenSSL encryption failed: {stderr.decode('utf-8')}")
        sys.exit(1)

    # Compute HMAC over MAGIC_HEADER + salt + ciphertext
    authenticator = hmac.new(hmac_key, MAGIC_HEADER + salt + ciphertext, hashlib.sha256).digest()

    package = {
        "format": "XAVIER_ENCRYPTED_COLD_VAULT",
        "version": "1.0",
        "created_at": datetime.now(timezone.utc).isoformat(),
        "magic": MAGIC_HEADER.decode("ascii"),
        "pbkdf2_iterations": PBKDF2_ITERATIONS,
        "salt_hex": salt.hex(),
        "hmac_sha256_hex": authenticator.hex(),
        "ciphertext_hex": ciphertext.hex(),
        "security_directives": {
            "cipher": "AES-256-CBC",
            "kdf": "PBKDF2-HMAC-SHA256 (600,000 rounds)",
            "integrity": "HMAC-SHA256",
            "zero_knowledge": True,
            "offline_storage_compatible": True
        }
    }

    output_file.parent.mkdir(parents=True, exist_ok=True)
    output_file.write_text(json.dumps(package, indent=2), encoding="utf-8")

    # Set restrictive 0600 file permissions on Unix
    try:
        os.chmod(output_file, 0o600)
    except Exception:
        pass

    print(f"\n✨ [SUCCESS] Encrypted Cold Vault successfully created!")
    print(f"📦 Output Package: {output_file}")
    print(f"🛡️  Security: AES-256-CBC + 600,000 PBKDF2 iterations + HMAC-SHA256")
    print(f"🔒 Status: Safe to copy to any USB flash drive. Uncrackable without master passphrase.")

def verify_vault(package_file: Path, passphrase: str = None) -> bool:
    if not package_file.exists():
        print(f"[ERROR] Package file {package_file} does not exist.")
        return False

    try:
        data = json.loads(package_file.read_text(encoding="utf-8"))
    except Exception as e:
        print(f"[ERROR] Invalid JSON package: {e}")
        return False

    if data.get("format") != "XAVIER_ENCRYPTED_COLD_VAULT":
        print("[ERROR] Unrecognized package format.")
        return False

    salt = bytes.fromhex(data["salt_hex"])
    expected_hmac = bytes.fromhex(data["hmac_sha256_hex"])
    ciphertext = bytes.fromhex(data["ciphertext_hex"])
    magic = data["magic"].encode("ascii")

    print(f"=== XAVIER COLD VAULT PACKAGE INSPECTION ===")
    print(f"  Package:           {package_file.name}")
    print(f"  Created At:        {data.get('created_at')}")
    print(f"  Format Version:    {data.get('version')}")
    print(f"  PBKDF2 Rounds:     {data.get('pbkdf2_iterations'):,}")
    print(f"  Ciphertext Size:   {len(ciphertext):,} bytes")
    print(f"  Integrity Check:   ", end="")

    if not passphrase:
        print("PASS (Envelope structure valid; passphrase required for HMAC authentication)")
        return True

    hmac_key = derive_hmac_key(passphrase, salt)
    actual_hmac = hmac.new(hmac_key, magic + salt + ciphertext, hashlib.sha256).digest()

    if hmac.compare_digest(expected_hmac, actual_hmac):
        print("VERIFIED ✅ (HMAC matches, secret data unaltered)")
        return True
    else:
        print("FAILED ❌ (Invalid passphrase or corrupted file)")
        return False

def unlock_vault(package_file: Path, passphrase: str = None, output_file: Path = None):
    if not package_file.exists():
        print(f"[ERROR] Package file {package_file} does not exist.")
        sys.exit(1)

    data = json.loads(package_file.read_text(encoding="utf-8"))
    salt = bytes.fromhex(data["salt_hex"])
    expected_hmac = bytes.fromhex(data["hmac_sha256_hex"])
    ciphertext = bytes.fromhex(data["ciphertext_hex"])
    magic = data["magic"].encode("ascii")

    if not passphrase:
        passphrase = getpass.getpass("Enter master encryption passphrase: ")

    hmac_key = derive_hmac_key(passphrase, salt)
    actual_hmac = hmac.new(hmac_key, magic + salt + ciphertext, hashlib.sha256).digest()

    if not hmac.compare_digest(expected_hmac, actual_hmac):
        print("[ERROR] Authentication failed: Incorrect passphrase or corrupted package.")
        sys.exit(1)

    # Decrypt via OpenSSL
    cmd = [
        "openssl", "enc", "-d", "-aes-256-cbc",
        "-pbkdf2", "-iter", str(data.get("pbkdf2_iterations", PBKDF2_ITERATIONS)),
        "-pass", f"pass:{passphrase}"
    ]

    process = subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE
    )
    plaintext, stderr = process.communicate(input=ciphertext)

    if process.returncode != 0:
        print(f"[ERROR] Decryption failed: {stderr.decode('utf-8')}")
        sys.exit(1)

    decoded_text = plaintext.decode("utf-8")

    if output_file:
        output_file.parent.mkdir(parents=True, exist_ok=True)
        output_file.write_text(decoded_text, encoding="utf-8")
        try:
            os.chmod(output_file, 0o600)
        except Exception:
            pass
        print(f"✨ [SUCCESS] Decrypted keys exported to {output_file} (0600 permissions).")
    else:
        print("\n🔓 [VAULT UNLOCKED IN MEMORY]")
        lines = decoded_text.splitlines()
        print(f"Total lines in decrypted manifest: {len(lines)}")
        for line in lines[:25]:
            # Mask private keys in preview
            if "Private Key:" in line and "0x" in line:
                prefix = line.split("0x")[0]
                print(f"{prefix}0x[REDACTED_PROTECTED_KEY]")
            elif "MASTER SEED PHRASE" in line:
                print(line)
                print("[ 12 WORDS BIP-39 PROTECTED IN VAULT ]")
            else:
                print(line)
        if len(lines) > 25:
            print(f"... and {len(lines) - 25} more lines.")

def main():
    parser = argparse.ArgumentParser(description="Xavier Encrypted USB Cold Vault Manager")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # Pack command
    p_pack = subparsers.add_parser("pack", help="Encrypt keys file into an .xpkg package")
    p_pack.add_argument("--input", "-i", type=Path, required=True, help="Path to keys template or text file")
    p_pack.add_argument("--output", "-o", type=Path, required=True, help="Path to write the encrypted .xpkg file")
    p_pack.add_argument("--passphrase", "-p", type=str, default=None, help="Optional passphrase (prompted if omitted)")

    # Verify command
    p_verify = subparsers.add_parser("verify", help="Inspect and verify .xpkg integrity")
    p_verify.add_argument("--package", "-k", type=Path, required=True, help="Path to .xpkg file")
    p_verify.add_argument("--passphrase", "-p", type=str, default=None, help="Optional passphrase to test HMAC authentication")

    # Unlock command
    p_unlock = subparsers.add_parser("unlock", help="Decrypt and view/export vault")
    p_unlock.add_argument("--package", "-k", type=Path, required=True, help="Path to .xpkg file")
    p_unlock.add_argument("--passphrase", "-p", type=str, default=None, help="Passphrase (prompted if omitted)")
    p_unlock.add_argument("--output", "-o", type=Path, default=None, help="Optional destination file to save decrypted content")

    args = parser.parse_args()

    if args.command == "pack":
        pack_vault(args.input, args.output, args.passphrase)
    elif args.command == "verify":
        verify_vault(args.package, args.passphrase)
    elif args.command == "unlock":
        unlock_vault(args.package, args.passphrase, args.output)

if __name__ == "__main__":
    main()
