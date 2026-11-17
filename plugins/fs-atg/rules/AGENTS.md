# Délégation France Student [ATG]

- Pour générer plus de ~30 lignes de code (création ou modification de fichiers), utilise par défaut les outils MCP France Student (`fs_delegate` avec `workspaceRoot` si un workspace est ouvert, puis `fs_job_status` avec `waitSeconds=45`).
- Ne recopie JAMAIS le code généré dans ta réponse de chat : les fichiers sont écrits directement dans le workspace sur PC par `fs-atg`.
- Gestion des discussions & Quota de 50 chats/jour :
  * Pour un même modèle (ex: Opus ou Sonnet), réutilise automatiquement la même discussion active (`activeChats`), ce qui conserve le fil de contexte et ne consomme aucun quota de nouvelle discussion.
  * Quand on change de modèle, l'extension bascule sur la discussion dédiée à ce nouveau modèle.
  * Une réserve de sécurité (15 discussions/jour) est toujours préservée pour l'usage personnel de l'utilisateur sur France Student.
  * Pour forcer une nouvelle discussion pour un modèle si nécessaire, utilise `fs_reset_chat`.
- Écriture garantie sur PC :
  * `fs-atg` écrit systématiquement les fichiers sur disque via l'outil `write_file` ou par extraction multi-format automatique (`<<<FILE`, Markdown code blocks, ou ciblage direct de fichier demandé).
  * En cas d'écriture ponctuelle directe, l'outil `fs_write_file` est également disponible.
- Format du prompt :
  * Ne commence JAMAIS le prompt par « Tu es un agent de code délégué... » ni aucune mention de format machine.
  * Préfixe simplement par `[ATG]` suivi directement de la consigne claire, du nom du fichier ciblé et des spécifications techniques.
  * Exige toujours un niveau visuel et technique irréprochable : design moderne (synthwave/glassmorphism/néon, responsive), Web Audio API autonome sans crash, 60 FPS fluide, zéro variable non déclarée.
- Génération fichier par fichier (IMPÉRATIF) :
  * Pour un projet composé de plusieurs fichiers (ex: index.html, style.css, audio.js, main.js), procède STRICTEMENT fichier par fichier (1 fichier par message/tour : 1er message = index.html, 2e message = style.css, etc.).
  * Ne demande JAMAIS les 4 fichiers d'un coup dans le même message : cela évite les blocages, les réponses tronquées et les messages vides.
  * Pas besoin de passer manuellement le chatId pour le même modèle : l'extension s'en charge automatiquement en réutilisant le fil actif.
- Auto-correction & Qualité :
  * L'extension vérifie automatiquement la syntaxe et l'intégrité des fichiers générés (`node -c`, parsing JSON).
  * En cas d'erreur ou de bug détecté, l'extension déclenche automatiquement une boucle d'auto-correction dans la même discussion pour corriger immédiatement le fichier sur disque.
