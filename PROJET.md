# France Student Desktop — FS Code

Client de bureau **non officiel** pour France Student (`ia.francestudent.org`), construit avec **Tauri 2**, **Rust** et **Svelte 5**.

Il combine :

- une interface de **chat** (modèles, streaming, historique, Markdown) ;
- un **mode Code** : l'IA travaille sur un dossier local de ta machine (lire, chercher, éditer, exécuter des commandes) avec des autorisations que tu contrôles ;
- des **serveurs MCP** locaux, utilisables par l'IA ;
- des **skills** (instructions réutilisables) ;
- un **terminal intégré** et un **panneau de différences**.

> **Version `0.1.0` — en développement.** Un compte France Student et une connexion Internet sont requis pour l'IA. Ce dépôt ne contient pas les serveurs de France Student.

---

## Sommaire

1. [Comment ça marche](#1-comment-ça-marche)
2. [Fonctionnalités](#2-fonctionnalités)
3. [Installation et lancement](#3-installation-et-lancement)
4. [Connexion et stockage local](#4-connexion-et-stockage-local)
5. [Mode Code et outils locaux](#5-mode-code-et-outils-locaux)
6. [Modes d'autorisation](#6-modes-dautorisation)
7. [MCP](#7-mcp)
8. [Skills](#8-skills)
9. [Architecture du dépôt](#9-architecture-du-dépôt)
10. [Construire un exécutable](#10-construire-un-exécutable)
11. [Vérifications](#11-vérifications)
12. [Dépannage](#12-dépannage)
13. [Contribution, licence](#13-contribution-licence)

---

## 1. Comment ça marche

```text
┌───────────────────────── App Tauri (fs-code) ─────────────────────────┐
│  Interface Svelte  ◄── commandes / événements Tauri ──►  Backend Rust │
└───────────────────────────────────────────────────────┬───────────────┘
       │ HTTPS + streaming
       ▼
      API France Student (api.francestudent.org)
```

1. **Interface** (`app/src`) : Svelte 5 + Vite. Elle n'appelle jamais l'API directement : elle passe par des **commandes Tauri** (`invoke`) et reçoit les réponses en **événements** (canal `Channel<UiEvent>`).
2. **Backend** (`app/src-tauri`) : Rust. Il gère la connexion, appelle l'API (crate `fs-api`), exécute les outils locaux (crate `fs-tools`), applique les autorisations (`perm.rs`), pilote le terminal, MCP et les skills.
3. **API France Student** : fournit les modèles et le chat en streaming. Documentation : [`API_FranceStudent.md`](API_FranceStudent.md).

### Boucle d'un message en mode Code

Le mode Code s'appuie sur les **outils côté client** de l'API ([`client-tools.md`](client-tools.md)) : le serveur déclare les outils au modèle, mais **n'exécute rien** ; c'est l'app qui exécute sur ta machine.

1. L'app envoie le message avec `clientTools` (liste des outils qu'elle sait exécuter).
2. Le modèle répond en streaming ; s'il veut agir, le tour se termine avec des `toolCalls`.
3. Pour chaque appel, l'app applique le **mode d'autorisation** (exécution directe, confirmation ou refus), exécute l'outil dans le dossier de travail et affiche le résultat.
4. L'app renvoie les `toolResults` ; le modèle continue, jusqu'à la réponse finale ou la limite de pas (plus basse en mode économie).

Chaque écriture, commande ou appel MCP est consignée dans `changes.jsonl` (journal local).

> Les outils côté client dépendent d'un flag côté serveur et ne sont pas supportés par tous les modèles (voir `client-tools.md`).

---

## 2. Fonctionnalités

- **Chat** : choix du modèle, streaming, Markdown, coloration syntaxique, pièces jointes, recherche web optionnelle.
- **Conversations** : historique, recherche, renommage, épinglage, archivage, messages sauvegardés, édition d'un message avec fork.
- **Compte** : connexion e-mail / mot de passe, ou jeton saisi à la main.
- **Suivi d'utilisation** : budget, consommation, anneau de contexte.
- **Mode Code** : dossier de travail, six outils locaux, journal des modifications.
- **Panneau de différences** et **terminal intégré** (PowerShell sous Windows, `sh` ailleurs).
- **Autorisations** : cinq modes, sélectionnables depuis la barre de saisie.
- **MCP** : serveurs locaux (`stdio` ou HTTP), pont vers le serveur France Student.
- **Skills** : locaux et du compte, insertion dans les messages.
- **Personnalisation** : thèmes, notifications, mode économie, mode développeur (console de logs).

La disponibilité des modèles et des fonctions dépend du compte, de l'API et du support des outils par le modèle choisi.

---

## 3. Installation et lancement

### Prérequis

- Node.js et npm ;
- Rust stable et Cargo ;
- les prérequis de compilation de Tauri 2 (sous Windows : outils C++ de Visual Studio et WebView2).

Le projet contient du code spécifique à Windows : un build Linux ou macOS n'est pas garanti sans adaptation.

### Installer

```powershell
cd app
npm ci
```

Cargo récupère les dépendances Rust à la première compilation.

### Lancer en développement

Depuis `app/` :

```powershell
npm run tauri -- dev
```

Tauri démarre Vite (`http://localhost:1420`), compile le backend et ouvre la fenêtre.

Interface seule, dans un navigateur : `npm run dev`. Les fonctions qui utilisent les commandes natives (connexion, outils, terminal, MCP) n'y fonctionnent pas.

---

## 4. Connexion et stockage local

1. Ouvrir l'app, saisir e-mail et mot de passe France Student, cliquer sur **Se connecter**.
2. L'app ouvre une session web (Auth.js + WHMCS), ne garde que le cookie `next-auth.session-token`, puis en tire un **jeton d'accès court**, renouvelé automatiquement.

Un captcha, une double authentification ou une vérification anti-robot peut bloquer la connexion automatique : la saisie manuelle d'un jeton reste possible.

| Donnée | Emplacement |
|---|---|
| Réglages | `config.json` (dossier de config de l'app) |
| Session de connexion | `session.json` (même dossier) |
| Jeton saisi à la main | Coffre-fort du système (`keyring`) |
| Serveurs MCP | `mcp.json` |
| Réglages du pont MCP | `bridge.json` |
| Journal des actions de l'IA | `changes.jsonl` |
| Skills de l'app | dossier `skills/` de la config |
| Mot de passe | **jamais enregistré** |

**`session.json` contient un secret en clair** : ne le partage pas, ne le commite pas. Même précaution pour les jetons, fichiers `.env` et configurations MCP contenant des secrets.

Adresses par défaut : API `https://api.francestudent.org` (configurable dans les réglages), connexion web `https://ia.francestudent.org`.

---

## 5. Mode Code et outils locaux

1. Passer de **Chat** à **Code** dans la barre de saisie.
2. Choisir un dossier de travail.
3. Choisir un [mode d'autorisation](#6-modes-dautorisation).
4. Décrire la tâche, puis examiner les actions et les différences.

| Outil | Fonction |
|---|---|
| `list_directory` | Parcourir un dossier (profondeur 1 à 3) |
| `read_file` | Lire un fichier, éventuellement par plage de lignes |
| `search_files` | Chercher du texte ou une regex (filtre `glob`) |
| `edit_file` | Remplacer un texte exact dans un fichier |
| `write_file` | Créer ou réécrire un fichier |
| `run_command` | Exécuter une commande (`cwd`, `timeout_seconds`) |

Les chemins sont **confinés au dossier de travail** : `..`, chemin absolu ailleurs et lien symbolique sortant sont refusés, sauf en mode **Système**.

---

## 6. Modes d'autorisation

| Mode | Comportement |
|---|---|
| **Demander** | Confirmation avant chaque édition ou commande |
| **Planification** | Lecture seule ; éditions et commandes refusées, l'IA présente un plan |
| **Auto** | Règles heuristiques (pas un jugement du modèle) : éditions du projet et commandes connues sûres sans confirmation, le reste est demandé |
| **Projet** | Tout dans le projet sans confirmation ; confirmation pour les commandes dangereuses ou hors projet |
| **Système** | Tout sans confirmation, y compris hors projet |

**Système** demande une confirmation à l'activation et n'est pas restauré au redémarrage.

> Les commandes et outils MCP s'exécutent avec les droits de ton compte : le dossier de travail n'est **pas** un bac à sable. Vérifie les commandes, utilise Git et garde des sauvegardes.

---

## 7. MCP

Les serveurs se déclarent dans `mcp.json`, au même format que Claude Desktop (`{"mcpServers": {...}}`). Un serveur est :

- **local** : `command` + `args` (transport `stdio`) ;
- **HTTP** : `url`, `token` et `headers` optionnels (HTTP « Streamable »).

Rien ne démarre tout seul : un serveur n'est lancé ou contacté que sur demande explicite. Chaque serveur a ses propres dépendances (Node.js, Python…), à installer séparément. Ils se gèrent dans l'onglet **MCP / Extensions** de l'app.

Deux façons de les utiliser avec l'IA :

### a) Commandes locales

L'IA exécute, via `run_command`, des commandes traitées localement par l'app (aucun tunnel) :

```text
mcp list
mcp tools <serveur>
mcp call <serveur> <outil> <json>
```

Pour que le modèle sache qu'elles existent, l'app joint au premier message d'un chat un bloc `[Contexte client]` listant les serveurs et leurs outils (masqué à l'affichage).

### b) Pont officiel (`bridge.rs`)

L'app expose les serveurs MCP locaux au **serveur France Student** (fonction `/mcp/servers`) :

```text
modèle → serveur FS → https://xxxx.trycloudflare.com/mcp → cloudflared → 127.0.0.1:PORT/mcp (l'app) → serveur MCP local
```

- Petit serveur HTTP local, joignable seulement via le tunnel `cloudflared`.
- Jeton aléatoire obligatoire, régénéré à chaque lancement.
- **Confirmation native** avant chaque outil non « lecture seule », sauf `auto_allow` ou outil marqué « toujours autoriser ».
- Réglages dans `bridge.json` : `auto_start`, `auto_allow`, `label`, `register_extra` (fusionné dans le corps de `POST /mcp/servers`).
- `cloudflared` : cherché d'abord dans les binaires embarqués (`app/src-tauri/binaries/`), puis dans le `PATH` et les emplacements usuels. Pour le télécharger : `node scripts/fetch-cloudflared.mjs` depuis `app/`, ou `winget install --id Cloudflare.cloudflared`.

### c) Instructions synchronisées (`instructions.rs`)

Tant que le pont est actif, l'app ajoute un bloc balisé (`<<<FSCODE:AUTO:v1>>>` … `<<<FSCODE:END:v1>>>`) aux instructions personnalisées du compte, pour informer l'IA des serveurs MCP disponibles.

- Contenu : noms des serveurs et nombre d'outils. Jamais de jeton, d'URL de tunnel ni de chemin.
- Limites : 8 000 caractères pour le bloc, 10 000 pour les instructions (limite de l'API).
- Le texte de l'utilisateur est conservé tel quel ; une modification faite ailleurs entre-temps n'est pas écrasée.
- Retrait à la déconnexion et à la fermeture de l'app (attente 8 s au plus). En cas d'échec, `instructions-state.json` garde l'état ; le retrait est retenté au démarrage (bloc orphelin retiré avant réapplication).
- Réglage `sync_instructions` (défaut : activé), onglet *Paramètres > Instructions* (interrupteur, aperçu en lecture seule, « Retirer maintenant »).
- Limites connues : l'état n'est pas séparé par compte ; pas de détection d'une modification entre la lecture et l'écriture.

### d) Explorateur d'API (`api_explorer.rs`, `fs-api/src/routes.rs`)

Onglet *Paramètres > API* : catalogue de toutes les routes de `API_FranceStudent.md`. Le front envoie un identifiant de route, jamais un chemin libre ; les segments sont encodés, les paramètres de requête et en-têtes non documentés sont refusés. Les routes destructives exigent une confirmation. Les routes multipart, SSE et binaires ne sont pas appelables depuis l'explorateur. Un test compare le catalogue à la documentation.

### Outil annexe : `app/fs-mcp-cli`

CLI Node.js indépendante pour piloter des serveurs MCP avec les modèles France Student (détection des serveurs de Claude Desktop, Cursor, VS Code…, pool de connexions, mode `-p`). Elle a son propre [README](app/fs-mcp-cli/README.md) et ses tests (`npm test`).

---

## 8. Skills

Un skill est un dossier contenant un `SKILL.md`. Dossiers lus, par priorité (le premier nom trouvé gagne) :

1. `<projet>/.fscode/skills`
2. `<projet>/.claude/skills`
3. `~/.claude/skills`
4. `<config de l'app>/skills`

On les crée, modifie et supprime depuis l'interface, puis on les insère dans un message ou on les référence avec `$nom-du-skill`. Les skills du compte sont aussi gérés depuis l'app (certains endpoints d'édition sont supposés, voir `ext.rs`).

---

## 9. Architecture du dépôt

```text
fscode/
├── app/
│   ├── src/   Interface Svelte : App.svelte, theme.css, lib/ (composants, api.js)
│   ├── src-tauri/
│   │   ├── src/
│   │   │   ├── main.rs      Commandes Tauri, boucle d'un message, état global
│   │   │   ├── login.rs     Connexion e-mail + mot de passe, renouvellement du jeton
│   │   │   ├── perm.rs      Modes d'autorisation
│   │   │   ├── mcp.rs       Client MCP local (stdio / HTTP)
│   │   │   ├── mcp_ctx.rs   Commandes `mcp …` et bloc de contexte MCP
│   │   │   ├── bridge.rs    Pont MCP vers le serveur FS (HTTP local + cloudflared)
│   │   │   ├── instructions.rs  Bloc auto dans les instructions du compte
│   │   │   ├── api_explorer.rs  Commandes `api_routes` / `api_call`
│   │   │   ├── ext.rs       Édition des skills et de mcp.json depuis l'UI
│   │   │   ├── skills.rs    Skills locaux
│   │   │   ├── term.rs      Terminal intégré
│   │   │   └── changelog.rs Journal des actions de l'IA
│   │   ├── binaries/ cloudflared (exécutable annexe)
│   │   └── tauri.conf.json  Config Tauri (identifier, fenêtre, bundle)
│   ├── fs-mcp-cli/   CLI Node.js pour MCP (outil annexe)
│   ├── scripts/      fetch-cloudflared.mjs
│   └── package.json
├── crates/
│   ├── fs-api/       Client HTTP, streaming, catalogue des routes (`routes.rs`)
│   ├── fs-tools/     Outils locaux, confinement au dossier de travail
│   ├── fs-store/     Réglages et coffre-fort de jetons
│   └── fs-agent/     Boucle d'agent (outils, politique, reprise, budget)
├── project-frstcli-1.0.0/   CLI séparée, ignorée par Git, non requise ici
├── API_FranceStudent.md     Référence de l'API
├── client-tools.md          Protocole des outils côté client
├── v4/        Logos et icônes sources
├── Cargo.toml        Workspace Rust (crates/* + app/src-tauri)
└── LICENSE
```

`.scratch/` (sauvegardes de travail) et `v4/` ne font pas partie de l'application exécutable.

---

## 10. Construire un exécutable

Depuis `app/` :

```powershell
npm run tauri -- build --bundles nsis
```

Le build lance `npm run build`, compile le backend et embarque l'interface. Sorties attendues (le dossier `target/` est à la racine) :

```text
target/release/fs-code.exe
target/release/bundle/nsis/*-setup.exe
```

Distribue de préférence l'installateur. **Ne remplace pas `tauri.conf.json` par un fragment** : il doit garder `identifier`, la fenêtre et la section `bundle`.

---

## 11. Vérifications

Depuis la racine :

```powershell
cargo check -p fs-code
cargo test --workspace
```

Interface, depuis `app/` :

```powershell
npm run build
```

Ces commandes ne remplacent pas un essai réel dans l'app : connexion, envoi de messages, renouvellement de session, autorisations, outils locaux et MCP. Un build réussi ne valide pas les échanges avec l'API réelle.

---

## 12. Dépannage

- **JSON de configuration invalide** : `tauri.conf.json` doit être un objet complet, sans virgule finale.
- **`identifier` manquant** : restaurer la configuration complète.
- **HTTP 401** : se reconnecter ; une session révoquée ne se rétablit pas par un simple renouvellement de jeton.
- **HTTP 403 (Cloudflare)** : blocage côté réseau ou service ; relever le message et le Ray ID.
- **Outils côté client refusés (`403` / `400`)** : flag serveur absent ou modèle non compatible (voir `client-tools.md`).
- **Interface vide après connexion** : vérifier l'accès réseau à l'API ; ne partage aucun secret dans les journaux.
- **Outil MCP indisponible** : vérifier la commande du serveur, ses dépendances et `mcp.json`.
- **`cloudflared` introuvable** : voir [Pont officiel](#b-pont-officiel-bridgers).
- **Commande refusée** : vérifier le mode d'autorisation et le dossier de travail.

---

## 13. Contribution, licence

Changements ciblés, tests pour les comportements modifiés, et description des vérifications réellement faites. Pour les outils, l'authentification ou les autorisations, documente aussi les risques et limites.

Licence **Apache 2.0** : voir [LICENSE](LICENSE). Ce client est **non officiel** : les noms, logos et services France Student restent soumis aux droits de leurs titulaires.
