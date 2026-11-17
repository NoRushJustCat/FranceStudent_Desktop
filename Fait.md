# Fait : historique de la session « Plan.md, MCP via tunnel Cloudflare »

Source : `Fait.json`. Dossier de travail : `E:\Dev\FranceStudentDesktop`. Issue de la session : **échec** (`outcome: failed`).

> Périmètre : lecture du début du log (lignes 1 à 2100) et de la fin (lignes 27250 à 27571). Le centre du fichier (environ 25 000 lignes) n'a pas été relu, la recherche dans ce fichier n'ayant rien renvoyé. La liste des modifications ci-dessous peut donc être incomplète.

## 1. Demande initiale

- Créer `Plan.md` à partir de `PROJET.md` (dossier `fscode`) avec :
  - une boucle d'agent fonctionnelle ;
  - une utilisation complète de `API_FranceStudent.md` et de `client-tools.md` ;
  - des serveurs MCP exposés via un tunnel Cloudflare sécurisé, passant par l'API France Student (le binaire `cloudflared` devait être fourni par l'utilisateur) ;
  - aucune injection dans le prompt : tout passe par les instructions personnalisées liées au compte France Student.

## 2. Travail effectué (vérifié dans le log)

1. **Exploration** : listage du dossier de travail, recherche des fichiers `.md`.
2. **Lecture** : `PROJET.md`, `FSCODE_NOTES.md`, `API_FranceStudent.md` (lignes 1 à 1023), `client-tools.md`.
3. **Constats sur le code** :
   - `bridge.rs` existe dans `app/src-tauri/src` mais n'est pas branché dans `main.rs` : pas de `mod bridge`, commandes non enregistrées, pas d'autostart ni d'arrêt, `Bridge` absent de `AppState`.
   - Côté interface, `Bridge.svelte` appelle `bridge_status`, `bridge_start`, `bridge_stop` et `bridge_set`.
   - Le dossier `fscode - Copie` n'a pas de `src-tauri` : `fscode` est donc la version principale.
   - `fs-store` stocke les réglages dans `config.json` : `base_url`, `last_agent_id`, `last_workdir`, `auto_accept`, `eco`.
4. **Inspection** du dossier `.mcp-history` (fichiers `.json` et `.bin`), sans interprétation.

## 3. Modifications de fichiers

- Aucune écriture de fichier n'a été trouvée dans les parties relues. Les outils appelés étaient en lecture seule.

## 4. Fin de session

- Plusieurs changements de modèle : `deepseek-v4.1-flash-free`, `hy4-flash-free`, `gemini-4-hydrogen`, `gemini-3.8-flash`.
- La demande finale, répétée plusieurs fois, était de faire la phase suivante sans l'utilisateur, puis de produire ce fichier `.md` récapitulatif.
- Toutes les tentatives finales ont échoué :
  - erreur 402 : solde insuffisant sur OpenCode ;
  - erreur 429 : quota gratuit Google dépassé pour `gemini-4-hydrogen`, puis pour `gemini-3.8-flash`.
- Aucun `Plan.md` n'apparaît dans les parties relues. Son existence reste à vérifier dans le dossier.

## 5. Reste à faire

- Recharger du crédit sur OpenCode ou attendre la réinitialisation du quota Google, puis relancer la session.
- Relire le centre du log pour compléter la section 3 (modifications de fichiers).
- Vérifier la présence de `Plan.md` et du binaire `cloudflared`.
- Brancher `bridge.rs` dans `main.rs` si la phase suivante l'exige.

## Mise à jour : Phase 2 (backend)
- pp/src-tauri/src/instructions.rs : bloc auto balisé dans les instructions du compte (≤ 10 000 car., sans secret), état instructions-state.json, restauration exacte, retrait au logout et à la fermeture, resynchronisation au démarrage et au changement des serveurs MCP.
- Commandes Tauri : instructions_status, instructions_sync, instructions_remove, instructions_set_enabled. Réglage sync_instructions (défaut true) dans s-store.
- Vérifié : cargo check --workspace OK, cargo test --workspace OK (35 tests dans fs-code, dont 10 pour instructions).
- Reste : UI Settings > Instructions, Phase 4 (couverture API), Phase 5.

## Mise à jour : Phases 4 et 5 (partielles)
- Phase 4 : crates/fs-api/src/routes.rs (catalogue des routes documentées, risque, paramètres autorisés), Client::call_route, pp/src-tauri/src/api_explorer.rs (pi_routes, pi_call), UI ApiExplorer.svelte (onglet Paramètres > API). ccount_skill_update utilise désormais POST (conforme à la doc).
- Phase 2 UI : bloc de synchro dans Paramètres > Instructions.
- Phase 5 : cargo test --workspace OK, 
pm run build OK, 	auri build --bundles nsis OK (installateur 17,7 Mo, cloudflared.exe présent dans target/release), PROJET.md mis à jour.
- Non fait : commandes dédiées pour fichiers/multipart, skills (ZIP), OAuth MCP guidé, passkeys WebAuthn ; bannières de reprise/file/budget ; confirmations enrichies ; nettoyage CSS ; test d'installation réel ; scénarios manuels.
