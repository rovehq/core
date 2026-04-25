#!/usr/bin/env python3
"""
Rove key generator — two-key contract.

Generates Ed25519 keypairs in memory. Saves private keys to macOS Keychain.
Prints .env-format output ready for Infisical / GitHub Actions secret import.

Keys (both required):

  1. OFFICIAL   — signs engine releases, core-tools, official/reviewed plugins
                  & drivers, brains, revoked.json, community-manifest wrapper.
                  Embedded in engine binary as ROVE_TEAM_OFFICIAL_PUBLIC_KEY.

  2. COMMUNITY  — signs community-tier plugin manifests on PR merge only.
                  Embedded in engine binary as ROVE_TEAM_COMMUNITY_PUBLIC_KEY.
                  Separate blast radius from OFFICIAL.

Both keys are embedded in every engine build (stable + dev). Rove is still
pre-stable; dev channel is the only release target today. The stable channel
reuses the same embedded keys — channel split is about distribution cadence
and home dir (.rove-dev vs .rove), not key identity.

Usage:
    python3 scripts/generate_keys.py
    python3 scripts/generate_keys.py --env dev   # default
    python3 scripts/generate_keys.py --env prod
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from dataclasses import dataclass
from typing import List


@dataclass
class KeyDef:
    id: str
    name: str
    private_var: str
    public_var: str
    description: str
    used_by: List[str]


KEYS: List[KeyDef] = [
    KeyDef(
        id="official",
        name="Official Signing Key",
        private_var="ROVE_TEAM_OFFICIAL_PRIVATE_KEY",
        public_var="ROVE_TEAM_OFFICIAL_PUBLIC_KEY",
        description=(
            "Signs engine releases, core-tools (telegram / ui-server), "
            "official & reviewed plugins, drivers, brains, revoked.json, "
            "and the community-manifest wrapper.\n"
            "  Private → GitHub Secret on orvislab/rove, orvislab/rove-registry, orvislab/rove-plugins\n"
            "  Public  → compiled into engine via build.rs"
        ),
        used_by=[
            "core/.github/workflows/ci.yml        → signs engine + core-tools, embeds public key",
            "registry/.github/workflows/ci.yml    → signs per-channel per-artifact manifests",
            "plugins/.github/workflows/ci.yml     → signs official plugin manifests",
        ],
    ),
    KeyDef(
        id="community",
        name="Community Signing Key",
        private_var="ROVE_TEAM_COMMUNITY_PRIVATE_KEY",
        public_var="ROVE_TEAM_COMMUNITY_PUBLIC_KEY",
        description=(
            "Signs community-tier WASM plugin manifests on PR merge.\n"
            "  Authors hold no keys. Reviewer merge triggers CI sign step.\n"
            "  Private → GitHub Secret on orvislab/rove-community-plugins only\n"
            "  Public  → compiled into engine via build.rs"
        ),
        used_by=[
            "community-plugins/.github/workflows/ci.yml → signs community WASM manifests",
            "engine/build.rs                             → embeds public key into binary",
        ],
    ),
]

R2_SECRETS = [
    ("R2_ACCESS_KEY_ID", "Cloudflare Dashboard → R2 → API Tokens"),
    ("R2_SECRET_ACCESS_KEY", "Cloudflare Dashboard → R2 → API Tokens"),
    ("R2_API_URL", "https://<account-id>.r2.cloudflarestorage.com"),
    ("R2_BUCKET_NAME", "rove-registry"),
    ("REGISTRY_PAT", "GitHub PAT with write access to orvislab/rove-registry"),
]


def run_cmd(cmd: List[str], input_data: str | None = None) -> str:
    process = subprocess.run(cmd, input=input_data, capture_output=True, text=True)
    if process.returncode != 0:
        print(f"Error running: {' '.join(cmd)}", file=sys.stderr)
        print(process.stderr, file=sys.stderr)
        sys.exit(1)
    return process.stdout


def generate_ed25519() -> tuple[str, str]:
    priv = run_cmd(["openssl", "genpkey", "-algorithm", "ed25519"])
    pub = run_cmd(["openssl", "pkey", "-pubout"], input_data=priv)
    return priv.strip(), pub.strip()


def save_to_keychain(service: str, account: str, secret: str) -> None:
    run_cmd([
        "security", "add-generic-password",
        "-s", service, "-a", account, "-w", secret, "-U",
    ])


def strip_pem(value: str) -> str:
    return "".join(
        line for line in value.splitlines() if not line.startswith("-----")
    ).strip()


def main() -> None:
    parser = argparse.ArgumentParser(description="Rove key generator (two-key contract).")
    parser.add_argument(
        "--env",
        choices=["dev", "prod"],
        default="dev",
        help="Environment tag used for keychain service name (default: dev). "
             "Rove is pre-stable; keep dev unless cutting a real production keyset.",
    )
    parser.add_argument(
        "--non-interactive",
        action="store_true",
        help="Skip prompts and generate both keys.",
    )
    args = parser.parse_args()

    env = args.env

    print()
    print("╔══════════════════════════════════════════════════════╗")
    print("║           Rove Key Generator (2-key)                ║")
    print("╚══════════════════════════════════════════════════════╝")
    print()
    print(f"  Environment tag: {env.upper()}")
    print("  Keys generated in memory → macOS Keychain → printed as .env")
    print()

    print("━" * 60)
    print("  KEYS")
    print("━" * 60)
    for k in KEYS:
        print(f"\n  ★  {k.name}")
        print(f"      {k.description}")
        print(f"      Secrets:")
        print(f"        Private → {k.private_var}")
        print(f"        Public  → {k.public_var}")
        print(f"      Used by:")
        for u in k.used_by:
            print(f"        • {u}")

    print(f"\n{'━' * 60}")
    print("  R2 / REGISTRY CREDENTIALS (set manually in Cloudflare + GitHub)")
    print("━" * 60)
    for name, source in R2_SECRETS:
        print(f"  {name:<25} ← {source}")
    print()

    if not args.non_interactive:
        confirm = input("Generate both keys now? (Y/n): ").strip().lower()
        if confirm == "n":
            print("Aborted.")
            return

    print("\nGenerating keys in memory...")
    results: List[tuple[str, str, str]] = []
    for k in KEYS:
        priv, pub = generate_ed25519()
        service_name = f"rove-{k.id}-key-{env}"
        save_to_keychain(service_name, "rove-engine", priv)
        save_to_keychain(service_name + "-public", "rove-engine", pub)
        results.append((k.private_var, priv, k.name))
        results.append((k.public_var, pub, k.name))
        print(f"  ✓ {k.name} → Keychain service '{service_name}'")

    print(f"\n{'═' * 60}")
    print(f"  .env — paste into Infisical ({env.upper()} environment)")
    print("    or set directly as GitHub Actions secrets")
    print(f"{'═' * 60}\n")

    current_group = ""
    for name, value, group in results:
        if group != current_group:
            print(f"\n# --- {group} ---")
            current_group = group
        print(f'{name}="{strip_pem(value)}"')

    print()
    print(f"{'═' * 60}")
    print("  NEXT STEPS")
    print(f"{'═' * 60}")
    print()
    print("  1. Copy the OFFICIAL private key into GitHub Actions secrets on:")
    print("       • orvislab/rove                  (engine + core-tools)")
    print("       • orvislab/rove-registry         (per-channel manifest signing)")
    print("       • orvislab/rove-plugins          (official plugin manifests)")
    print("     Secret name: ROVE_TEAM_OFFICIAL_PRIVATE_KEY")
    print()
    print("  2. Copy the COMMUNITY private key into GitHub Actions secrets on:")
    print("       • orvislab/rove-community-plugins")
    print("     Secret name: ROVE_TEAM_COMMUNITY_PRIVATE_KEY")
    print()
    print("  3. Copy BOTH public keys into GitHub Actions secrets on:")
    print("       • orvislab/rove (for build.rs embedding)")
    print("     Secret names: ROVE_TEAM_OFFICIAL_PUBLIC_KEY, ROVE_TEAM_COMMUNITY_PUBLIC_KEY")
    print()
    print("  4. Private keys also persist in macOS Keychain under:")
    for k in KEYS:
        print(f"       rove-{k.id}-key-{env}")
    print()


if __name__ == "__main__":
    if sys.platform != "darwin":
        print("Error: macOS Keychain required. On Linux run openssl directly.", file=sys.stderr)
        sys.exit(1)
    try:
        main()
    except KeyboardInterrupt:
        print("\nAborted.")
        sys.exit(0)
