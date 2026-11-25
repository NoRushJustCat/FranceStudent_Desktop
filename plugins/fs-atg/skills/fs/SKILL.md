---
name: fs
description: Déléguer à France Student avec écriture directe dans workspaceRoot et sauvegarde /undo.
---

# /fs

Appelle immédiatement `fs_delegate` avec `/fs ` + une consigne complète et précise (n’hésite pas à envoyer de longs prompts détaillés : France Student gère très bien les gros cahiers des charges et produit 200–1000+ lignes sans problème), `workspaceRoot` (chemin absolu du projet), `contextMode="minimal"` et `waitSeconds=110` **sans appeler `read_file` avant**. En mode `acceptEdits`/`full`, France Student lit et écrit directement sur disque avec sauvegarde atomique `/undo` : présente le bilan (`filesWritten`, `commandsRun`) sans relire ni rééditer les fichiers. En mode `plan`, applique les blocs retournés via `edit_file` (`planB: true` + `parts` si échec). N’appelle `fs_job_status` (`waitSeconds=110`) que si `status` est `waiting` ou `running`.
