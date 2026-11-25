---
name: computer
description: Piloter l'écran (multi-écrans), les fenêtres, la souris, le clavier et les captures d'écran façon Computer Use via fs_computer, avec la Boucle Vision Live via fs_delegate.
---

Appelle immédiatement l'outil MCP `fs_computer` de France Student (`fs-atg`). Il agit sur le **vrai bureau Windows** de l'utilisateur, jamais sur les fichiers du workspace.

1. **Inspection ou capture d'écran** :
   - `fs_computer({ action: "screen_info" })` : liste des écrans (`deviceName`, `primary`, position, taille), curseur et fenêtre active. Commence toujours par là sur une machine multi-écrans.
   - `fs_computer({ action: "screenshot", screenIndex: 1 })` : capture un écran précis en PNG. `screenIndex` part de 0, dans l'ordre de `screen_info`. Sans `screenIndex`, c'est l'écran choisi dans le panneau (`selectedScreenIndex`, 0 par défaut).
   - `fs_computer({ action: "screenshot", deviceName: "\\\\.\\DISPLAY2" })` : même chose par nom Windows (prioritaire sur `screenIndex`).
   - `region: { x, y, width, height }` : sous-zone, en coordonnées absolues du bureau virtuel.
   - La réponse contient un bloc texte (`path`, `width`, `height`, `screenIndex`, `deviceName`, `screensCount`, `screenBounds`) et l'**image PNG** elle-même.
   - `fs_computer({ action: "windows" })` liste les fenêtres, puis `fs_computer({ action: "focus_window", title: "..." })` en active une.
2. **Actions souris et clavier** (bloquées en mode `plan`, Computer Use doit être activé) :
   - Souris : `click`, `double_click`, `right_click`, `mouse_move`, `drag`, `scroll` avec `x`, `y` en **pixels absolus**. Sur un 2e écran, ajoute `screenBounds.x` / `screenBounds.y` de la capture.
   - Clavier : `type` (`text`), `key` ou `hotkey` (`key: "Enter"`, `"Ctrl+C"`, `"Alt+Tab"`). Les raccourcis système dangereux sont refusés.
   - Presse-papiers : `clipboard_get`, `clipboard_set`.

## Boucle Vision Live (France Student voit l'écran)

Pour une tâche visuelle (« clique sur le bouton Exporter », « remplis ce formulaire ») :

1. `fs_delegate({ task: "...", attachScreenshot: true, liveComputerMode: true, screenIndex: 0, waitSeconds: 120 })`
   - La capture est envoyée à France Student (`/filesv2`) et jointe au message.
   - `imagePaths: ["C:/chemin/image.png"]` permet de joindre d'autres images (6 max).
2. France Student répond par **une seule** étape : une action `fs_computer` en JSON, une nouvelle capture à prendre, ou `TERMINÉ`.
3. Exécute l'action avec `fs_computer`, puis relance l'étape 1 avec une capture fraîche.
4. Arrête-toi quand la réponse dit `TERMINÉ`, après ~10 tours, ou dès qu'une action est sensible (paiement, suppression, envoi de message, mot de passe) : demande alors confirmation à l'utilisateur.
