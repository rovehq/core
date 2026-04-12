#!/usr/bin/env python3
"""
Rove SECURE Key Generator
Generates Ed25519 keypairs entirely in memory.
Saves the Private Keys directly to macOS Keychain.
Outputs in .env format ready for Infisical import.
"""

import subprocess
import sys

# ─── Key definitions with usage context ──────────────────────────────────────

KEYS = [
    {
        "id": "signing",
        "name": "Manifest Signing Key",
        "required": True,
        "infisical_private": "ROVE_SIGNING_PRIVATE_KEY",
        "infisical_public": "ROVE_SIGNING_PUBLIC_KEY",
        "description": (
            "Signs registry/manifest.json, revoked.json, and all plugin manifests.\n"
            "  Private → Infisical + GitHub Actions (all repos)\n"
            "  Public  → Compiled into engine binary via build.rs (ROVE_TEAM_PUBLIC_KEY)"
        ),
        "used_by": [
            "core/.github/workflows/ci.yml        → signs manifests before R2 upload",
            "registry/.github/workflows/ci.yml    → signs manifest.json + revoked.json",
            "plugins/.github/workflows/ci.yml     → signs plugin hashes",
            "engine/build.rs                       → embeds public key into binary",
        ],
    },
    {
        "id": "core",
        "name": "Core Tool Signing Key",
        "required": False,
        "infisical_private": "ROVE_CORE_TOOL_PRIVATE_KEY",
        "infisical_public": "ROVE_CORE_TOOL_PUBLIC_KEY",
        "description": (
            "Signs native cdylib core tools (telegram, screenshot, etc).\n"
            "  Used when core tools are distributed separately from engine."
        ),
        "used_by": [
            "core/.github/workflows/ci.yml → signs core tool binaries",
        ],
    },
    {
        "id": "community",
        "name": "Community Plugin Key",
        "required": False,
        "infisical_private": "ROVE_COMMUNITY_PRIVATE_KEY",
        "infisical_public": "ROVE_COMMUNITY_PUBLIC_KEY",
        "description": (
            "Signs community-submitted WASM plugins.\n"
            "  Separate from official key to limit blast radius."
        ),
        "used_by": [
            "community-plugins/.github/workflows/ci.yml → signs community .wasm files",
        ],
    },
]

# ─── R2 / Infisical credentials (not generated, just listed) ────────────────

R2_SECRETS = [
    {"name": "R2_ACCESS_KEY_ID",     "source": "Cloudflare Dashboard → R2 → API Tokens"},
    {"name": "R2_SECRET_ACCESS_KEY", "source": "Cloudflare Dashboard → R2 → API Tokens"},
    {"name": "R2_API_URL",           "source": "https://<account-id>.r2.cloudflarestorage.com"},
    {"name": "R2_BUCKET_NAME",       "source": "rove-registry"},
]

# ─── Helpers ─────────────────────────────────────────────────────────────────

def run_cmd(cmd, input_data=None):
    process = subprocess.run(cmd, input=input_data, capture_output=True, text=True)
    if process.returncode != 0:
        print(f"Error running command: {' '.join(cmd)}")
        print(process.stderr)
        sys.exit(1)
    return process.stdout

def generate_ed25519():
    priv_key = run_cmd(["openssl", "genpkey", "-algorithm", "ed25519"])
    pub_key = run_cmd(["openssl", "pkey", "-pubout"], input_data=priv_key)
    return priv_key.strip(), pub_key.strip()

def save_to_keychain(service: str, account: str, secret: str):
    cmd = ["security", "add-generic-password", "-s", service, "-a", account, "-w", secret, "-U"]
    run_cmd(cmd)

def clean_pem(value: str) -> str:
    lines = [line for line in value.split('\n') if not line.startswith("-----")]
    return "".join(lines).strip()

# ─── Main ────────────────────────────────────────────────────────────────────

def main():
    print()
    print("╔══════════════════════════════════════════════════════╗")
    print("║         Rove Key & Secrets Generator                ║")
    print("╚══════════════════════════════════════════════════════╝")
    print()
    print("Keys are generated in memory → saved to macOS Keychain")
    print("→ printed in .env format for Infisical import.")
    print()

    # ── Show what's needed ───────────────────────
    print("━" * 60)
    print("  KEYS REQUIRED FOR CI/CD PIPELINE")
    print("━" * 60)
    for k in KEYS:
        tag = "★ REQUIRED" if k["required"] else "  optional"
        print(f"\n  {tag}  {k['name']}")
        print(f"          {k['description']}")
        print(f"          Infisical secrets:")
        print(f"            Private → {k['infisical_private']}")
        print(f"            Public  → {k['infisical_public']}")
        print(f"          Used by:")
        for u in k["used_by"]:
            print(f"            • {u}")

    print(f"\n{'━' * 60}")
    print("  R2 CREDENTIALS (not generated here — get from Cloudflare)")
    print("━" * 60)
    for s in R2_SECRETS:
        print(f"  {s['name']:<25} ← {s['source']}")

    print()

    # ── Environment ──────────────────────────────
    env_choice = ""
    while env_choice not in ['1', '2']:
        print("Select environment:")
        print("  1) Development (dev)")
        print("  2) Production (prod)")
        env_choice = input("Enter 1 or 2: ").strip()

    env = "dev" if env_choice == '1' else "prod"
    print(f"\n[Environment: {env.upper()}]\n")

    # ── Select keys to generate ──────────────────
    selected = []
    for k in KEYS:
        default = "Y/n" if k["required"] else "y/N"
        ans = input(f"Generate '{k['name']}'? ({default}): ").strip().lower()
        if k["required"] and ans != 'n':
            selected.append(k)
        elif not k["required"] and ans == 'y':
            selected.append(k)

    if not selected:
        print("\nNo keys selected. Exiting.")
        return

    # ── Generate ─────────────────────────────────
    print("\nGenerating keys in memory...")
    results = []
    for k in selected:
        priv, pub = generate_ed25519()

        service_name = f"rove-{k['id']}-key-{env}"
        save_to_keychain(service_name, "rove-engine", priv)

        results.append((k["infisical_private"], priv, k["name"]))
        results.append((k["infisical_public"], pub, k["name"]))
        print(f"  ✓ {k['name']}")

    # ── Output ───────────────────────────────────
    print(f"\n{'═' * 60}")
    print(f"  .env FORMAT — paste into Infisical ({env.upper()} environment)")
    print(f"{'═' * 60}\n")

    current_group = ""
    for name, value, group in results:
        if group != current_group:
            print(f"# --- {group} ---")
            current_group = group
        print(f'{name}="{clean_pem(value)}"')

    print(f"\n{'═' * 60}")
    print(f"\n[✓] Private keys saved to macOS Keychain")
    print(f"[→] Copy the output above → Infisical → Import as .env")
    print(f"[→] Then sync Infisical → GitHub Actions secrets for each repo")
    print(f"\nRepos that need these secrets:")
    print(f"  • orvislab/rove          (core engine + tools)")
    print(f"  • orvislab/rove-plugins  (official WASM plugins)")
    print(f"  • orvislab/rove-community-plugins")
    print(f"  • orvislab/rove-registry (manifest signing)")
    print()

if __name__ == "__main__":
    if sys.platform != "darwin":
        print("Error: This script requires macOS Keychain.")
        print("On Linux, use `openssl genpkey -algorithm ed25519` manually.")
        sys.exit(1)

    try:
        main()
    except KeyboardInterrupt:
        print("\n\nExiting...")
        sys.exit(0)
