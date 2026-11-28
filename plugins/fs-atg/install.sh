#!/usr/bin/env bash
set -euo pipefail

echo ""
echo "=== Installation France Student [ATG] pour Antigravity ==="

if ! command -v node >/dev/null 2>&1; then
  echo "ERREUR : Node.js >= 18 est requis (https://nodejs.org/)." >&2
  exit 1
fi

echo "Node.js détecté : $(node -v)"

SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="${HOME}/.gemini/config/plugins/fs-atg"
mkdir -p "${TARGET_DIR}"

if [ "${SOURCE_DIR}" != "${TARGET_DIR}" ]; then
  echo "Copie des fichiers du plugin vers ${TARGET_DIR}..."
  if command -v rsync >/dev/null 2>&1; then
    rsync -a --exclude='node_modules' --exclude='.git' --exclude='.fsatg_*' --exclude='mcp_config.json' "${SOURCE_DIR}/" "${TARGET_DIR}/"
  else
    cp -R "${SOURCE_DIR}/." "${TARGET_DIR}/"
  fi
fi

cat > "${TARGET_DIR}/mcp_config.json" <<EOF
{
  "mcpServers": {
    "fs-atg": {
      "command": "node",
      "args": [
        "${TARGET_DIR}/mcp_server.mjs"
      ],
      "env": {
        "FS_ATG_IDE": "antigravity"
      }
    }
  }
}
EOF

echo "mcp_config.json généré automatiquement : ${TARGET_DIR}/mcp_server.mjs"
echo "Installation des dépendances npm..."
(cd "${TARGET_DIR}" && npm install --omit=dev --no-audit --no-fund)

echo ""
echo "Installation terminée avec succès !"
echo "Redémarre Antigravity puis connecte ton compte dans le panneau France Student [ATG]."
