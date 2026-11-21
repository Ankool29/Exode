# Règles pour les agents de codage

Référence : *Cahier de conception consolidé EXODE v0.4*. Statuts : [V] validé, [P] provisoire,
[R] recommandation, [O] ouvert.

## Gouvernance

- Ne jamais transformer une recommandation [R] en exigence, ni trancher un point [O], sans
  demande explicite du concepteur. Signaler les choix provisoires dans le code et dans le README.
- Le contenu de `crates/exode-core/data/content.json` est provisoire : les listes de ressources,
  recettes et bâtiments restent ouvertes.

## Architecture (v0.4 section 9)

- `exode-core` ne dépend d'aucun moteur graphique. Godot n'est qu'une couche de présentation.
- L'interface envoie des commandes (`build`, `remove_at`, `set_recipe_at`...) ; elle ne modifie
  jamais l'état directement. Le rendu lit des vues de la zone visible.
- Coordonnées de monde entières ; coordonnées de rendu relatives à une origine proche de la caméra.

## Déterminisme (décisions 03 et 21)

- Aucun flottant dans la simulation ni dans la génération du monde. Le bruit et le hasard
  sont ceux de `rng.rs`, jamais ceux de Godot ou d'une bibliothèque tierce.
- Ordre de traitement stable : `BTreeMap` pour tout ce qui est parcouru. Les tables de hachage
  ne servent qu'aux recherches.
- Toute modification du générateur doit incrémenter `GENERATOR_VERSION` et mettre à jour
  l'empreinte de référence du test `generator_reference_fingerprint`. Un monde existant ne doit
  jamais changer.
- Les chunks seulement affichés n'entrent pas dans l'empreinte d'état : la caméra ne change rien.

## Règles de gameplay déjà codées

- Même comportement à l'écran et hors écran ; si le matériel sature, la simulation ralentit
  (`MAX_TICKS_PER_FRAME`) sans changer les règles.
- Conservation de la matière : une recette ne peut pas produire plus de masse qu'elle n'en
  consomme ; `Simulation::mass_balance_ok` doit rester vrai.
- Réseaux (étape 3) : socle commun `networks.rs` (composantes, identifiant canonique = plus
  petit identifiant d'entité, version par réseau). Électricité : la puissance servie ne dépasse
  jamais la production ni la capacité d'une liaison ; le flot est recalculé au changement de
  topologie et toutes les `FLOW_PERIOD` ticks, et ses solutions font partie de la sauvegarde.
- Fluides : un segment de conduites a un seul fluide, un volume et une température ; le fluide
  ne passe d'un volume à l'autre que par une pompe, une vanne, un réservoir ou une machine, avec
  un débit plafonné ; deux fluides ne se mélangent jamais ; seule la pompe à eau fait entrer de
  la matière.
- Chaleur : énergies entières en W × tick ; bilan `Simulation::heat_balance_ok` (produite =
  stockée + perdue + transformée en vapeur ; combustible brûlé = chaleur produite + reste à
  brûler) ; la turbine ne livre que l'énergie de la vapeur qu'elle évacue (masse perdue).
- Générateur du monde : v2 depuis l'étape 3 (ressource C). Un monde garde la version de son
  générateur ; les empreintes v1 et v2 sont vérifiées par `generator_reference_fingerprint`.
- Déchets [R] : un bras ne dépose des déchets que dans une benne ou sur un convoyeur ; une
  machine dont la sortie de déchets est pleine s'arrête.

## Avant chaque envoi

```
cargo test -p exode-core
cargo build --release -p exode-godot
godot --headless --path game -- --selftest   # avec la bibliothèque copiée dans game/bin
```
