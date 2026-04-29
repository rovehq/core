#!/usr/bin/env python3
"""
Sign an engine/core-tools manifest JSON with the official Ed25519 key.

Writes <manifest>.sig alongside the manifest (hex-encoded Ed25519 signature
over the canonical JSON — same canonicalization used by verify_manifest_signature
in update.rs: strip signature+signed_at, sort keys, compact JSON).

Usage:
  OFFICIAL_KEY_HEX=<hex> python3 scripts/release/sign-manifest.py manifest.json [manifest2.json ...]

  # Or: write PEM first, then sign
  python3 scripts/release/sign-manifest.py --make-pem-only
  python3 scripts/release/sign-manifest.py manifest.json
"""

import argparse
import base64
import json
import os
import subprocess
import sys
from pathlib import Path

PKCS8_PREFIX = bytes.fromhex("302e020100300506032b657004220420")
PEM_PATH = "/tmp/rove_official.pem"


def make_pem(key_hex: str) -> None:
    cleaned = key_hex.strip()
    raw = bytes.fromhex(cleaned)
    if len(raw) == 48 and raw[:16] == PKCS8_PREFIX:
        raw = raw[16:]
    elif len(raw) != 32:
        sys.exit(f"key seed must be 32 bytes, got {len(raw)}")
    der = PKCS8_PREFIX + raw
    pem = (
        "-----BEGIN PRIVATE KEY-----\n"
        + base64.encodebytes(der).decode()
        + "-----END PRIVATE KEY-----\n"
    )
    Path(PEM_PATH).write_text(pem)
    print(f"PEM written to {PEM_PATH}")


def sign_manifest(manifest_path: Path) -> None:
    d = json.loads(manifest_path.read_text())
    d.pop("signature", None)
    d.pop("signed_at", None)
    canon = json.dumps(d, sort_keys=True, separators=(",", ":")).encode()
    canon_file = Path("/tmp/rove_canon.bin")
    sig_file = Path("/tmp/rove_sig.bin")
    canon_file.write_bytes(canon)
    subprocess.run(
        [
            "openssl", "pkeyutl", "-sign", "-rawin",
            "-inkey", PEM_PATH,
            "-in", str(canon_file),
            "-out", str(sig_file),
        ],
        check=True,
    )
    sig = sig_file.read_bytes().hex()
    out = Path(str(manifest_path) + ".sig")
    out.write_text(sig)
    print(f"  signed {manifest_path.name} → {out.name}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("manifests", nargs="*", help="manifest.json files to sign")
    parser.add_argument("--make-pem-only", action="store_true")
    args = parser.parse_args()

    key_hex = os.environ.get("OFFICIAL_KEY_HEX", "").strip()

    if not Path(PEM_PATH).exists() or args.make_pem_only:
        if not key_hex:
            sys.exit("OFFICIAL_KEY_HEX env var required")
        make_pem(key_hex)

    if args.make_pem_only:
        return

    if not args.manifests:
        sys.exit("No manifest files specified")

    for path_str in args.manifests:
        sign_manifest(Path(path_str))


if __name__ == "__main__":
    main()
