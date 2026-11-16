# France Student Desktop — FS Code

Client de bureau non officiel pour France Student, construit avec **Tauri 2**, **Rust** et **Svelte 5**.

L’application réunit une interface de conversation et un mode Code permettant de travailler sur un projet local : explorer des fichiers, proposer des modifications, exécuter des commandes et utiliser des serveurs MCP, selon les autorisations choisies.

> **Version du dépôt : `0.1.0` — en développement.** Une connexion Internet et un compte France Student disposant des accès nécessaires sont requis pour les fonctionnalités d’IA. Ce dépôt ne contient pas les serveurs de France Student.

## Fonctionnalités

- **Chat** : choix du modèle, réponses en streaming, rendu Markdown et coloration syntaxique.
- **Conversations** : historique, recherche, renommage, épinglage, archivage et messages sauvegardés.
- **Compte** : connexion par e-mail et mot de passe, ou saisie manuelle d’un jeton de connexion.
- **Suivi d’utilisation** : budget, consommation et informations de contexte renvoyés par l’API.
- **Mode Code** : sélection d’un dossier, lecture et recherche de fichiers, modifications et exécution de commandes.
- **Suivi des modifications** : panneau de différences et terminal intégré.
- **Autorisations** : sélection du mode depuis la barre de saisie.
- **MCP** : configuration et utilisation de serveurs locaux, avec gestion des autorisations.
- **Skills** : instructions réutilisables et insertion dans les conversations.
- **Tâches parallèles** : exécution par vagues, contrat commun et suivi dédié.
- **Personnalisation** : thèmes, notifications et mode économie.

La disponibilité des modèles et de certaines fonctions dépend du compte, de l’API et du support des outils par le modèle sélectionné.

## Démarrage rapide

### Prérequis

Pour compiler et développer l’application :

- Node.js compatible avec les dépendances du projet, et npm ;
- Rust stable et Cargo ;
- les prérequis de compilation de Tauri 2 pour votre système.

Sous Windows, prévoyez notamment les outils de compilation C++ de Visual Studio et WebView2. Le projet contient du code spécifique à Windows : ne présumez pas qu’un build Linux ou macOS fonctionne sans adaptation ni test.

### Installer les dépendances

Depuis la racine du dépôt :

```powershell
cd app
npm ci
```

Cargo récupère les dépendances Rust lors de la compilation.

### Lancer l’application en développement

Depuis `app/` :

```powershell
npm run tauri -- dev
```

Tauri démarre Vite, compile le backend Rust et ouvre l’application native.

Pour lancer uniquement l’interface :

```powershell
npm run dev
```

Ce dernier mode sert au développement visuel : les fonctionnalités utilisant les commandes natives Tauri ne fonctionnent pas dans un navigateur ordinaire.

## Connexion

1. Ouvrir l’application.
2. Renseigner l’e-mail et le mot de passe France Student.
3. Cliquer sur **Se connecter**.

L’application utilise le flux de connexion web pour obtenir une session, puis un jeton d’accès à l’API. Le mot de passe n’est pas enregistré. Le jeton d’accès est renouvelé à partir de la session enregistrée.

Une session expirée ou révoquée nécessite une nouvelle connexion. Un captcha, une double authentification ou une vérification anti-robot peut empêcher la connexion automatique ; la saisie manuelle d’un jeton reste disponible.

### Stockage local

| Donnée | Emplacement / traitement |
|---|---|
| Réglages | `config.json`, dans le dossier de configuration de l’application |
| Session issue de la connexion | `session.json`, dans le même dossier |
| Jeton saisi manuellement | Coffre-fort du système via `keyring` |
| Mot de passe | Utilisé pour la connexion, non enregistré |

**Attention : `session.json` contient un secret de connexion en clair.** Ne le partagez pas et ne l’ajoutez pas au dépôt. Ne publiez pas non plus vos cookies, jetons, fichiers `.env` ou configurations MCP contenant des secrets.

L’adresse de l’API est configurable dans les réglages. La valeur par défaut du stockage est `https://api.francestudent.org` ; le flux de connexion web utilise `https://ia.francestudent.org`.

## Utiliser le mode Code

1. Passer de **Chat** à **Code** dans la barre de saisie.
2. Sélectionner un dossier de travail.
3. Choisir le mode d’autorisation approprié.
4. Décrire la tâche et examiner les actions proposées ainsi que les différences produites.

### Outils locaux

| Outil | Fonction |
|---|---|
| `list_directory` | Parcourir un dossier |
| `read_file` | Lire un fichier, éventuellement par plage de lignes |
| `search_files` | Rechercher du texte ou une expression régulière |
| `edit_file` | Remplacer une portion précise d’un fichier |
| `write_file` | Créer ou réécrire un fichier |
| `run_command` | Exécuter une commande locale |

### Modes d’autorisation

| Mode | Comportement annoncé par l’interface |
|---|---|
| **Demander** | Confirmation avant une édition ou une commande |
| **Planification** | Exploration en lecture seule ; éditions et commandes refusées |
| **Auto** | Lectures et commandes reconnues comme sûres autorisées ; confirmation pour le reste |
| **Projet** | Actions dans le projet sans confirmation ; confirmation pour les commandes dangereuses ou hors projet |
| **Système** | Accès étendu, y compris hors projet, sans confirmation |

Le mode **Système** demande une confirmation à l’activation et n’est pas restauré au redémarrage.

> Les commandes et outils MCP s’exécutent avec les droits du processus local : le dossier de travail n’est pas un bac à sable système. Vérifiez les commandes, les modifications et les serveurs configurés. Utilisez Git et conservez des sauvegardes.

### Tâches parallèles

Exemple de forme acceptée :

```text
tasks parallel {"contract":"Conventions communes et fichiers attribués","tasks":[{"task":"Analyser la structure sans modifier les fichiers","role":"explore","name":"analyse","wave":1},{"task":"Réaliser une modification précisément décrite","role":"worker","name":"implementation","wave":2}]}
```

Limites définies dans `agents.rs` : **8 tâches par vague**, **12 par lancement**, **4 vagues au maximum**. Chaque tâche doit être autonome ; attribuez des fichiers distincts aux tâches qui écrivent.

## MCP et skills

Les serveurs MCP se configurent dans les extensions de l’application. Les dépendances propres à chaque serveur doivent être installées séparément : un serveur peut nécessiter Node.js, Python ou un autre exécutable.

Les commandes locales MCP reconnues comprennent :

```text
mcp list
mcp tools <serveur>
mcp call <serveur> <outil> <json>
```

Les skills sont des instructions réutilisables, notamment via des fichiers `SKILL.md`. Ils peuvent être consultés et insérés depuis l’interface, et référencés avec `$nom-du-skill`.

## Construire un exécutable Windows

Depuis `app/` :

```powershell
npm run tauri -- build --bundles nsis
```

Le build Tauri lance automatiquement `npm run build`, compile le backend Rust et embarque l’interface. Il n’y a pas de serveur Rust séparé à démarrer pour l’usage normal de l’application.

Avec la configuration actuelle du workspace, les sorties attendues sont :

```text
target/release/fs-code.exe
target/release/bundle/nsis/*-setup.exe
```

Le dossier `target/` est à la racine du dépôt. Ces chemins peuvent changer si une cible ou un répertoire Cargo personnalisé est utilisé. Privilégiez l’installateur pour distribuer l’application.

**Ne remplacez pas `tauri.conf.json` par un simple fragment de configuration.** Il doit conserver notamment `identifier`, les paramètres de fenêtre et la section de bundle.

## Vérifications de développement

Depuis la racine :

```powershell
cargo check
cargo test --workspace
```

Tests ciblés de l’application :

```powershell
cargo test -p fs-code login
cargo test -p fs-code agents
```

Compilation de l’interface, depuis `app/` :

```powershell
npm run build
```

Ces commandes ne remplacent pas les essais dans l’application : connexion, chargement des données, envoi de messages, renouvellement de session, autorisations et outils locaux. Un build réussi ne valide pas à lui seul les échanges avec l’API réelle.

## Organisation du dépôt

```text
fscode/
├── app/
│   ├── src/                 Interface Svelte et styles
│   ├── src-tauri/           Backend natif, connexion, autorisations et MCP
│   ├── public/              Ressources statiques
│   ├── scripts/             Scripts de préparation
│   └── package.json         Commandes npm
├── crates/
│   ├── fs-agent/            Composants de boucle de traitement
│   ├── fs-api/              Client API HTTP et streaming
│   ├── fs-store/            Réglages et stockage des jetons
│   └── fs-tools/            Outils locaux
├── v4/                      Logos et icônes sources
├── Cargo.toml               Workspace Rust
├── LICENSE
└── README.md
```

Le dossier `v4/` contient des ressources graphiques, pas une autre version exécutable de l’application. Les éventuels dossiers de référence locaux ne sont pas requis pour suivre les commandes de build ci-dessus.

## Dépannage

- **Erreur de configuration JSON** : vérifier que `app/src-tauri/tauri.conf.json` est un objet JSON complet, sans virgule finale ni texte supplémentaire.
- **Champ `identifier` manquant** : restaurer la configuration complète, plutôt qu’ajouter uniquement le bloc `build`.
- **HTTP 401** : vérifier la session et se reconnecter si nécessaire. Une session révoquée ne peut pas être rétablie par le seul renouvellement d’un jeton.
- **Interface vide après connexion** : relever le message d’erreur et vérifier l’accès réseau à l’API. Ne transmettre aucun secret avec les journaux.
- **Outil MCP indisponible** : vérifier la commande du serveur, ses dépendances et sa configuration locale.
- **Commande refusée** : vérifier le mode d’autorisation et le dossier de travail sélectionné.

## Contribution

Gardez les changements ciblés, ajoutez des tests pour les comportements modifiés et décrivez les vérifications réellement effectuées. Pour les changements liés aux outils, à l’authentification ou aux autorisations, documentez également les risques et les limites.

## Licence et affiliation

Le dépôt contient une licence **Apache License 2.0** : voir [LICENSE](LICENSE).

Ce client se présente comme **non officiel**. Les noms, logos et services France Student restent soumis aux droits et conditions de leurs titulaires ; la licence du code n’accorde pas à elle seule des droits sur ces éléments.
