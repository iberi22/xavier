# Xavier Air-Gap Capsule Protocol Specification (`.swal_capsule`)

## 1. Overview & Threat Model

The **Xavier Air-Gap Capsule Protocol** provides a sovereign, air-gapped mechanism for archiving, packaging, and physically transferring confidential payloads across offline environments using removable USB storage or cold-storage media.

### Threat Model
- **Physical Loss or Theft:** An attacker intercepts the physical USB flash drive.
- **Untrusted Host Systems:** The capsule is transferred through an intermediary computer that may run surveillance or unauthorized indexing software.
- **Offline Brute-Force Attacks:** An adversary attempts high-throughput GPU/ASIC dictionary attacks against the encrypted container.
- **Tampering & Bit-Flipping:** An attacker modifies ciphertext bytes in an attempt to manipulate decrypted memory or trigger deserialization vulnerabilities.

### Security Guarantees
1. **Authenticated Encryption (AEAD):** AES-256-GCM guarantees both confidentiality and ciphertext integrity. Any altered byte fails decryption immediately.
2. **Memory-Hard Key Derivation:** Argon2id with 64 MB of RAM and 3 iterations protects user passphrases against parallelized GPU cracking.
3. **Targeted Node Locking (Zero-Knowledge Transport):** Optionally encrypts the payload using ephemeral X25519 ECDH key agreement with the destination node's public identity. Only the designated Xavier node can unlock the capsule, even if the physical USB is compromised.
4. **Header Transparency & Safe Inspection:** Capsule metadata (ID, version, creation timestamp, payload kind, recipient node ID) can be inspected without exposing the payload or requiring the passphrase.

---

## 2. Binary Capsule Structure

Each `.swal_capsule` file adheres to the following byte layout:

```
+------------------+-------------------+--------------------+-----------------------+
| Magic Bytes (8B) | Version (2B)      | Header Length (4B) | Encrypted Header JSON |
| "SWALCAPS"       | 0x0001            | Big-Endian u32     | (Plaintext Metadata)  |
+------------------+-------------------+--------------------+-----------------------+
| Salt (16B)       | Nonce / IV (12B)  | Ciphertext (N B)   | Poly1305 / GCM Tag    |
| Argon2id Salt    | AES-GCM Nonce     | Compressed Payload | 16-byte Auth Tag      |
+------------------+-------------------+--------------------+-----------------------+
```

### Metadata Fields
- `capsule_id`: ULID or UUIDv4 identifying the capsule run.
- `version`: Protocol version (currently `1`).
- `payload_kind`: One of `keys`, `knowledge_rag`, `model_weights`, `generic`.
- `created_at`: ISO 8601 UTC timestamp.
- `recipient_node`: Optional Ed25519/X25519 public key of the destination Xavier node.
- `filename_hint`: Original filename or logical directory descriptor.
- `uncompressed_len`: Length of the plaintext before optional Zstd compression.

---

## 3. Cryptographic Lifecycle

### Packaging (`pack`)
1. Plaintext payload is ingested (single file, tarball archive, or memory snapshot).
2. If targeted to a node, an ephemeral X25519 keypair is generated and shared secret derived via Diffie-Hellman.
3. Otherwise, Argon2id derives a 256-bit symmetric key from user passphrase and a cryptographically secure 16-byte random salt.
4. Payload is encrypted via AES-256-GCM with a 12-byte random nonce.
5. The container is written atomically to the destination path on the USB device.

### Unpacking (`unpack`)
1. Magic bytes (`SWALCAPS`) and version are validated.
2. Header metadata is deserialized.
3. Decryption key is derived via Argon2id using the header's salt, or reconstructed via node private key.
4. AES-256-GCM verifies authentication tag; fails atomically if corrupt.
5. Plaintext is restored directly into target directory or memory.

---

## 4. CLI Usage

### USB Device Detection
```bash
xavier airgap detect
```
Scans `/sys/block` and mount points to identify connected removable flash media and available storage.

### Packaging a Capsule
```bash
# Packaging wallet keys with passphrase
xavier airgap pack \
  --input ~/.xavier/vault/cold_keys.json \
  --output /media/usb/backup.swal_capsule \
  --kind keys \
  --passphrase "CorrectHorseBatteryStaple#2026"

# Packaging private RAG dataset for a specific legal node
xavier airgap pack \
  --input ./contracts_archive.tar.gz \
  --output /media/usb/contracts.swal_capsule \
  --kind knowledge \
  --target-node "xav_pub_8f29ab01..."
```

### Inspecting Capsule Metadata
```bash
xavier airgap inspect --capsule /media/usb/backup.swal_capsule
```

### Unpacking a Capsule
```bash
xavier airgap unpack \
  --capsule /media/usb/backup.swal_capsule \
  --output-dir ~/.xavier/vault/restored/ \
  --passphrase "CorrectHorseBatteryStaple#2026"
```