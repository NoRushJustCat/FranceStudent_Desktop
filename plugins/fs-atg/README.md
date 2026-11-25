# France Student [ATG] (`fs-atg` v2.6.0)

Orchestrateur **France Student** complet pour **Antigravity** (MCP Server + panneau latéral Electron/Sidecar style **visionOS / Dynamic Island**).

Il délègue l’exploration, la planification, l’édition atomique de code (`/undo`), le raisonnement approfondi, la création d’artifacts Canvas, le pilotage bureau Windows (*Computer Use* + boucle Vision Live) et les recherches web directement aux modèles France Student afin d’économiser le quota Antigravity.

---

## Nouveautés majeures (v2.6.0)

- **Dock flottant bas & animations Liquid Spring Pill** : barre d’onglets (`Accueil`, `Modèles`, `Réglages`), sélecteurs de routage/permissions/raisonnement et politiques modèles animés par une pilule verre dépoli (`backdrop-filter: blur`) avec rebond physique.
- **Contrôle strict par modèle (`Oui` / `À demander` / `Non`)** :
  - `Oui` : sélection automatique autorisée ;
  - `À demander` : bloque l’exécution et retourne `status: "needs_approval"` avec approbation en 1 clic dans le panneau ou `approveModel="..."` ;
  - `Non` : interdiction absolue (`assertModelNotDenied`), même en secours ou en alias explicite.
- **Mistral hors quota (`Hors quota`)** : les modèles Mistral (`ministral-14b-latest`, `mistral*`, `codestral*`, `devstral*`) passent outre les quotas et la réserve (`cutoffPercent`). En cas de budget à `0 %`, sous réserve, ou sur `429 CHAT_BUDGET_EXHAUSTED`, bascule automatique et instantanée vers Mistral (avec repli propre en mode texte/patch si `clientTools` n’est pas supporté par le endpoint).
- **Computer Use & boucle Vision Live (`Live`)** :
  - Multi-écrans (`screen_info`, `screenshot` avec `screenIndex` / `deviceName`), liste/focus des fenêtres, souris, clavier, presse-papiers.
  - Upload réel via `POST /filesv2` et attachement aux tours `/chats/stream` pour une boucle itérative Antigravity ↔ France Student (`liveComputerMode`).
- **Raisonnement séquentiel (`fs_thinking`)** & **Canvas Live (`fs_canvas`)** : journal de pensée cross-processus (`.fsatg_thinking.json`) et prévisualisation HTML/SVG/Mermaid/3D.
- **Authentification robuste** : connexion web navigateur (Playwright) avec renouvellement automatique de session, migration sécurisée dans le **Gestionnaire d’identification Windows** (`windows_credential_store.ps1`), et clé API manuelle.
- **Historique étendu (50 tâches)**, télémétrie modèles (tok/s, TTFT, taux de succès, badge `Instable`), et suivi des économies de tokens Antigravity.

---

## Outils MCP exposés (`mcp_server.mjs`)

| Outil | Rôle |
|---|---|
| `fs_delegate` | Délègue une tâche avec routage intelligent (`Fort`, `Auto`, `Éco`), outils locaux `/undo`, Plan B et Vision Live |
| `fs_job_status` | Suivi temps réel d’un job (`waiting`, `running`, `needs_approval`, `done`, `failed`) |
| `fs_undo` | Annulation atomique de modifications disque |
| `fs_memory` | Lecture/écriture de la mémoire projet persistante (`FRST.md` / `FRSTCLI.md`) |
| `fs_canvas` | Création/prévisualisation d’artifacts HTML, SVG, Mermaid, 3D |
| `fs_computer` | Capture multi-écrans, gestion fenêtres, souris, clavier, presse-papiers |
| `fs_thinking` | Raisonnement structuré étape par étape avec hypothèses et branches |
| `fs_logs` | Consultation des journaux du connecteur |
| `fs_session` | Gestion et réinitialisation des sessions persistantes par projet |
| `fs_quota` | Lecture du budget restant, de l’heure de reset et du cooldown global |
| `fs_diagnostics` | Vérification auth, connectivité API, catalogue et télémétrie modèles |
| `fs_status` | État complet du connecteur et des créneaux de stream |
| `fs_catalog` | Découverte lecture seule (`/skills`, `/projects`, `/mcp/servers`, `/filesv2`) |
| `fs_cancel` | Annulation d’un job en cours |

---

## Installation rapide (Antigravity)

1. Copier ce dossier dans `%USERPROFILE%\.gemini\config\plugins\fs-atg`.
2. Installer les dépendances Node.js à la racine du plugin et du sidecar :
   ```powershell
   cd "$env:USERPROFILE\.gemini\config\plugins\fs-atg"
   npm install
   cd sidecars\fs_atg_panel
   npm install
   ```
3. Adapter `mcp_config.json` avec votre chemin utilisateur (`C:/Users/<USER>/.gemini/config/plugins/fs-atg/mcp_server.mjs`), puis activer le plugin `fs-atg` dans Antigravity.
4. Ouvrir le panneau **France Student [ATG]** et cliquer sur **Connexion navigateur** (ou coller votre clé / jeton).
