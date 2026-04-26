#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "infisicalsdk>=1.0.0",
# ]
# ///
"""
Rove key generator — two-key contract with Infisical auto-push.

Generates Ed25519 keypairs in memory:

  1. OFFICIAL   — signs engine releases, core-tools, official/reviewed plugins
                  & drivers, brains, revoked.json, community-manifest wrapper.
                  Embedded in engine binary as ROVE_TEAM_OFFICIAL_PUBLIC_KEY.

  2. COMMUNITY  — signs community-tier plugin manifests on PR merge only.
                  Embedded in engine binary as ROVE_TEAM_COMMUNITY_PUBLIC_KEY.
                  Separate blast radius from OFFICIAL.

Flow:
  1. openssl genpkey in memory → Ed25519 private + public
  2. Private key → macOS Keychain (backup)
  3. Private + public → Infisical via SDK (source of truth for CI sync)
  4. Public keys optionally written to manifest/ for local reproducible builds

Requires:
  uv (https://docs.astral.sh/uv/) — shebang runs via `uv run --script`
  openssl CLI in PATH
  macOS (Keychain). Pass --no-keychain to skip on Linux/CI.

Usage:
  # Reads Infisical creds from scripts/release/.env.local (preferred):
  ./scripts/release/generate_keys.py --env dev --write-public core --yes

  # Or explicitly via uv:
  uv run scripts/release/generate_keys.py --env dev --write-public core --yes

  # Process-env override:
  INFISICAL_CLIENT_ID=... \\
  INFISICAL_CLIENT_SECRET=... \\
  INFISICAL_PROJECT_ID=... \\
  uv run scripts/release/generate_keys.py --env dev --yes

.env.local (scripts/release/.env.local — gitignored):
  INFISICAL_HOST=https://app.infisical.com
  INFISICAL_CLIENT_ID=...
  INFISICAL_CLIENT_SECRET=...
  INFISICAL_PROJECT_ID=...
  INFISICAL_ENVIRONMENT=dev
  INFISICAL_SECRET_PATH=/

Override file location with ROVE_ENV_FILE=/path/to/env.

Flags:
  --env dev|prod            Infisical environment slug + keychain tag (default dev)
  --yes                     skip confirmation prompts
  --no-keychain             skip macOS Keychain write
  --no-infisical            skip Infisical push (print-only fallback)
  --write-public <path>     write public keys as raw 32-byte .bin under <path>/manifest/
"""

from __future__ import annotations

import argparse
import base64
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional


# ─── .env.local auto-load ────────────────────────────────────────────────────
# Loaded before arg parse so env-backed prompts can resolve non-interactively.
#
# Lookup order (first hit wins):
#   1. ROVE_ENV_FILE env var (explicit override)
#   2. scripts/release/.env.local relative to this script
#   3. .env.local in CWD
#
# Simple parser — no python-dotenv dep. Supports: KEY=value, KEY="value with spaces",
# lines beginning # ignored, blank lines ignored. Existing env vars are NOT overridden.

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


@dataclass
class KeyDef:
    id: str
    name: str
    private_var: str
    public_var: str


KEYS: List[KeyDef] = [
    KeyDef(
        id="official",
        name="Official Signing Key",
        private_var="ROVE_TEAM_OFFICIAL_PRIVATE_KEY",
        public_var="ROVE_TEAM_OFFICIAL_PUBLIC_KEY",
    ),
    KeyDef(
        id="community",
        name="Community Signing Key",
        private_var="ROVE_TEAM_COMMUNITY_PRIVATE_KEY",
        public_var="ROVE_TEAM_COMMUNITY_PUBLIC_KEY",
    ),
]


# ─── OpenSSL helpers ─────────────────────────────────────────────────────────

def run_cmd(cmd: List[str], input_data: Optional[str] = None) -> str:
    p = subprocess.run(cmd, input=input_data, capture_output=True, text=True)
    if p.returncode != 0:
        print(f"error: {' '.join(cmd)}", file=sys.stderr)
        print(p.stderr, file=sys.stderr)
        sys.exit(1)
    return p.stdout


def generate_ed25519() -> tuple[str, str]:
    priv_pem = run_cmd(["openssl", "genpkey", "-algorithm", "ed25519"]).strip()
    pub_pem = run_cmd(["openssl", "pkey", "-pubout"], input_data=priv_pem).strip()
    return priv_pem, pub_pem


def public_key_raw_bytes(pub_pem: str) -> bytes:
    """PEM public key → raw 32-byte Ed25519 key.

    Ed25519 public key DER = 12-byte ASN.1 header + 32-byte raw key.
    """
    b64 = "".join(l for l in pub_pem.splitlines() if not l.startswith("-----"))
    der = base64.b64decode(b64)
    if len(der) != 44:
        raise ValueError(f"unexpected Ed25519 DER length {len(der)} (want 44)")
    return der[12:]


def strip_pem(value: str) -> str:
    return "".join(
        line for line in value.splitlines() if not line.startswith("-----")
    ).strip()


# ─── Keychain ────────────────────────────────────────────────────────────────

def save_to_keychain(service: str, account: str, secret: str) -> None:
    run_cmd([
        "security", "add-generic-password",
        "-s", service, "-a", account, "-w", secret, "-U",
    ])


# ─── Infisical ───────────────────────────────────────────────────────────────

def push_to_infisical(
    host: str,
    client_id: str,
    client_secret: str,
    project_id: str,
    environment: str,
    secret_path: str,
    kv: dict,
) -> None:
    try:
        from infisical_sdk import InfisicalSDKClient
    except ImportError:
        print("error: infisical-sdk not resolved. Run via `uv run scripts/release/generate_keys.py`.", file=sys.stderr)
        sys.exit(1)

    client = InfisicalSDKClient(host=host)
    client.auth.universal_auth.login(client_id=client_id, client_secret=client_secret)

    # Fetch existing secrets → decide create vs update. Broad catch survives
    # SDK shape drift between versions.
    existing_names = set()
    try:
        existing = client.secrets.list_secrets(
            project_id=project_id,
            environment_slug=environment,
            secret_path=secret_path,
        )
        items = getattr(existing, "secrets", existing)
        for s in items:
            name = getattr(s, "secret_key", None) or getattr(s, "secretKey", None)
            if name:
                existing_names.add(name)
    except Exception as e:
        print(f"warn: list_secrets failed ({e}); treating all as new", file=sys.stderr)

    for key, value in kv.items():
        try:
            if key in existing_names:
                client.secrets.update_secret_by_name(
                    secret_name=key,
                    secret_value=value,
                    project_id=project_id,
                    environment_slug=environment,
                    secret_path=secret_path,
                )
                print(f"  ↑ updated {key} in Infisical")
            else:
                client.secrets.create_secret_by_name(
                    secret_name=key,
                    secret_value=value,
                    project_id=project_id,
                    environment_slug=environment,
                    secret_path=secret_path,
                )
                print(f"  + created {key} in Infisical")
        except Exception as e:
            print(f"  ✗ {key} failed: {e}", file=sys.stderr)


def prompt(var_name: str, label: str, secret: bool = False) -> str:
    v = os.environ.get(var_name)
    if v:
        return v
    if secret:
        import getpass
        return getpass.getpass(f"{label}: ").strip()
    return input(f"{label}: ").strip()


# ─── Main ────────────────────────────────────────────────────────────────────

def main() -> None:
    parser = argparse.ArgumentParser(description="Rove key generator + Infisical push.")
    parser.add_argument("--env", choices=["dev", "prod"], default="dev")
    parser.add_argument("--yes", action="store_true", help="Skip confirmation.")
    parser.add_argument("--no-keychain", action="store_true")
    parser.add_argument("--no-infisical", action="store_true")
    parser.add_argument("--write-public", metavar="DIR",
                        help="Write raw 32-byte .bin public keys under DIR/manifest/.")
    args = parser.parse_args()

    env = args.env
    print(f"\n=== Rove key generator · env={env.upper()} ===\n")

    # Infisical creds
    infisical_cfg = None
    if not args.no_infisical:
        host = os.environ.get("INFISICAL_HOST", "https://app.infisical.com")
        print(f"Infisical host: {host}")
        print("(Set INFISICAL_CLIENT_ID / INFISICAL_CLIENT_SECRET / INFISICAL_PROJECT_ID to skip prompts.)\n")
        infisical_cfg = {
            "host": host,
            "client_id": prompt("INFISICAL_CLIENT_ID", "Machine identity client ID"),
            "client_secret": prompt("INFISICAL_CLIENT_SECRET", "Machine identity client secret", secret=True),
            "project_id": prompt("INFISICAL_PROJECT_ID", "Project ID"),
            "environment": os.environ.get("INFISICAL_ENVIRONMENT", env),
            "secret_path": os.environ.get("INFISICAL_SECRET_PATH", "/"),
        }
        missing = [k for k in ("client_id", "client_secret", "project_id") if not infisical_cfg[k]]
        if missing:
            print(f"error: Infisical creds incomplete ({missing}). Re-run with --no-infisical to skip push.", file=sys.stderr)
            sys.exit(1)

    # Confirm
    if not args.yes:
        sinks = ["memory"]
        if sys.platform == "darwin" and not args.no_keychain:
            sinks.append("macOS Keychain")
        if infisical_cfg:
            sinks.append(f"Infisical ({infisical_cfg['environment']} / {infisical_cfg['secret_path']})")
        if args.write_public:
            sinks.append(f"{args.write_public}/manifest/ (public only)")
        print("Keys will be stored in: " + ", ".join(sinks))
        if input("Proceed? (Y/n): ").strip().lower() == "n":
            print("Aborted.")
            return

    kv_to_push: dict = {}
    print("\nGenerating Ed25519 keypairs...")
    for k in KEYS:
        priv_pem, pub_pem = generate_ed25519()
        priv_flat = strip_pem(priv_pem)
        pub_flat = strip_pem(pub_pem)

        # Keychain
        if sys.platform == "darwin" and not args.no_keychain:
            service = f"rove-{k.id}-key-{env}"
            save_to_keychain(service, "rove-engine", priv_pem)
            save_to_keychain(f"{service}-public", "rove-engine", pub_pem)
            print(f"  ✓ {k.name} → Keychain '{service}'")

        # Public key → manifest/*.bin
        if args.write_public:
            manifest_dir = Path(args.write_public) / "manifest"
            manifest_dir.mkdir(parents=True, exist_ok=True)
            raw = public_key_raw_bytes(pub_pem)
            bin_path = manifest_dir / f"team_{k.id}_public_key.bin"
            bin_path.write_bytes(raw)
            print(f"  ✓ {k.name} public → {bin_path}")

        kv_to_push[k.private_var] = priv_flat
        kv_to_push[k.public_var] = pub_flat

    if infisical_cfg:
        print(f"\nPushing to Infisical [{infisical_cfg['environment']}] {infisical_cfg['secret_path']}...")
        push_to_infisical(kv=kv_to_push, **infisical_cfg)
    else:
        print("\n--no-infisical: skipping push. Secrets to set manually:")
        for key in kv_to_push:
            print(f"  {key}")

    print("\n" + "═" * 60)
    print("  NEXT STEPS")
    print("═" * 60)
    print()
    print("  Sync Infisical → GitHub Actions secrets (via Infisical integration):")
    print("    ROVE_TEAM_OFFICIAL_PRIVATE_KEY  →  orvislab/rove-registry")
    print("    ROVE_TEAM_COMMUNITY_PRIVATE_KEY →  orvislab/rove-community-plugins")
    print("    ROVE_TEAM_OFFICIAL_PUBLIC_KEY   →  orvislab/rove (build.rs embed)")
    print("    ROVE_TEAM_COMMUNITY_PUBLIC_KEY  →  orvislab/rove (build.rs embed)")
    print()
    if args.write_public:
        print(f"  Public key .bin files written under {args.write_public}/manifest/")
        print("  Commit these so local builds reproduce without GitHub secrets.")
        print()
    if sys.platform == "darwin" and not args.no_keychain:
        print("  Keychain services created:")
        for k in KEYS:
            print(f"    rove-{k.id}-key-{env}")
        print()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nAborted.")
        sys.exit(0)
