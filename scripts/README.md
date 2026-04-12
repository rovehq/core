# 📜 Scripts

Utility scripts for installation, key management, and deployment.

## 📦 Files

| Script                                      | Description                                    |
| ------------------------------------------- | ---------------------------------------------- |
| 🐧 [`install.sh`](./install.sh)             | macOS / Linux installer                        |
| 🪟 [`install.ps1`](./install.ps1)           | Windows PowerShell installer                   |
| 🗑️ [`uninstall.sh`](./uninstall.sh)         | macOS / Linux complete uninstaller             |
| 🗑️ [`uninstall.ps1`](./uninstall.ps1)       | Windows PowerShell uninstaller                 |
| 🔑 [`generate_keys.py`](./generate_keys.py) | Ed25519 keypair generator (stores in Keychain) |

## 🗑️ Uninstalling

```bash
# macOS / Linux
curl -fsSL https://roveai.co/uninstall.sh | sh

# Windows
irm https://roveai.co/uninstall.ps1 | iex
```

The uninstaller removes: binary, config, data, cache, plugins, database, and any daemon/service.

---

⬆️ [Back to root](../README.md)
