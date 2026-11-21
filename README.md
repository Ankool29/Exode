# EXODE

Jeu d'automatisation industrielle en vue du dessus, pensé d'abord pour Android.
Référence de conception : *Cahier de conception consolidé v0.4*.

Ce dépôt contient la **première version de développement** : le cœur de simulation, un
affichage provisoire en formes simples (en attendant les sprites du lot GRAPHICS-P0) et la
chaîne de compilation Android.

## Récupérer le jeu sur le téléphone

À chaque envoi sur `main`, GitHub Actions construit un APK de débogage et le publie dans la
pré-version **dev** du dépôt (onglet *Releases*). Il suffit de télécharger
`exode-debug.apk` depuis le téléphone et de l'installer.

Pour ouvrir le projet dans l'éditeur Godot Android : copier le dossier `game/`, puis décompresser
`exode-bin.zip` (même pré-version) dans `game/` pour obtenir `game/bin/`. Sans ces bibliothèques,
le jeu affiche « Bibliothèque native EXODE absente ».

## Ce qui fonctionne

| Domaine | État |
| --- | --- |
| Monde procédural par chunks de 32 × 32, déterministe, limite ±1 000 000 cases | Fait |
| Terrain plat : herbe, terre, sable, lacs, forêts, rochers ; défrichement, remblai | Fait |
| Gisements : richesse croissante avec la distance puis plateau, modes fini et infini | Fait |
| Convoyeurs à deux voies, objets individuels, virages, insertion latérale, saturation | Fait |
| Bras mécaniques, foreuses, fours (recette automatique), assembleur, recycleur | Fait |
| Coffres, bennes à déchets ; une machine dont la sortie de déchets est pleine s'arrête | Fait |
| Électricité simple : générateur provisoire, poteaux, réseaux, satisfaction partielle | Fait |
| Conservation de la matière : contrôle des recettes et bilan global en jeu | Fait |
| Déterminisme, empreinte d'état, indépendance vis-à-vis de la caméra | Fait, testé |
| Sauvegarde binaire versionnée, écriture atomique, sauvegarde auto et à la mise en arrière-plan | Fait |
| Interface tactile : déplacement, pincement, aperçu fantôme, confirmation, portrait et paysage | Fait |
| Ralentissement cohérent si le matériel sature | Fait |

## Pas encore fait (prochaines étapes)

- Construction avec matériaux, inventaire, robots et chantiers (v0.4 6.3).
- Corridors électriques à capacité, fluides, chaleur (bancs F3, F4).
- Trains, recherche, jalons.
- Représentation compacte des convoyeurs (banc F1) : 200 000 objets sur les tapis coûtent
  environ 13 ms par tick sur un serveur x86, au-dessus du budget visé.
- Une boucle de convoyeurs remplie à 100 %, sans aucun vide, reste immobile (limite connue).
- Sprites : lot P0 Pionnier V5 intégré (terrain, convoyeurs, machines, nature, objets) ;
  bras et icônes encore provisoires. Défauts connus : [docs/A_CORRIGER.md](docs/A_CORRIGER.md).

## Organisation

```
crates/exode-core    noyau de simulation (Rust, sans dépendance graphique)
crates/exode-core/data/content.json   contenu PROVISOIRE : objets, recettes, bâtiments
crates/exode-godot   pont GDExtension (classe ExodeSim)
crates/exode-bench   bancs de mesure en ligne de commande
game/                projet Godot 4.4 (scènes, scripts, préréglage d'export Android)
.github/workflows    compilation, tests, autotest Godot, APK
```

## Développer

```
cargo test -p exode-core                  # tests du noyau
cargo run --release -p exode-bench -- all # mesures indicatives
cargo build --release -p exode-godot      # bibliothèque pour Godot (Linux)
cp target/release/libexode_godot.so game/bin/libexode_godot.linux.x86_64.so
godot --headless --path game -- --selftest
```

Les règles pour les agents de codage sont dans [AGENTS.md](AGENTS.md).
