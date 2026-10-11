# Xavier Air-Gap Capsule Protocol Specification (`.swal_capsule`)

## 1. Overview & Threat Model

The **Xavier Air-Gap Capsule Protocol** provides a sovereign, air-gapped mechanism for archiving, packaging, and physically transferring confidential payloads across offline environments using removable USB storage or cold-storage media.

### Threat Model
- **Physical Loss or Theft:** An attacker intercepts the physical USB flash drive.
- **Untrusted Host Systems:** The capsule is transferred through an intermediary computer that may run surveillance or unauthorized indexing software.
- **Offline Brute-Force Attacks:** An adversary attempts high-throughput GPU/ASIC dictionary attacks against the encrypted container.
- **Tampering & Bit-Flipping:** An attacker modifies ciphertext bytes in an attempt to manipulate decrypted memory or trigger deserialization vulnerabilities.

### Security Guarantees
1. **Authenticated Encryption (AEAD):** AES-256-GCM guarantees both confidentiality and ciphertext integrity. Ciphertext authentication completes before output is written.
2. **Memory-Hard Key Derivation:** Argon2id uses 19,456 KiB memory, 2 iterations, and parallelism 1 to derive a 256-bit key from the passphrase. These parameters are pinned in the code.
3. **Header Transparency & Safe Inspection:** Capsule metadata (version, creation timestamp, payload kind, author) can be inspected without exposing the payload or requiring the passphrase.

---

## 2. Binary Capsule Structure

The existing version-1 `.swal_capsule` format consists of:

1. The 8-byte magic value `SWALCAPS`.
2. A 4-byte little-endian header length.
3. Plaintext JSON metadata of the declared length.
4. AES-256-GCM ciphertext, including its authentication tag.

### Metadata Fields

- `version`: Protocol version (currently `1`).
- `payload_kind`: `single_file`, `directory_archive`, `database_backup`, `memory_dump`, or `secrets_vault`.
- `original_filename`: Display metadata only; never an output path.
- `author`: Operator identity tag.
- `created_at`: UTC timestamp.
- `salt`: Hex-encoded 16-byte Argon2id salt.
- `nonce`: Hex-encoded 12-byte AES-GCM nonce.
- `ciphertext_length`: Length of the encrypted payload, including its authentication tag.

Inspection validates salt and nonce dimensions and returns an error for invalid metadata.

---

## 3. Cryptographic Lifecycle

### Packaging (`pack`)

Capsule creation is disabled pending a format redesign. `xavier airgap pack` remains parseable but exits non-zero without creating a capsule.

### Unpacking (`unpack`)

1. Magic bytes, version, and header metadata are validated.
2. Argon2id derives the key using the capsule salt and the supplied passphrase.
3. AES-256-GCM decryption and authentication complete in memory before any output is written.
4. The user-selected output path is validated. Header filenames may be displayed in sanitized form but never determine a filesystem path.
5. Single-file payloads are written to a new file. Directory archives are validated and extracted only into a new directory created by unpack.

Output paths must be relative and contain no `..` components. Symlinks and existing destination files are refused, as are protected Xavier key areas and `record.key`/`master.key` names (matched case-insensitively). Unpack refuses to run when the home directory cannot be resolved. Directory entries must remain within the extraction directory, must not be symlinks, and must not use protected key names. Files are created without overwriting existing files. There is no overwrite option.

Passphrases come from `--passphrase-file` or an interactive prompt. `-p`/`--passphrase` arguments are not accepted.

---

## 4. CLI Usage

### USB Device Detection
```bash
xavier airgap detect
```
Scans `/sys/block` and mount points to identify connected removable flash media and available storage.

### Capsule Creation

`xavier airgap pack` is disabled pending a format redesign.

### Inspecting Capsule Metadata
```bash
xavier airgap inspect --capsule /media/usb/backup.swal_capsule
```

### Unpacking a Capsule

For a single-file capsule, select an explicit output file:

```bash
xavier airgap unpack \
  --capsule ./backup.swal_capsule \
  --output ./restored-backup.json \
  --passphrase-file ./capsule-passphrase.txt
```

Alternatively, `--output-dir ./restored` uses `<capsule-file-stem>.out` inside an existing directory for single-file capsules. Directory capsules require `--output-dir` naming a new directory created by unpack. Omit `--passphrase-file` to use the interactive prompt.
