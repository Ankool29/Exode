# EXODE — À corriger

Liste des défauts connus, mise à jour au fil des lots. Les défauts majeurs bloquent
l'intégration ; les mineurs sont intégrés et corrigés plus tard.

Gravité : **majeur** (bloque), **moyen** (visible, gênant), **mineur** (finition).
Responsable : **sprites** (production graphique) ou **code** (moteur, intégration).

| N° | Domaine | Élément | Problème | Gravité | Responsable | Origine | Statut |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Terrain | Herbe, terre, eau 256 px | Répétition encore perceptible (bandes claires régulières sur grande surface) | mineur | sprites | P0 V5 | ouvert |
| 2 | Terrain | Sable | Pas de texture ni de transitions (sable–eau, herbe–sable) : affiché en terre teintée, bords en escalier | moyen | sprites | P0 V5 | ouvert |
| 3 | Terrain | Rives | Masque par côtés + 4 coins : rives en escalier sur les grands lacs. Un jeu complet de 47 tuiles lisserait les formes | mineur | sprites | P0 V5 | ouvert |
| 4 | Bras | Pièces du bras | Un seul segment dessiné sous 4 angles, sans point de pivot : bras non intégré (affichage provisoire) | moyen | sprites | P0 V2–V5 | ouvert |
| 5 | Machines | Pièces mobiles foreuse et assembleur | Pas de position dans les métadonnées : non intégrées | mineur | sprites | P0 V2–V5 | ouvert |
| 6 | Bâtiments | Poteau, coffre, benne, générateur | Style en aplats, sans le détail des machines ; la benne n'a pas la bande brun-olive des déchets | moyen | sprites | P0 V2–V5 | ouvert (intégrés provisoirement) |
| 7 | Interface | Icônes | Formes de couleur sans signification, doublons ; style d'interface (pixel ou vectoriel) non tranché | moyen | sprites | P0 V2–V5 | ouvert (non intégrées) |
| 8 | Convoyeurs | Virages | Les chevrons animés ne sont dessinés que sur les pièces droites | mineur | code | P0 V5 | ouvert |
| 9 | Nature | Arbres | Dessinés avant les bâtiments : un bâtiment au nord d'un arbre recouvre sa couronne | mineur | code | P0 V5 | ouvert |
| 10 | Gisements | Correspondance ressources / fichiers | Associées par lettre (ressource-a, -b) : à remplacer par la clé de ressource du noyau | mineur | code | P0 V2 | ouvert |
| 11 | Convoyeurs | Simulation | Une boucle remplie à 100 % se fige ; coût élevé au-delà de 200 000 objets (banc F1) | moyen | code | v0.1 | ouvert |
