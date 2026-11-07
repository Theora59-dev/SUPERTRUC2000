# SUPERTRUC 2000

Atelier de bureau pour **découper et préparer de grands modèles 3D destinés à l'impression**.
Application locale en **Rust (Tauri v2)** avec une interface React/three.js, inspirée de
[LuBan](https://luban3d.com).

Le binaire `LuBan` présent à la racine est conservé tel quel comme référence : il ne fait pas
partie de l'application et n'est pas modifié.

---

## Fonctionnalités

- **Import** de modèles STL (binaire et ASCII), OBJ et 3MF, par dialogue ou glisser-déposer.
  Les packages 3MF multi-fichiers (extension production `p:path`, utilisée notamment par Bambu Lab)
  sont pris en charge : la racine est résolue via `_rels/.rels`, les parties `.model` externes sont
  lues récursivement, et les transformations parent/enfant ainsi que les unités de chaque partie
  sont appliquées.
- **Découpe par plan** sur X, Y ou Z, avec position réglable et aperçu du plan dans la vue 3D.
- **Découpe automatique** : le modèle est découpé par plans répartis uniformément pour que chaque
  pièce tienne dans le volume d'impression, en tenant compte de la marge.
- **Connecteurs** cylindriques ajoutés automatiquement (tenons d'un côté, logements de l'autre),
  sans opération booléenne lourde.
- **Réparation** du maillage : soudure des sommets, suppression des triangles dégénérés, recalcul
  des normales.
- **Évidement** : vide l'intérieur d'un modèle plein en ne conservant qu'une coque d'épaisseur
  réglable. La surface intérieure est calculée par décalage des faces vers l'intérieur, puis
  retournée et fusionnée avec la surface extérieure. Le volume avant/après est indiqué, et un
  avertissement signale les maillages non étanches ou les épaisseurs trop grandes.
- **Couleurs 3MF** : les couleurs par triangle (`basematerials` et `m:colorgroup`) sont lues à
  l'import, propagées lors des découpes, réparations, transformations et évidements, puis
  réécrites à l'export 3MF. Les bouchons et connecteurs reprennent la couleur dominante de la
  pièce.
- **Transformations** : translation, rotation, mise à l'échelle, hauteur cible, ajustement au
  plateau, pose sur le plateau, centrage, miroir.
- **Organisation** : duplication, suppression, disposition automatique sur le plateau,
  sélection multiple.
- **Vue éclatée** : les pièces s'écartent radialement du centre de la scène, avec une animation
  douce, sans modifier la géométrie exportée. Le recadrage caméra tient compte de l'écartement.
- **Historique** annuler/rétablir (16 niveaux).
- **Export** :
  - STL binaire (un fichier par pièce, dans un dossier) ;
  - OBJ groupé (toutes les pièces dans un seul fichier) ;
  - 3MF groupé (toutes les pièces dans un seul fichier), avec conservation des couleurs.
  - Les exports OBJ et 3MF répartissent automatiquement les pièces sur le plateau.
- Aperçu 3D performant : la géométrie reste dans le backend Rust et transite par IPC binaire.
- Interface entièrement en français, thème sombre « atelier industriel ».

---

## Prérequis

- **Node.js 18+** et **npm**
- **Rust 1.77+** (avec `cargo`)
- Les dépendances système de Tauri v2.

Sur Debian/Ubuntu :

```bash
sudo apt update
sudo apt install -y build-essential curl wget file \
  libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

Sur les autres distributions, consultez la
[documentation des prérequis Tauri](https://v2.tauri.app/start/prerequisites/).

---

## Installation

```bash
cd SUPERTRUC2000
npm install
```

## Développement

```bash
npm run tauri dev
```

Vite démarre sur le port **1420** (port strict) et la fenêtre Tauri s'ouvre automatiquement.

## Build de production

```bash
npm run tauri build
```

Les binaires et paquets sont générés dans `src-tauri/target/release/` (et
`src-tauri/target/release/bundle/` pour les installeurs).

Pour compiler uniquement l'exécutable, sans créer d'installeur :

```bash
npm run tauri build -- --no-bundle
```

---

## Utilisation

1. **Importer** un modèle avec le bouton d'import ou en le glissant dans la fenêtre.
2. **Découpe** (onglet par défaut) :
   - choisissez l'axe et la position du plan ;
   - activez éventuellement les connecteurs et réglez leur nombre, rayon, profondeur et jeu ;
   - cliquez sur « Découper ». Les deux pièces apparaissent dans la liste.
3. **Auto** : ajustez le plateau (X, Y, Z, marge), puis lancez la découpe automatique sur toutes
   les pièces ou sur la sélection. L'application estime le nombre de pièces avant de couper.
4. **Édition** : sélectionnez une pièce dans la liste ou dans la vue 3D, puis appliquez une
   transformation, réparez-la, évidez-la, dupliquez-la ou supprimez-la.
   - **Évidement** : réglez l'épaisseur des parois (en mm) puis cliquez sur « Évider la pièce ».
     Le modèle doit être étanche pour obtenir une coque propre ; les parois internes se croisent
     si l'épaisseur est trop grande, et l'opération est alors refusée avec un message.
5. **Export** : choisissez le format, la portée (tout ou sélection) et le fichier/dossier de
   destination.

La **vue éclatée** s'active avec le bouton « Éclater » de la surcouche 3D ou la touche `E` ;
la touche `!` (ou un second appui) rassemble les pièces. L'écartement est purement visuel :
les fichiers exportés conservent les positions réelles des pièces.

Le plateau par défaut est de **220 × 220 × 250 mm** avec une marge de **5 mm**. Ces valeurs
servent au calcul de la découpe automatique et à la disposition des exports groupés.

### Raccourcis clavier

| Raccourci | Action |
| --- | --- |
| `Ctrl+O` | Importer un modèle |
| `Ctrl+Z` | Annuler |
| `Ctrl+Shift+Z` ou `Ctrl+Y` | Rétablir |
| `Ctrl+D` | Dupliquer la pièce sélectionnée |
| `Suppr` | Supprimer la pièce sélectionnée |
| `F` | Recadrer la caméra sur la scène |
| `E` | Éclater / rassembler les pièces |
| `!` | Rassembler les pièces |
| `Échap` | Désélectionner |

---

## Architecture

```
SUPERTRUC2000/
├── LuBan                  # binaire de référence d'origine (non utilisé)
├── assets/icon.svg        # source des icônes
├── index.html
├── src/                   # frontend React + TypeScript + three.js
│   ├── App.tsx            # interface, état, commandes, export
│   ├── Viewport.tsx       # scène 3D, caméra, sélection, aperçu
│   ├── api.ts             # contrat IPC et types partagés
│   ├── Icons.tsx          # icônes SVG
│   └── styles.css         # thème sombre
└── src-tauri/
    ├── src/
    │   ├── lib.rs         # enregistrement Tauri
    │   ├── main.rs
    │   ├── commands.rs    # commandes exposées au frontend
    │   ├── workspace.rs   # état, découpe, transformations, export, tests
    │   ├── cut.rs         # découpe plane, bouchons, connecteurs, couleurs
    │   ├── mesh.rs        # maillage, bornes, statistiques, évidement, couleurs
    │   ├── io_stl.rs      # lecture/écriture STL
    │   ├── io_obj.rs      # lecture/écriture OBJ
    │   └── io_3mf.rs      # lecture/écriture 3MF
    └── tauri.conf.json
```

### Principes techniques

- **Toute la géométrie est traitée en Rust.** Le frontend ne fait qu'afficher le résultat.
- **Aperçu binaire** : `preview_mesh` renvoie un `ArrayBuffer` composé de
  `u32 vertex_count`, `u32 index_count`, puis des positions `f32 × 3` et des indices `u32`
  en little-endian. Cela évite le coût du JSON pour les gros maillages.
- **Cache d'aperçu** : chaque pièce possède une `revision` ; le frontend ne recharge la géométrie
  que lorsque celle-ci change.
- **Découpe sans CSG** : les bouchons de coupe sont triangulés avec `earcutr`. Les connecteurs
  sont des cylindres : des tenons sur la pièce négative et des logements percés depuis le plan sur
  la pièce positive, ce qui reste rapide et robuste.
- **Couleurs par triangle** : `Mesh.tri_colors` stocke une couleur RGB par triangle (vide si le
  modèle n'en a pas). Toutes les opérations qui reconstruisent le maillage propagent ces couleurs ;
  l'export 3MF écrit un groupe `basematerials` par pièce avec `pid`/`p1` sur chaque triangle.
- **Évidement** : chaque sommet est décalé vers l'intérieur en résolvant au mieux les plans
  tangents de ses faces adjacentes (moindres carrés), avec repli sur la normale moyenne. La coque
  intérieure est inversée puis fusionnée à l'extérieur pour former une paroi étanche.
- **Calcul parallèle** avec `rayon` pour les opérations lourdes.

---

## Tests

Tests unitaires du backend (soudure, miroir, aller-retour STL, découpe étanche, découpe
automatique, évidement, couleurs par triangle et exports STL/OBJ/3MF) :

```bash
cd src-tauri
cargo test --lib
```

Vérification du frontend (TypeScript + build Vite) :

```bash
npm run build
```

---

## Limites connues

- La découpe se fait uniquement par **plans droits** alignés sur X, Y ou Z (pas de découpe
  courbe ou guidée par une surface).
- La découpe automatique est limitée à **256 pièces** par opération.
- Les connecteurs sont des **cylindres simples** ; il n'y a pas encore de tenons coniques, de
  queues d'aronde ni de vissage.
- Les maillages très lourds (plus de **1,5 million de triangles**) sont acceptés mais peuvent
  ralentir l'aperçu.
- Il n'existe pas encore de sauvegarde de projet : l'atelier est perdu à la fermeture.
- L'historique est limité à **16 niveaux**.
- L'évidement conserve une épaisseur constante et ne gère pas encore les ouvertures de vidange ;
  il est déconseillé sur les maillages ouverts ou les parois plus épaisses que la moitié de la
  plus petite dimension.
