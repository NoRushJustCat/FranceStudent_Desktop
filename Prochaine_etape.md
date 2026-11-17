# Prochaine étape : agent de dev

Contexte : voir `Fait.md`. La session précédente a échoué (quota 402 et 429). Aucun `Plan.md` vérifié.

## Objectif

Produire `Plan.md` à partir de `PROJET.md`, puis implémenter les points suivants.

1. **Vérifier l'existant**
   - Confirmer si `Plan.md` existe dans `fscode/`.
   - Confirmer la présence du binaire `cloudflared` et son emplacement.

2. **Boucle d'agent**
   - Implémenter la boucle complète (envoi, `completed`, `message.toolCalls[]`, exécution des outils, `toolResults[]`, reprise du flux SSE).
   - Respecter `API_FranceStudent.md` et `client-tools.md` en entier.

3. **Brancher `bridge.rs`**
   - Ajouter `mod bridge;` dans `app/src-tauri/src/main.rs`.
   - Ajouter `Bridge` à `AppState`.
   - Enregistrer les commandes `bridge_status`, `bridge_start`, `bridge_stop`, `bridge_set` dans `generate_handler!`.
   - Appeler l'autostart au démarrage et `bridge::shutdown` sur `RunEvent::Exit`.

4. **Tunnel Cloudflare pour les MCP**
   - Chaque serveur MCP passe par un tunnel `cloudflared` sécurisé, créé via l'API France Student.
   - Retirer l'entrée et arrêter le tunnel à la fermeture de l'application.

5. **Contraintes**
   - Aucune injection dans le prompt : la configuration passe uniquement par les instructions personnalisées liées au compte France Student.
   - Ne pas modifier `fscode - Copie`, qui n'a pas de `src-tauri`.

## Vérifications attendues

- Compilation de `app/src-tauri` (`cargo check`).
- Lancement de l'app et état de `bridge_status`.
- Test d'un MCP à travers le tunnel.

## Points à confirmer avant de coder

- Les routes exactes du tunnel dans l'API France Student.
- Le format des instructions personnalisées.
