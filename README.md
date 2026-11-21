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
| Lignes de transport (banc F1) : coût proportionnel à l'activité, voies bloquées en veille | Fait, testé |
| Bras mécaniques, foreuses, fours (recette automatique), assembleur, recycleur | Fait |
| Coffres, bennes à déchets ; une machine dont la sortie de déchets est pleine s'arrête | Fait |
| Socle commun des réseaux : composantes par union à la pose, reconstruction locale au retrait, identifiant canonique, version par réseau | Fait, testé |
| Électricité à corridors : poteau, pylône, sous-station, liaisons à capacité, flot entier, corridor limitant affiché (câble rouge, étincelles, charge en %) | Fait, testé |
| Conservation de la matière : contrôle des recettes et bilan global en jeu | Fait |
| Déterminisme, empreinte d'état, indépendance vis-à-vis de la caméra | Fait, testé |
| Sauvegarde binaire versionnée (v3, lecture et conversion des v1 et v2), écriture atomique, sauvegarde auto et à la mise en arrière-plan | Fait |
| Interface tactile : déplacement, pincement, aperçu fantôme, confirmation, portrait et paysage | Fait |
| Ralentissement cohérent si le matériel sature | Fait |

## Pas encore fait (prochaines étapes)

- Construction avec matériaux, inventaire, robots et chantiers (v0.4 6.3).
- Fluides et chaleur (étape 3, pull requests suivantes ; banc F4).
- Trains, recherche, jalons.
- Convoyeurs (banc F1) : les cases reliées forment des lignes de transport stockées par
  écarts, et une voie bloquée est mise en veille. Mesures indicatives sur un serveur x86
  (ce ne sont pas des mesures sur téléphone) : 200 000 objets sur des lignes saturées en
  mouvement coûtent environ 0,01–0,02 ms par tick (12,8 ms avant), les mêmes lignes bloquées
  en bout environ 0,001 ms (9 à 13 ms avant). Reste à faire : la pose d'un convoyeur recalcule
  encore la topologie de tout le réseau (environ 18 ms pour 25 000 convoyeurs, 51 ms avant).
- Une boucle de convoyeurs remplie à 100 % circule désormais (corrigé).
- Limite provisoire du modèle de lignes : vitesse d'un convoyeur au plus égale à l'écart
  minimal (64 unités par tick), vérifiée au chargement du contenu.
- Électricité (étape 3) : choix provisoires signalés [O] — capacités de liaison (poteau
  500 kW, sous-station 20 MW, pylône 50 MW), portées (9, 12, 32 cases), zone d'alimentation
  de la sous-station (7 cases), période de recalcul du flot (30 ticks) et réserve jusqu'à la
  puissance nominale des machines raccordées (au lieu d'un seuil de variation de la demande).
  Banc F3 (1 500 sous-stations maillées, serveur x86, indicatif) : environ 0,7 à 1 ms par tick
  contre 0,5 ms avec l'ancien calcul global sans corridors ; un tick avec calcul de flot
  coûte environ 3 ms.
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
