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
| 11 | Convoyeurs | Simulation | Corrigé (banc F1) : la boucle pleine circule ; 200 000 objets ≈ 0,01–0,02 ms/tick en mouvement, ≈ 0,001 ms bloqués (12,8 et 9–13 ms avant, serveur x86, indicatif). Reste : la pose d'un convoyeur recalcule tout le réseau (≈ 18 ms pour 25 000 convoyeurs) | moyen | code | v0.1 | corrigé (reste mineur) |
| 12 | Réseaux | Poteau (étape 3 V3) | Peint en vert vif, hors du style acier et cuivre des autres bâtiments | mineur | sprites | Étape 3 V3 | ouvert |
| 13 | Réseaux | Chaudière, échangeur, turbine (_n et _s) | Orientations nord et sud presque identiques : seuls les raccords changent | mineur | sprites | Étape 3 V3 | ouvert |
| 14 | Réseaux | Sous-station | Accroches de câbles fausses dans la fiche : recalculées à la main sur les isolateurs | mineur | sprites | Étape 3 V2–V3 | corrigé dans la livraison |
| 15 | Convoyeurs | Pose et retrait | Recalcul de toute la topologie à chaque pose : environ 14 ms pour 25 000 convoyeurs (à-coup possible en pose glissée) | mineur | code | Banc F1 | ouvert |
| 16 | Réseaux | Poteau (étape 3 V3) | Le poteau V3 remplace le poteau P0 (gardé dans le dossier pour revenir en arrière) ; voir aussi le point 12 | mineur | sprites | Étape 3 V3 | intégré provisoirement |
| 17 | Réseaux | Électricité | Valeurs provisoires [O] : capacités de liaison (poteau 500 kW, sous-station 20 MW, pylône 50 MW), portées, période de recalcul du flot (30 ticks) | moyen | code | Étape 3 | ouvert |
| 18 | Réseaux | Électricité | Un tick avec recalcul du flot coûte environ 3 ms sur 1 500 sous-stations (banc F3) : à étaler entre plusieurs ticks si les appareils de référence le demandent | mineur | code | Étape 3 | ouvert |
| 19 | Réseaux | Fluides | Valeurs provisoires [O] : volumes, débits, pompe à eau sans électricité, vanne à sens unique | moyen | code | Étape 3 | ouvert |
| 20 | Réseaux | Raccords des bâtiments | Le raccord d'un côté occupe une case précise (milieu du côté arrondi), alors que le sprite le dessine au milieu exact pour les bâtiments de largeur paire | mineur | code | Étape 3 | ouvert |
| 21 | Réseaux | Conduites souterraines | Une entrée peut se relier à deux sorties si l'on en pose une troisième dans le tunnel ; elle ne se tourne pas (à démonter puis reposer) | mineur | code | Étape 3 | ouvert |
| 22 | Réseaux | Conduites | Pas de calque de niveau pour les conduites (seulement pour le réservoir) : le fluide n'est visible que dans le panneau | mineur | sprites | Étape 3 V3 | ouvert |
