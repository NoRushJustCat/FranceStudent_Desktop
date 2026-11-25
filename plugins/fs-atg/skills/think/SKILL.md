---
name: think
description: Raisonnement approfondi pas-à-pas (Sequential Thinking) avec hypothèses, révisions et branches via fs_thinking.
---

Utilise l'outil MCP `fs_thinking` pour structurer un raisonnement approfondi étape par étape (décomposition, hypothèse, vérification, révision ou branche alternative) avant d'écrire du code complexe ou de prendre une décision d'architecture.

- `action: "think"` avec `thought`, `thoughtNumber`, `totalThoughts`, `nextThoughtNeeded` (et optionnellement `isRevision`, `revisesThought`, `branchId`, `hypothesis`, `verified`).
- `action: "history"` pour relire la chaîne de pensée active.
- `action: "clear"` pour repartir sur un nouveau raisonnement.
