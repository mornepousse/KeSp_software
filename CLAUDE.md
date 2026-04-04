# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Projet

KeSp Controller — outil de configuration pour le clavier mécanique split KaSe/KeSp. Communique avec le firmware ESP32 via USB CDC série (VID `0xCAFE`, PID `0x4001`, 115200 baud).

Port depuis egui/eframe vers **Slint** (Rust). La logique métier (serial, protocol, parsers, keycode, layout) est réutilisée telle quelle dans `src/logic/`. Seule la couche UI est réécrite en Slint.

## Architecture

```
src/
├── main.rs              # Entry point, crée MainWindow, câble les bridges Rust↔Slint
├── logic/               # Logique métier (portée de l'original egui, ne pas modifier sauf bug)
│   ├── serial/          # Communication série USB CDC + protocol v2 binaire
│   ├── binary_protocol  # Frames KS/KR avec CRC-8/MAXIM
│   ├── keycode          # Décodage HID 0x0000-0x6FFF (MT, LT, TD, macros, etc.)
│   ├── layout           # Parser JSON du layout physique (Group/Line/Keycap)
│   ├── layout_remap     # Remapping 13 layouts clavier (AZERTY, QWERTZ, Dvorak, etc.)
│   ├── parsers          # Parsing des réponses firmware (tap dance, combo, leader, etc.)
│   ├── settings         # Persistance JSON des préférences
│   ├── stats_analyzer   # Analyse heatmap, bigrams, finger load
│   └── flasher          # Flasher ESP32 via SLIP protocol
└── bridge/              # (à créer) Modules de pont Rust↔Slint par feature

ui/
├── main.slint           # Fenêtre principale, TabWidget, modals
├── theme.slint          # Couleurs Dracula
├── globals.slint        # Structs partagées, globals (AppState, KeymapBridge, etc.)
├── components/          # Composants réutilisables
│   ├── connection_bar   # Barre de connexion (LED, port, bouton)
│   ├── status_bar       # Barre de statut (message, WPM, version)
│   ├── keyboard_view    # Rendu 2D du clavier split (for loop + positions absolues)
│   └── key_button       # Touche individuelle avec sélection
└── tabs/                # Un fichier par onglet
    ├── tab_keymap       # Éditeur de keymap + rendu clavier
    ├── tab_advanced     # Tap dance, combos, leader, key override, BT
    ├── tab_macros       # Éditeur de macros
    ├── tab_stats        # Heatmap + bigrams
    └── tab_settings     # Layout picker, backup/restore, OTA, flasher
```

## Build

```bash
# NixOS
nix-shell --run "cargo build --release"

# Ou directement si les dépendances sont dans le système
cargo build --release
```

## Conventions

- L'UI est dans les fichiers `.slint`, la logique dans le Rust — ne pas mélanger.
- Les données passent via les `global` Slint (définis dans `globals.slint`), câblés dans `main.rs`.
- Communication thread série → UI via `mpsc::channel` + `slint::Timer` polling à 50ms.
- `slint::invoke_from_event_loop()` pour les updates depuis des threads background.
- Les `VecModel<T>` sont la façon standard de passer des listes dynamiques (keycaps, layers, etc.).
- Pas de `states` hover sur les 66 touches du clavier (cause 5%+ CPU idle). Utiliser uniquement le click.
- `border-width` doit être fixe (pas de ternaire) sur les éléments répétés en `for` loop — sinon Slint recalcule le layout en boucle.

## Protocol série

Le firmware supporte deux protocols :
- **v2 binaire** (auto-détecté via PING) : frames `KS`/`KR` avec CRC-8/MAXIM, ~40 commandes
- **Legacy ASCII** : commandes texte + réponses ligne par ligne, terminées par `OK`/`ERROR`

L'auto-connect cherche le port USB par VID:PID (`0xCAFE:0x4001`) ou par nom produit "KaSe"/"KeSp".

## État actuel

- [x] Squelette complet (5 onglets, connection bar, status bar)
- [x] Rendu 2D du clavier split (66 touches positionnées)
- [x] Auto-connect série + lecture keymap layer 0
- [x] Affichage des keycodes décodés sur les touches
- [x] Switch de layer avec rechargement du keymap
- [ ] Key selector modal (popup de choix de touche)
- [ ] Tab Advanced (tap dance, combos, leader, key override, BT)
- [ ] Tab Macros
- [ ] Tab Stats (heatmap overlay)
- [ ] Tab Settings (layout picker, backup/restore, OTA, flasher)
