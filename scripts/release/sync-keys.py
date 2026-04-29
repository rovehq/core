#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "infisicalsdk>=1.0.0",
# ]
# ///
"""
Sync public keys from Infisical → core/manifest/*.bin

Reads ROVE_TEAM_OFFICIAL_PUBLIC_KEY and ROVE_TEAM_COMMUNITY_PUBLIC_KEY from
Infisical and writes raw 32-byte public key files to <core>/manifest/.

These .bin files are embedded by build.rs into the rove binary at compile time,
so the binary can verify signatures made by CI.

Usage:
  # From core/ root (reads scripts/release/.env.local automatically):
  uv run scripts/release/sync-keys.py

  # Explicit output dir:
  uv run scripts/release/sync-keys.py --write-public /path/to/core

  # Skip Infisical, read from env vars directly:
  ROVE_TEAM_OFFICIAL_PUBLIC_KEY=<hex> \\
  ROVE_TEAM_COMMUNITY_PUBLIC_KEY=<hex> \\
  uv run scripts/release/sync-keys.py --from-env

.env.local (scripts/release/.env.local — gitignored):
  INFISICAL_HOST=https://app.infisical.com
  INFISICAL_CLIENT_ID=...
  INFISICAL_CLIENT_SECRET=...
  INFISICAL_PROJECT_ID=...
  INFISICAL_ENVIRONMENT=dev
  INFISICAL_SECRET_PATH=/
"""

from __future__ import annotations

import argparse
import base64
import os
import sys
from pathlib import Path
from typing import List, Optional


# ─── .env.local auto-load (same logic as generate_keys.py) ──────────────────

def load_env_file(path: Path) -> int:
    if not path.is_file():
        return 0
    loaded = 0
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip()
        if (value.startswith('"') and value.endswith('"')) or (
            value.startswith("'") and value.endswith("'")
        ):
            value = value[1:-1]
        if key and key not in os.environ:
            os.environ[key] = value
            loaded += 1
    return loaded


def _autoload_env() -> None:
    candidates: List[Path] = []
    override = os.environ.get("ROVE_ENV_FILE")
    if override:
        candidates.append(Path(override))
    script_dir = Path(__file__).resolve().parent
    candidates.append(script_dir / ".env.local")
    candidates.append(Path.cwd() / ".env.local")

    seen: set = set()
    for p in candidates:
        rp = p.resolve() if p.exists() else p
        if rp in seen:
            continue
        seen.add(rp)
        if p.is_file():
            n = load_env_file(p)
            if n > 0:
                print(f"[env] loaded {n} var(s) from {p}")


_autoload_env()


# ─── Key parsing ─────────────────────────────────────────────────────────────

# Ed25519 SubjectPublicKeyInfo DER prefix (12 bytes)
SPKI_PREFIX = bytes.fromhex("302a300506032b6570032100")
# Ed25519 PKCS8 private key DER prefix (16 bytes)
PKCS8_PREFIX = bytes.fromhex("302e020100300506032b657004220420")


def public_key_raw_bytes(value: str) -> bytes:
    """Parse a public key in any supported format → raw 32 bytes.

    Accepts:
      - 64-char hex (raw 32-byte key as hex)
      - base64-encoded SubjectPublicKeyInfo DER (44 bytes: 12-byte prefix + 32)
      - base64-encoded raw 32 bytes
    """
    v = value.strip()

    # Hex: 64 chars = 32 bytes raw key
    try:
        raw = bytes.fromhex(v)
        if len(raw) == 32:
            return raw
        if len(raw) == 44 and raw[:12] == SPKI_PREFIX:
            return raw[12:]
        # 48-byte PKCS8 private key DER — caller passed wrong key
        if len(raw) == 48 and raw[:16] == PKCS8_PREFIX:
            raise ValueError("Got a PRIVATE key hex — need the PUBLIC key")
    except ValueError as e:
        if "PRIVATE" in str(e):
            raise
        pass

    # Base64
    try:
        der = base64.b64decode(v)
        if len(der) == 32:
            return der
        if len(der) == 44 and der[:12] == SPKI_PREFIX:
            return der[12:]
        # PEM body without headers (base64 of DER)
        raise ValueError(f"Unexpected base64 key length {len(der)}")
    except Exception as e2:
        raise ValueError(
            f"Cannot parse public key (len={len(v)} chars): {e2}"
        ) from None


# ─── Infisical fetch ──────────────────────────────────────────────────────────

KEY_VARS = [
    "ROVE_TEAM_OFFICIAL_PUBLIC_KEY",
    "ROVE_TEAM_COMMUNITY_PUBLIC_KEY",
]

BIN_NAMES = {
    "ROVE_TEAM_OFFICIAL_PUBLIC_KEY":  "team_official_public_key.bin",
    "ROVE_TEAM_COMMUNITY_PUBLIC_KEY": "team_community_public_key.bin",
}


def fetch_from_infisical(
    host: str,
    client_id: str,
    client_secret: str,
    project_id: str,
    environment: str,
    secret_path: str,
) -> dict[str, str]:
    try:
        from infisical_sdk import InfisicalSDKClient
    except ImportError:
        print("error: run via `uv run scripts/release/sync-keys.py`", file=sys.stderr)
        sys.exit(1)

    client = InfisicalSDKClient(host=host)
    client.auth.universal_auth.login(
        client_id=client_id, client_secret=client_secret
    )

    result: dict[str, str] = {}
    for name in KEY_VARS:
        try:
            secret = client.secrets.get_secret_by_name(
                secret_name=name,
                project_id=project_id,
                environment_slug=environment,
                secret_path=secret_path,
            )
            value = getattr(secret, "secret_value", None) or getattr(secret, "secretValue", None)
            if value:
                result[name] = value
            else:
                print(f"  warn: {name} is empty in Infisical", file=sys.stderr)
        except Exception as e:
            print(f"  warn: could not fetch {name}: {e}", file=sys.stderr)

    return result


def fetch_from_env() -> dict[str, str]:
    result = {}
    for name in KEY_VARS:
        v = os.environ.get(name, "").strip()
        if v:
            result[name] = v
    return result


# ─── Main ─────────────────────────────────────────────────────────────────────

def main() -> None:
    parser = argparse.ArgumentParser(
        description="Sync public keys from Infisical → manifest/*.bin"
    )
    parser.add_argument(
        "--write-public",
        metavar="DIR",
        default=None,
        help="Root dir containing manifest/ (default: auto-detect core root)",
    )
    parser.add_argument(
        "--from-env",
        action="store_true",
        help="Read keys from env vars instead of Infisical",
    )
    parser.add_argument(
        "--env",
        default=os.environ.get("INFISICAL_ENVIRONMENT", "dev"),
        help="Infisical environment slug (default: dev)",
    )
    args = parser.parse_args()

    # Resolve output dir
    if args.write_public:
        core_root = Path(args.write_public)
    else:
        # Auto: walk up from script dir to find manifest/
        script_dir = Path(__file__).resolve().parent
        core_root = script_dir.parent.parent  # scripts/release/ → core/
    manifest_dir = core_root / "manifest"

    if not manifest_dir.exists():
        print(f"error: manifest dir not found at {manifest_dir}", file=sys.stderr)
        print("Pass --write-public <core-root> explicitly.", file=sys.stderr)
        sys.exit(1)

    print(f"Writing to: {manifest_dir}/")

    # Fetch keys
    if args.from_env:
        print("Reading keys from environment variables...")
        kv = fetch_from_env()
    else:
        host = os.environ.get("INFISICAL_HOST", "https://app.infisical.com")
        client_id = os.environ.get("INFISICAL_CLIENT_ID", "")
        client_secret = os.environ.get("INFISICAL_CLIENT_SECRET", "")
        project_id = os.environ.get("INFISICAL_PROJECT_ID", "")
        secret_path = os.environ.get("INFISICAL_SECRET_PATH", "/")

        missing = [k for k, v in [
            ("INFISICAL_CLIENT_ID", client_id),
            ("INFISICAL_CLIENT_SECRET", client_secret),
            ("INFISICAL_PROJECT_ID", project_id),
        ] if not v]
        if missing:
            print(
                f"error: Infisical creds missing: {missing}\n"
                "Add to scripts/release/.env.local or pass --from-env.",
                file=sys.stderr,
            )
            sys.exit(1)

        print(f"Fetching from Infisical [{args.env}] {host}...")
        kv = fetch_from_infisical(host, client_id, client_secret, project_id, args.env, secret_path)

    if not kv:
        print("error: no keys retrieved", file=sys.stderr)
        sys.exit(1)

    # Parse and write
    wrote = 0
    for var_name, raw_value in kv.items():
        bin_name = BIN_NAMES.get(var_name)
        if not bin_name:
            continue
        try:
            raw = public_key_raw_bytes(raw_value)
        except ValueError as e:
            print(f"  error: {var_name}: {e}", file=sys.stderr)
            continue

        out_path = manifest_dir / bin_name
        out_path.write_bytes(raw)
        print(f"  wrote {bin_name}  ({raw.hex()[:16]}...)")
        wrote += 1

    if wrote == 0:
        print("error: nothing written", file=sys.stderr)
        sys.exit(1)

    print(f"\nDone. Rebuild rove to embed updated keys:")
    print(f"  cargo build -p engine")
    print(f"\nThen test against real dev registry:")
    print(f"  rove plugin install fs-editor --registry https://registry.roveai.co/dev/extensions")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nAborted.")
        sys.exit(0)
