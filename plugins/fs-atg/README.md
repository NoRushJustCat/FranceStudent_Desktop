# Extension France Student pour Antigravity [ATG]

Plugin officiel permettant d'intégrer **France Student** directement dans Antigravity.

## Fonctionnalités

- **Délégation asynchrone intelligente (`fs_delegate` / `fs_job_status`)** : délègue la génération et la modification de code aux modèles de France Student (Opus, Sonnet, GPT, Kimi...).
- **Préservation du quota & continuité de contexte** :
  - **Même modèle = même discussion** : réutilise automatiquement la même discussion active (`activeChats`), ce qui conserve le fil de contexte et ne consomme aucun quota de nouvelle discussion (limite France Student de 50 chats/jour).
  - **Changement de modèle dynamique** : bascule sur la discussion dédiée à chaque modèle automatiquement.
  - **Réserve sanctuarisée** : conserve toujours une réserve de sécurité (15 discussions/jour) pour l'usage personnel de l'utilisateur sur France Student.
  - **Outil de réinitialisation (`fs_reset_chat`)** : permet de forcer une nouvelle discussion pour un modèle si nécessaire.
- **Écriture 100% fiable sur PC** :
  - Exécute les appels d'outils client `write_file` et `edit_file` directement dans le workspace local.
  - Extraction multi-formats robuste (`<<<FILE`, Markdown code fences, mapping automatique sur fichier cible).
  - Outil `fs_write_file` pour l'écriture ponctuelle directe.
- **Panneau latéral temps réel (Sidecar UI)** : visualisez en direct vos tâches, logs de raisonnement, utilisation des tokens et quotas restants.

## Installation

Dans Antigravity :
1. Copiez ce dossier dans votre répertoire de plugins (ex: `~/.gemini/config/plugins/fs-atg`).
2. Ouvrez les réglages ou le fichier `.fsatg_state.json` pour y renseigner votre token d'accès France Student.
3. Rechargez Antigravity : les outils `fs_delegate`, `fs_job_status`, `fs_quota`, `fs_reset_chat` et `fs_write_file` sont immédiatement disponibles.
