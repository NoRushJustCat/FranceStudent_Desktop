# France Student Desktop

Application de bureau permettant de connecter les IA de **France Student** à l’ordinateur de l’utilisateur.

FS Code fournit une interface de chat capable d’interagir avec un projet local : lecture et recherche de fichiers, modifications contrôlées, exécution de commandes et connexion à des serveurs MCP.

> **Statut :** projet en développement — version `0.1.0`.

## Fonctionnalités

- Interface de bureau basée sur **Tauri** et **Svelte**.
- Connexion à l’API France Student.
- Sélection d’un dossier de travail local.
- Outils disponibles pour le mode Code :
  - `list_directory` — parcourir l’arborescence ;
  - `read_file` — lire un fichier ou une partie d’un fichier ;
  - `search_files` — rechercher du texte ou une expression régulière ;
  - `edit_file` — modifier une portion de fichier ;
  - `write_file` — créer ou réécrire un fichier ;
  - `run_command` — exécuter une commande dans le dossier de travail.
- Confirmation des actions sensibles :
  - les opérations de lecture sont autorisées automatiquement ;
  - les modifications de fichiers peuvent demander une confirmation ;
  - l’exécution de commandes demande toujours une confirmation.
- Protection du dossier de travail : les chemins sortant de la racine sélectionnée sont refusés.
- Mode économie avec sorties d’outils limitées, moins d’étapes et compression automatique du contexte.
- Gestion des agents, conversations, utilisation de l’API et contexte courant.
- Support des extensions et des serveurs **MCP**.
- Serveurs MCP locaux appelés via `run_command` (`mcp list`, `mcp tools`, `mcp call …`), sans tunnel, sur le principe de `fs-mcp-cli`.
- Stockage sécurisé du token API dans le coffre-fort du système.

## Architecture

Le projet est organisé comme un workspace Rust avec une interface frontend JavaScript/Svelte :

```text
fscode/
├── app/
│   ├── src/                 # Interface Svelte
│   ├── src-tauri/           # Application native Tauri et commandes Rust
│   ├── scripts/             # Scripts de préparation
│   ├── package.json
│   └── vite.config.js
├── crates/
│   ├── fs-agent/            # Boucle agent : événements, outils et résultats
│   ├── fs-api/              # Client HTTP et streaming de l’API
│   ├── fs-store/            # Réglages locaux et stockage du token API
│   └── fs-tools/            # Outils d’interaction avec le projet local
├── Cargo.toml
└── README.md
```

## Technologies utilisées

### Backend et application native

- Rust 2021
- Tauri 2
- Tokio
- Reqwest
- Serde / Serde JSON
- Axum
- Keyring
- EventSource streaming

### Frontend

- Svelte 5
- Vite 7
- JavaScript moderne
- Marked pour le rendu Markdown
- DOMPurify pour la sécurisation du contenu HTML

## Prérequis

Pour développer le projet, il faut installer :

- Rust et Cargo ;
- Node.js et npm ;
- les dépendances système nécessaires à Tauri 2 selon le système d’exploitation ;
- un accès à l’API France Student ;
- un token de connexion France Student service IA.

Les serveurs MCP sont lancés en local par l’application, selon la configuration de l’onglet MCP. Aucun tunnel ni binaire externe n’est nécessaire.

## Installation

Depuis la racine du dépôt :

```bash
cd app
npm install
```

Les dépendances Rust sont gérées automatiquement par Cargo.

## Lancer en mode développement

Depuis le dossier `app` :

```bash
npm run tauri dev
```

Cette commande :

1. démarre le serveur de développement Vite ;
2. compile et lance l’application Tauri ;
3. ouvre la fenêtre **FS Code**.

Pour lancer uniquement le frontend web :

```bash
npm run dev
```

## Construire l’application

Pour générer le frontend :

```bash
cd app
npm run build
```

Pour construire l’application de bureau et ses bundles :

```bash
npm run tauri build
```

Les fichiers générés sont placés dans les répertoires de sortie de Vite et Tauri.

## Configuration

Au premier démarrage, ouvrez les réglages de l’application et renseignez :

- l’URL de base de l’API ;
- le token de connexion France Student ;
- éventuellement l’agent à utiliser ;
- le dernier dossier de travail ;
- le mode économie ;
- l’acceptation automatique des modifications de fichiers.

L’URL par défaut de l’API est :

```text
https://api.francestudent.org
```

Les réglages non sensibles sont enregistrés dans un fichier `config.json` dans le dossier de configuration de l’application. Le jeton API est stockée dans le coffre-fort système. En développement, elle peut également être fournie avec la variable d’environnement :

```bash
FS_KEY="votre-cle-api"
```

## Sécurité

Les outils locaux sont exécutés uniquement à l’intérieur du dossier de travail sélectionné. Le projet refuse notamment :

- les chemins absolus situés en dehors du dossier de travail ;
- les chemins contenant une remontée non autorisée avec `..` ;
- les liens symboliques qui sortent de la racine ;
- les commandes dépassant le délai maximal autorisé.

Les opérations sont classées par niveau de risque :

| Outil | Niveau | Confirmation |
|---|---:|---|
| `list_directory` | Faible | Non |
| `read_file` | Faible | Non |
| `search_files` | Faible | Non |
| `edit_file` | Modification | Oui, sauf mode automatique |
| `write_file` | Modification | Oui, sauf mode automatique |
| `run_command` | Commande | Toujours |

Même lorsque l’acceptation automatique des modifications est activée, l’exécution de commandes reste soumise à confirmation.

Hors mode Code, `run_command` n’accepte que les commandes `mcp …`. `mcp list` et `mcp tools` sont libres ; `mcp call` demande confirmation.

## Limites actuelles

- Le crate `fs-agent` contient encore une implémentation minimale de la boucle agent.
- Le stockage local des conversations et du compteur de requêtes est prévu mais pas encore implémenté dans `fs-store`.
- Le projet est actuellement en version `0.1.0` et peut évoluer rapidement.

## Licence

Le projet est distribué sous licence **MIT**. Voir le fichier [`LICENSE`](LICENSE).

## Contribution

Les contributions sont les bienvenues. Avant de proposer une modification :

1. créez une branche dédiée ;
2. décrivez clairement le changement ;
3. testez l’application en développement ;
4. vérifiez que les changements respectent les règles de sécurité du dossier de travail ;
5. ouvrez une pull request avec un résumé et les éventuelles étapes de test.
