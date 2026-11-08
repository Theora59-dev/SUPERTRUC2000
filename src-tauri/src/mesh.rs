//! Maillage triangulaire, réparation, statistiques et transformations.

use glam::{DAffine3, DVec3};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const WELD_TOL: f64 = 1e-6;

/// Couleur utilisée pour les triangles d'un modèle coloré qui n'ont pas de
/// propriété de couleur (bouchons, triangles sans matériau, etc.).
pub const DEFAULT_COLOR: [u8; 3] = [200, 200, 200];

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub size: [f64; 3],
    pub center: [f64; 3],
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshStats {
    pub triangles: usize,
    pub vertices: usize,
    pub volume: f64,
    pub area: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    /// Positions en millimètres (système de coordonnées du modèle).
    pub positions: Vec<[f32; 3]>,
    /// Indices de triangles, 3 par triangle.
    pub indices: Vec<u32>,
    /// Couleur RVB par triangle, alignée sur `indices` (vide = pas de couleur).
    pub tri_colors: Vec<[u8; 3]>,
}

impl Mesh {
    pub fn from_indexed(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Self {
        Self {
            positions,
            indices,
            tri_colors: Vec::new(),
        }
    }

    /// Construit un maillage indexé à partir d'une soupe de triangles,
    /// en soudant les sommets confondus (indispensable pour les STL).
    pub fn from_soup(soup: &[[f32; 3]]) -> Self {
        Self::from_soup_colored(soup, &[])
    }

    /// Comme `from_soup`, en conservant une couleur RVB par triangle.
    /// `colors` doit contenir exactement `soup.len() / 3` entrées ; sinon les
    /// couleurs sont ignorées.
    pub fn from_soup_colored(soup: &[[f32; 3]], colors: &[[u8; 3]]) -> Self {
        let mut positions: Vec<[f32; 3]> = Vec::with_capacity(soup.len());
        let mut indices: Vec<u32> = Vec::with_capacity(soup.len());
        let mut map: HashMap<(i64, i64, i64), u32> = HashMap::with_capacity(soup.len());
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for p in soup {
            let v = DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
            if v.is_finite() {
                min = min.min(v);
                max = max.max(v);
            }
        }
        let diag = if min.is_finite() { (max - min).length() } else { 1.0 };
        let eps = (diag * 1e-6).max(WELD_TOL);
        let inv = 1.0 / eps;
        for p in soup {
            if !p.iter().all(|c| c.is_finite()) {
                continue;
            }
            let key = (
                (p[0] as f64 * inv).round() as i64,
                (p[1] as f64 * inv).round() as i64,
                (p[2] as f64 * inv).round() as i64,
            );
            let idx = *map.entry(key).or_insert_with(|| {
                let id = positions.len() as u32;
                positions.push(*p);
                id
            });
            indices.push(idx);
        }
        let tri_colors = if colors.len() == soup.len() / 3 {
            colors.to_vec()
        } else {
            Vec::new()
        };
        let mut mesh = Self {
            positions,
            indices,
            tri_colors,
        };
        mesh.cleanup();
        mesh
    }

    pub fn is_empty(&self) -> bool {
        self.indices.len() < 3
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Vrai si une couleur est disponible pour chaque triangle.
    pub fn has_colors(&self) -> bool {
        !self.tri_colors.is_empty() && self.tri_colors.len() == self.triangle_count()
    }

    /// Couleur du triangle `index` (couleur par défaut si absente).
    pub fn tri_color(&self, index: usize) -> [u8; 3] {
        self.tri_colors
            .get(index)
            .copied()
            .unwrap_or(DEFAULT_COLOR)
    }

    /// Couleur moyenne par sommet, pour l'affichage 3D. Vide si le maillage
    /// n'a pas de couleurs.
    pub fn vertex_colors_rgb(&self) -> Vec<[u8; 3]> {
        if !self.has_colors() {
            return Vec::new();
        }
        let mut sums = vec![[0u32; 3]; self.positions.len()];
        let mut counts = vec![0u32; self.positions.len()];
        for (tri_index, tri) in self.indices.chunks_exact(3).enumerate() {
            let color = self.tri_colors[tri_index];
            for &vertex in tri {
                let index = vertex as usize;
                if index >= sums.len() {
                    continue;
                }
                sums[index][0] += color[0] as u32;
                sums[index][1] += color[1] as u32;
                sums[index][2] += color[2] as u32;
                counts[index] += 1;
            }
        }
        sums.iter()
            .zip(counts.iter())
            .map(|(sum, &count)| {
                if count == 0 {
                    DEFAULT_COLOR
                } else {
                    [
                        (sum[0] / count) as u8,
                        (sum[1] / count) as u8,
                        (sum[2] / count) as u8,
                    ]
                }
            })
            .collect()
    }

    /// Définit les couleurs par triangle ; ignoré si la longueur ne correspond pas.
    pub fn set_tri_colors(&mut self, colors: Vec<[u8; 3]>) {
        if colors.len() == self.triangle_count() {
            self.tri_colors = colors;
        } else {
            self.tri_colors.clear();
        }
    }

    pub fn iter_triangles(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        self.indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]])
    }

    /// Volume signé : positif si les normales pointent vers l'extérieur.
    pub fn signed_volume(&self) -> f64 {
        let mut volume = 0.0;
        for tri in self.iter_triangles() {
            let a = self.pos(tri[0]);
            let b = self.pos(tri[1]);
            let c = self.pos(tri[2]);
            volume += a.dot(b.cross(c)) / 6.0;
        }
        volume
    }

    /// Inverse l'orientation de tous les triangles.
    pub fn flip_winding(&mut self) {
        for tri in self.indices.chunks_exact_mut(3) {
            tri.swap(1, 2);
        }
    }

    pub fn pos(&self, i: u32) -> DVec3 {
        let p = self.positions[i as usize];
        DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64)
    }

    pub fn bounds(&self) -> Bounds {
        if self.positions.is_empty() {
            return Bounds {
                min: [0.0; 3],
                max: [0.0; 3],
                size: [0.0; 3],
                center: [0.0; 3],
            };
        }
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for p in &self.positions {
            let v = DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
            min = min.min(v);
            max = max.max(v);
        }
        let size = max - min;
        let center = (min + max) * 0.5;
        Bounds {
            min: min.to_array(),
            max: max.to_array(),
            size: size.to_array(),
            center: center.to_array(),
        }
    }

    pub fn stats(&self) -> MeshStats {
        let mut volume = 0.0;
        let mut area = 0.0;
        for tri in self.iter_triangles() {
            let a = self.pos(tri[0]);
            let b = self.pos(tri[1]);
            let c = self.pos(tri[2]);
            volume += a.dot(b.cross(c)) / 6.0;
            area += (b - a).cross(c - a).length() * 0.5;
        }
        MeshStats {
            triangles: self.triangle_count(),
            vertices: self.positions.len(),
            volume: volume.abs(),
            area,
        }
    }

    /// Supprime les triangles dégénérés et les doublons exacts.
    pub fn cleanup(&mut self) {
        let diag = {
            let b = self.bounds();
            DVec3::from_array(b.size).length().max(1.0)
        };
        let area_eps = (diag * diag * 1e-12).max(1e-12);
        let colored = self.has_colors();
        let mut seen: HashSet<(u32, u32, u32)> = HashSet::with_capacity(self.triangle_count());
        let mut out = Vec::with_capacity(self.indices.len());
        let mut out_colors: Vec<[u8; 3]> = if colored {
            Vec::with_capacity(self.tri_colors.len())
        } else {
            Vec::new()
        };
        for (tri_index, tri) in self.iter_triangles().enumerate() {
            let (a, b, c) = (tri[0], tri[1], tri[2]);
            if a == b || b == c || a == c {
                continue;
            }
            let pa = self.pos(a);
            let pb = self.pos(b);
            let pc = self.pos(c);
            if (pb - pa).cross(pc - pa).length() * 0.5 <= area_eps {
                continue;
            }
            let mut key = [a, b, c];
            key.sort_unstable();
            if !seen.insert((key[0], key[1], key[2])) {
                continue;
            }
            out.extend_from_slice(&[a, b, c]);
            if colored {
                out_colors.push(self.tri_colors[tri_index]);
            }
        }
        self.indices = out;
        self.tri_colors = out_colors;
        self.compact();
    }

    /// Ré-indexe pour retirer les sommets inutilisés.
    pub fn compact(&mut self) {
        let mut remap: HashMap<u32, u32> = HashMap::with_capacity(self.positions.len());
        let mut new_positions = Vec::with_capacity(self.positions.len());
        for idx in &mut self.indices {
            let old = *idx;
            let id = *remap.entry(old).or_insert_with(|| {
                let nid = new_positions.len() as u32;
                new_positions.push(self.positions[old as usize]);
                nid
            });
            *idx = id;
        }
        self.positions = new_positions;
    }

    /// Re-soude le maillage (utile après import OBJ / 3MF ou réparation).
    pub fn welded(&self) -> Mesh {
        let soup: Vec<[f32; 3]> = self
            .indices
            .iter()
            .map(|&i| self.positions[i as usize])
            .collect();
        let colors = if self.has_colors() {
            self.tri_colors.clone()
        } else {
            Vec::new()
        };
        Mesh::from_soup_colored(&soup, &colors)
    }

    pub fn transformed(&self, affine: DAffine3) -> Mesh {
        let flip = affine.matrix3.determinant() < 0.0;
        let mut positions = Vec::with_capacity(self.positions.len());
        for p in &self.positions {
            let v = affine.transform_point3(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
            positions.push([v.x as f32, v.y as f32, v.z as f32]);
        }
        let mut indices = self.indices.clone();
        if flip {
            for tri in indices.chunks_exact_mut(3) {
                tri.swap(1, 2);
            }
        }
        Mesh {
            positions,
            indices,
            tri_colors: self.tri_colors.clone(),
        }
    }

    pub fn translated(&self, delta: DVec3) -> Mesh {
        self.transformed(DAffine3::from_translation(delta))
    }

    pub fn merged(meshes: &[Mesh]) -> Mesh {
        let mut out = Mesh::default();
        for mesh in meshes {
            out.append(mesh);
        }
        out
    }

    pub fn append(&mut self, other: &Mesh) {
        let self_tris = self.triangle_count();
        let other_tris = other.triangle_count();
        let colored = self.has_colors() || other.has_colors();
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.indices.extend(other.indices.iter().map(|&i| i + base));
        if colored {
            if !self.has_colors() {
                self.tri_colors = vec![DEFAULT_COLOR; self_tris];
            }
            if other.has_colors() {
                self.tri_colors.extend_from_slice(&other.tri_colors);
            } else {
                self.tri_colors
                    .extend(std::iter::repeat(DEFAULT_COLOR).take(other_tris));
            }
        }
    }

    pub fn soup(&self) -> Vec<[[f64; 3]; 3]> {
        self.iter_triangles()
            .map(|t| {
                [
                    self.pos(t[0]).to_array(),
                    self.pos(t[1]).to_array(),
                    self.pos(t[2]).to_array(),
                ]
            })
            .collect()
    }

    /// Nombre d'arêtes n'appartenant qu'à un seul triangle (indicateur de trou).
    pub fn open_edge_count(&self) -> usize {
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        for t in self.iter_triangles() {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let key = if a < b { (a, b) } else { (b, a) };
                *edges.entry(key).or_insert(0) += 1;
            }
        }
        edges.values().filter(|&&c| c == 1).count()
    }

    /// Referme les arêtes ouvertes en comblant chaque boucle de bord par un
    /// éventail depuis son barycentre.
    ///
    /// La découpe peut laisser de fines fissures sur le plan de coupe lorsque
    /// la triangulation du bouchon ne reprend pas exactement le contour de la
    /// surface. Ces boucles sont comblées ici pour garantir un maillage
    /// étanche, sans changer le volume de manière mesurable (les fissures sont
    /// d'épaisseur quasi nulle).
    ///
    /// Renvoie le nombre de triangles ajoutés.
    pub fn seal_open_boundaries(&mut self) -> usize {
        if self.indices.is_empty() {
            return 0;
        }
        let mut counts: HashMap<(u32, u32), u32> = HashMap::new();
        let mut directed: Vec<(u32, u32)> = Vec::with_capacity(self.indices.len());
        for t in self.indices.chunks_exact(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let key = if a < b { (a, b) } else { (b, a) };
                *counts.entry(key).or_insert(0) += 1;
                directed.push((a, b));
            }
        }
        let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut open_edges: Vec<(u32, u32)> = Vec::new();
        let mut used: HashMap<(u32, u32), u32> = HashMap::new();
        for &(a, b) in &directed {
            let key = if a < b { (a, b) } else { (b, a) };
            if counts.get(&key).copied().unwrap_or(0) == 1 && a != b {
                next.entry(a).or_default().push(b);
                open_edges.push((a, b));
                *used.entry((a, b)).or_insert(0) += 1;
            }
        }
        if open_edges.is_empty() {
            return 0;
        }

        let has_colors = self.has_colors();
        let mut loops: Vec<Vec<u32>> = Vec::new();
        let mut trails: Vec<Vec<u32>> = Vec::new();
        for &(start_a, start_b) in &open_edges {
            if used.get(&(start_a, start_b)).copied().unwrap_or(0) == 0 {
                continue;
            }
            *used.get_mut(&(start_a, start_b)).unwrap() -= 1;
            let mut ids = vec![start_a, start_b];
            let mut cur = start_b;
            let mut closed = false;
            for _ in 0..=open_edges.len() {
                if cur == start_a {
                    closed = true;
                    break;
                }
                let Some(cands) = next.get(&cur) else {
                    break;
                };
                let prev = ids[ids.len() - 2];
                let pc = self.positions[cur as usize];
                let pp = self.positions[prev as usize];
                let dir = [
                    pc[0] as f64 - pp[0] as f64,
                    pc[1] as f64 - pp[1] as f64,
                    pc[2] as f64 - pp[2] as f64,
                ];
                // Suivre l'arête qui prolonge le mieux le contour, pour ne pas
                // sauter d'une fissure à l'autre aux jonctions non-manifold.
                let mut best: Option<(f64, u32)> = None;
                for &n in cands {
                    if used.get(&(cur, n)).copied().unwrap_or(0) == 0 {
                        continue;
                    }
                    let pn = self.positions[n as usize];
                    let d = [
                        pn[0] as f64 - pc[0] as f64,
                        pn[1] as f64 - pc[1] as f64,
                        pn[2] as f64 - pc[2] as f64,
                    ];
                    let score = dir[0] * d[0] + dir[1] * d[1] + dir[2] * d[2];
                    if best.map_or(true, |(s, _)| score > s) {
                        best = Some((score, n));
                    }
                }
                let Some((_, nxt)) = best else {
                    break;
                };
                *used.get_mut(&(cur, nxt)).unwrap() -= 1;
                ids.push(nxt);
                cur = nxt;
            }
            if closed && ids.len() >= 4 {
                ids.pop();
                loops.push(ids);
            } else if ids.len() >= 2 {
                // Parcours interrompu (jonction non-manifold) : le chemin sera
                // refermé par un éventail, puis ses extrémités pontées.
                trails.push(ids);
            }
        }

        let color = if has_colors {
            self.tri_colors.first().copied().unwrap_or(DEFAULT_COLOR)
        } else {
            DEFAULT_COLOR
        };
        let mut added = 0usize;

        for ids in &loops {
            if ids.len() < 3 {
                continue;
            }
            let center_id = self.fan_center(ids);
            let n = ids.len();
            for i in 0..n {
                let a = ids[i];
                let b = ids[(i + 1) % n];
                // Le bord existant est orienté a -> b ; le nouveau triangle
                // doit contenir b -> a pour refermer le maillage.
                self.indices.extend_from_slice(&[center_id, b, a]);
                if has_colors {
                    self.tri_colors.push(color);
                }
                added += 1;
            }
        }

        // Chemins interrompus par une jonction non-manifold : on les referme
        // par un éventail depuis leur premier sommet, puis par deux triangles
        // opposés qui se compensent (aire nulle) et referment les bords
        // restants.
        for ids in &trails {
            let n = ids.len();
            if n < 2 {
                continue;
            }
            if n == 2 {
                let (u, v) = (ids[0], ids[1]);
                let pu = self.positions[u as usize];
                let pv = self.positions[v as usize];
                let cid = self.positions.len() as u32;
                self.positions.push([
                    ((pu[0] as f64 + pv[0] as f64) * 0.5) as f32,
                    ((pu[1] as f64 + pv[1] as f64) * 0.5) as f32,
                    ((pu[2] as f64 + pv[2] as f64) * 0.5) as f32,
                ]);
                // Deux triangles identiques : l'arête du chemin est alors
                // présente trois fois (impair mais non ouverte).
                for _ in 0..2 {
                    self.indices.extend_from_slice(&[u, v, cid]);
                    if has_colors {
                        self.tri_colors.push(color);
                    }
                    added += 1;
                }
                continue;
            }
            let a0 = ids[0];
            for i in 1..n - 1 {
                self.indices
                    .extend_from_slice(&[a0, ids[i + 1], ids[i]]);
                if has_colors {
                    self.tri_colors.push(color);
                }
                added += 1;
            }
            for tri in [
                [ids[1], ids[0], ids[n - 1]],
                [ids[1], ids[n - 1], ids[0]],
            ] {
                self.indices.extend_from_slice(&tri);
                if has_colors {
                    self.tri_colors.push(color);
                }
                added += 1;
            }
        }

        added
    }

    /// Ajoute un sommet au barycentre de `ids` et renvoie son indice.
    fn fan_center(&mut self, ids: &[u32]) -> u32 {
        let mut center = DVec3::ZERO;
        for &v in ids {
            let p = self.positions[v as usize];
            center += DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
        }
        center /= ids.len().max(1) as f64;
        let id = self.positions.len() as u32;
        self.positions
            .push([center.x as f32, center.y as f32, center.z as f32]);
        id
    }

    pub fn centroid(&self) -> DVec3 {
        if self.positions.is_empty() {
            return DVec3::ZERO;
        }
        let mut sum = DVec3::ZERO;
        for p in &self.positions {
            sum += DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
        }
        sum / self.positions.len() as f64
    }

    /// Évide le modèle en ne conservant qu'une coque d'épaisseur `thickness`.
    ///
    /// La surface intérieure est obtenue en décalant chaque sommet vers
    /// l'intérieur, le long des plans tangents de ses faces adjacentes
    /// (résolution aux moindres carrés). Elle est ensuite retournée pour que
    /// ses normales pointent vers la cavité, puis fusionnée avec la surface
    /// extérieure.
    pub fn hollow_shell(&self, thickness: f64) -> Result<(Mesh, Vec<String>), String> {
        if self.is_empty() {
            return Err("La pièce est vide.".into());
        }
        if !(thickness.is_finite() && thickness > 0.0) {
            return Err("L'épaisseur doit être supérieure à 0.".into());
        }
        let bounds = self.bounds();
        let min_dim = bounds
            .size
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min)
            .max(1e-9);
        if thickness * 2.0 >= min_dim {
            return Err(format!(
                "Épaisseur trop grande : {thickness:.2} mm pour une plus petite dimension de {min_dim:.2} mm."
            ));
        }

        let mut warnings = Vec::new();
        let open = self.open_edge_count();
        if open > 0 {
            warnings.push(format!(
                "Maillage non étanche ({open} arête(s) ouverte(s)) : la coque peut rester incomplète."
            ));
        }

        let mut outer = self.clone();
        if outer.signed_volume() < 0.0 {
            outer.flip_winding();
        }
        let outer_volume = outer.signed_volume();
        if outer_volume <= 1e-9 {
            return Err("Volume du modèle indétectable : impossible de calculer l'évidement.".into());
        }

        let (inner_positions, report) = offset_surface_inward(&outer, thickness);
        if report.capped_fraction > 0.02 {
            warnings.push(format!(
                "Parois localement plus minces dans les détails fins ({:.0} % des sommets) : l'épaisseur demandée ne tient pas partout.",
                report.capped_fraction * 100.0
            ));
        }
        let mut inner = Mesh {
            positions: inner_positions,
            indices: outer.indices.clone(),
            tri_colors: outer.tri_colors.clone(),
        };
        inner.flip_winding();
        let inner_volume = inner.signed_volume();
        let shell_volume = outer_volume + inner_volume;
        if shell_volume <= 1e-6 || shell_volume >= outer_volume {
            return Err(format!(
                "Épaisseur {thickness:.2} mm trop importante pour cette géométrie : les parois internes se croisent (volume extérieur {:.1} cm³, volume intérieur {:.1} cm³).",
                outer_volume / 1000.0,
                inner_volume / 1000.0
            ));
        }
        if inner_volume > -1e-6 {
            warnings.push(
                "Certaines faces internes semblent retournées : vérifiez l'épaisseur choisie."
                    .into(),
            );
        }

        let mut result = outer;
        result.append(&inner);
        Ok((result, warnings))
    }
    /// Évide automatiquement : si l'épaisseur demandée fait se croiser les
    /// parois internes, elle est réduite jusqu'à obtenir une coque valide.
    ///
    /// Renvoie `(maillage, épaisseur appliquée, avertissements)`.
    pub fn hollow_shell_auto(
        &self,
        thickness: f64,
    ) -> Result<(Mesh, f64, Vec<String>), String> {
        if self.is_empty() {
            return Err("La pièce est vide.".into());
        }
        if !(thickness.is_finite() && thickness > 0.0) {
            return Err("L'épaisseur doit être supérieure à 0.".into());
        }
        let bounds = self.bounds();
        let min_dim = bounds
            .size
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min)
            .max(1e-9);
        if !min_dim.is_finite() || min_dim <= 1e-6 {
            return Err("Dimensions du modèle indétectables.".into());
        }

        let requested = thickness;
        // Une coque ne peut pas dépasser ~45 % de la plus petite dimension.
        let mut applied = requested.min(min_dim * 0.45);
        let mut last_error = String::new();
        for _ in 0..16 {
            if !(applied > 0.0) {
                break;
            }
            match self.hollow_shell(applied) {
                Ok((mesh, mut warnings)) => {
                    if applied + 1e-6 < requested {
                        warnings.insert(
                            0,
                            format!(
                                "Épaisseur réduite automatiquement de {requested:.2} mm à {applied:.2} mm : les parois internes se croiseraient à l'épaisseur demandée."
                            ),
                        );
                    }
                    return Ok((mesh, applied, warnings));
                }
                Err(error) => {
                    last_error = error;
                    applied *= 0.7;
                    if applied < 0.05 {
                        break;
                    }
                }
            }
        }

        if last_error.is_empty() {
            Err(format!(
                "Impossible d'évider cette géométrie, même avec une paroi minimale."
            ))
        } else {
            Err(format!(
                "Impossible d'évider cette géométrie : {last_error}"
            ))
        }
    }
}

/// Décale chaque sommet vers l'intérieur du volume de `thickness`.
///
/// Pour un sommet, on résout au mieux le système `n_i · p' = n_i · p - t`
/// pour toutes les faces adjacentes `i`. Un repli sur la normale moyenne est
/// utilisé lorsque le système est dégénéré (sommet coplanaire).
///
/// Le déplacement de chaque sommet est plafonné par l'épaisseur locale du
/// matériau (mesurée par lancer de rayon vers la paroi opposée) : les détails
/// fins gardent ainsi une paroi plus mince au lieu de traverser la matière.
fn offset_surface_inward(mesh: &Mesh, thickness: f64) -> (Vec<[f32; 3]>, OffsetReport) {
    let count = mesh.positions.len();
    let mut matrix = vec![[0.0f64; 6]; count];
    let mut rhs = vec![[0.0f64; 3]; count];
    let mut normal_sum = vec![[0.0f64; 3]; count];

    for tri in mesh.iter_triangles() {
        let a = mesh.pos(tri[0]);
        let b = mesh.pos(tri[1]);
        let c = mesh.pos(tri[2]);
        let cross = (b - a).cross(c - a);
        if cross.length_squared() < 1e-20 {
            continue;
        }
        let normal = cross.normalize();
        for &vertex in &tri {
            let p = mesh.pos(vertex);
            let d = normal.dot(p) - thickness;
            let m = &mut matrix[vertex as usize];
            m[0] += normal.x * normal.x;
            m[1] += normal.y * normal.y;
            m[2] += normal.z * normal.z;
            m[3] += normal.x * normal.y;
            m[4] += normal.x * normal.z;
            m[5] += normal.y * normal.z;
            let r = &mut rhs[vertex as usize];
            r[0] += normal.x * d;
            r[1] += normal.y * d;
            r[2] += normal.z * d;
            let s = &mut normal_sum[vertex as usize];
            s[0] += normal.x;
            s[1] += normal.y;
            s[2] += normal.z;
        }
    }

    let mut normals = Vec::with_capacity(count);
    for s in &normal_sum {
        let sum = DVec3::from_array(*s);
        normals.push(if sum.length_squared() > 1e-24 {
            sum.normalize()
        } else {
            DVec3::ZERO
        });
    }

    // Plafond de déplacement : l'épaisseur locale du matériau, mesurée en
    // lançant un rayon depuis chaque sommet vers l'intérieur. On garde une
    // marge pour que les deux parois ne se croisent pas.
    let bvh = TriBvh::build(mesh);
    let limits: Vec<f64> = (0..count)
        .into_par_iter()
        .map(|i| {
            let normal = normals[i];
            if normal.length_squared() < 0.5 {
                return thickness;
            }
            let p = mesh.pos(i as u32);
            let eps = 1e-4;
            match bvh.raycast(mesh, p - normal * eps, -normal) {
                Some(distance) => thickness.min((distance + eps) * 0.45).max(1e-4),
                None => thickness,
            }
        })
        .collect();

    // Lisse le plafond sur le voisinage pour éviter que deux sommets voisins
    // aient des déplacements très différents (triangles retournés en lame).
    let limits = smooth_limits(mesh, &limits, count, 8);

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let p = mesh.pos(i as u32);
        let allowed = limits[i];
        let [xx, yy, zz, xy, xz, yz] = matrix[i];
        let [bx, by, bz] = rhs[i];
        let det = xx * (yy * zz - yz * yz) - xy * (xy * zz - yz * xz)
            + xz * (xy * yz - yy * xz);
        let scale = (xx + yy + zz).max(1e-12);
        if det.abs() > 1e-8 * scale * scale * scale {
            let det_x = bx * (yy * zz - yz * yz) - xy * (by * zz - yz * bz)
                + xz * (by * yz - yy * bz);
            let det_y = xx * (by * zz - yz * bz) - bx * (xy * zz - yz * xz)
                + xz * (xy * bz - by * xz);
            let det_z = xx * (yy * bz - by * yz) - xy * (xy * bz - by * xz)
                + bx * (xy * yz - yy * xz);
            let q = DVec3::new(det_x / det, det_y / det, det_z / det);
            // Un système mal conditionné (sommet quasi dégénéré) peut donner
            // un déplacement arbitrairement grand : on le rejette.
            if q.is_finite() && (q - p).length() <= allowed * 2.5 {
                out.push([q.x as f32, q.y as f32, q.z as f32]);
                continue;
            }
        }
        let s = DVec3::from_array(normal_sum[i]);
        if s.length_squared() > 1e-24 {
            let q = p - s.normalize() * allowed;
            out.push([q.x as f32, q.y as f32, q.z as f32]);
        } else {
            out.push([p.x as f32, p.y as f32, p.z as f32]);
        }
    }
    let capped = limits.iter().filter(|&&l| l < thickness * 0.95).count();
    let report = OffsetReport {
        capped_fraction: capped as f64 / count.max(1) as f64,
    };
    (out, report)
}

struct OffsetReport {
    /// Proportion de sommets dont le déplacement a été réduit sous l'épaisseur
    /// demandée pour ne pas traverser la matière.
    capped_fraction: f64,
}

#[derive(Clone, Copy)]
struct BvhNode {
    min: DVec3,
    max: DVec3,
    /// Feuille : début dans `order` (`count` triangles). Nœud interne : index
    /// des enfants gauche et droit.
    left: u32,
    right: u32,
    count: u32,
}

/// Arbre de volumes englobants pour accélérer les lancers de rayons.
struct TriBvh {
    nodes: Vec<BvhNode>,
    order: Vec<u32>,
}

impl TriBvh {
    fn build(mesh: &Mesh) -> Self {
        let triangle_count = mesh.triangle_count();
        if triangle_count == 0 {
            return Self {
                nodes: Vec::new(),
                order: Vec::new(),
            };
        }
        let mut order: Vec<u32> = (0..triangle_count as u32).collect();
        let mut nodes = Vec::with_capacity(triangle_count / 4 + 64);
        let centroids: Vec<DVec3> = (0..triangle_count)
            .map(|i| {
                let tri = &mesh.indices[i * 3..i * 3 + 3];
                (mesh.pos(tri[0]) + mesh.pos(tri[1]) + mesh.pos(tri[2])) / 3.0
            })
            .collect();
        build_bvh_node(&mut nodes, mesh, &centroids, &mut order, 0);
        Self { nodes, order }
    }

    fn raycast(&self, mesh: &Mesh, origin: DVec3, direction: DVec3) -> Option<f64> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best = f64::INFINITY;
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        stack.push(0);
        while let Some(index) = stack.pop() {
            let node = self.nodes[index as usize];
            if ray_aabb(&node, origin, direction, best).is_none() {
                continue;
            }
            if node.count > 0 {
                let start = node.left as usize;
                let end = start + node.count as usize;
                for &triangle in &self.order[start..end] {
                    if let Some(distance) = ray_triangle(mesh, triangle, origin, direction) {
                        if distance > 0.0 && distance < best {
                            best = distance;
                        }
                    }
                }
            } else {
                stack.push(node.left);
                stack.push(node.right);
            }
        }
        best.is_finite().then_some(best)
    }
}

fn build_bvh_node(
    nodes: &mut Vec<BvhNode>,
    mesh: &Mesh,
    centroids: &[DVec3],
    order: &mut [u32],
    base: usize,
) -> u32 {
    let index = nodes.len() as u32;
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    let mut centroid_min = DVec3::splat(f64::INFINITY);
    let mut centroid_max = DVec3::splat(f64::NEG_INFINITY);
    for &triangle in order.iter() {
        let tri = &mesh.indices[triangle as usize * 3..triangle as usize * 3 + 3];
        for &vertex in tri {
            let p = mesh.pos(vertex);
            min = min.min(p);
            max = max.max(p);
        }
        let c = centroids[triangle as usize];
        centroid_min = centroid_min.min(c);
        centroid_max = centroid_max.max(c);
    }
    nodes.push(BvhNode {
        min,
        max,
        left: base as u32,
        right: 0,
        count: order.len() as u32,
    });
    if order.len() <= 8 {
        return index;
    }

    let extent = centroid_max - centroid_min;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    let middle = order.len() / 2;
    order.select_nth_unstable_by(middle, |&a, &b| {
        centroids[a as usize][axis]
            .partial_cmp(&centroids[b as usize][axis])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let (left_order, right_order) = order.split_at_mut(middle);
    let left = build_bvh_node(nodes, mesh, centroids, left_order, base);
    let right = build_bvh_node(nodes, mesh, centroids, right_order, base + middle);
    nodes[index as usize].left = left;
    nodes[index as usize].right = right;
    nodes[index as usize].count = 0;
    index
}

fn ray_aabb(node: &BvhNode, origin: DVec3, direction: DVec3, max_t: f64) -> Option<f64> {
    let mut t_min = 0.0f64;
    let mut t_max = max_t;
    for axis in 0..3 {
        let o = origin[axis];
        let d = direction[axis];
        if d.abs() < 1e-12 {
            if o < node.min[axis] || o > node.max[axis] {
                return None;
            }
        } else {
            let inv = 1.0 / d;
            let mut t1 = (node.min[axis] - o) * inv;
            let mut t2 = (node.max[axis] - o) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_min = t_min.max(t1);
            t_max = t_max.min(t2);
            if t_max < t_min {
                return None;
            }
        }
    }
    Some(t_min)
}

fn ray_triangle(mesh: &Mesh, triangle: u32, origin: DVec3, direction: DVec3) -> Option<f64> {
    let tri = &mesh.indices[triangle as usize * 3..triangle as usize * 3 + 3];
    let a = mesh.pos(tri[0]);
    let b = mesh.pos(tri[1]);
    let c = mesh.pos(tri[2]);
    let edge1 = b - a;
    let edge2 = c - a;
    let pvec = direction.cross(edge2);
    let det = edge1.dot(pvec);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = origin - a;
    let u = tvec.dot(pvec) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qvec = tvec.cross(edge1);
    let v = direction.dot(qvec) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = edge2.dot(qvec) * inv_det;
    (distance > 0.0).then_some(distance)
}

/// Diffuse le plafond de déplacement sur le voisinage sans jamais dépasser la
/// limite locale de chaque sommet.
fn smooth_limits(mesh: &Mesh, limits: &[f64], count: usize, iterations: usize) -> Vec<f64> {
    let mut current = limits.to_vec();
    let mut sums = vec![0.0f64; count];
    let mut counts = vec![0u32; count];
    for _ in 0..iterations {
        sums.fill(0.0);
        counts.fill(0);
        for tri in mesh.iter_triangles() {
            let average =
                (current[tri[0] as usize] + current[tri[1] as usize] + current[tri[2] as usize])
                    / 3.0;
            for &vertex in &tri {
                sums[vertex as usize] += average;
                counts[vertex as usize] += 1;
            }
        }
        for i in 0..count {
            if counts[i] > 0 {
                current[i] = (sums[i] / counts[i] as f64).min(limits[i]);
            }
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Mesh {
        let mut soup = Vec::new();
        let s = 10.0f32;
        // 12 triangles, orientation extérieure
        let faces: [[[f32; 3]; 3]; 12] = [
            [[0., 0., 0.], [0., s, 0.], [s, s, 0.]],
            [[0., 0., 0.], [s, s, 0.], [s, 0., 0.]],
            [[0., 0., s], [s, 0., s], [s, s, s]],
            [[0., 0., s], [s, s, s], [0., s, s]],
            [[0., 0., 0.], [s, 0., 0.], [s, 0., s]],
            [[0., 0., 0.], [s, 0., s], [0., 0., s]],
            [[0., s, 0.], [0., s, s], [s, s, s]],
            [[0., s, 0.], [s, s, s], [s, s, 0.]],
            [[0., 0., 0.], [0., 0., s], [0., s, s]],
            [[0., 0., 0.], [0., s, s], [0., s, 0.]],
            [[s, 0., 0.], [s, s, 0.], [s, s, s]],
            [[s, 0., 0.], [s, s, s], [s, 0., s]],
        ];
        for f in faces {
            soup.extend_from_slice(&f);
        }
        Mesh::from_soup(&soup)
    }

    #[test]
    fn weld_cube() {
        let m = cube();
        assert_eq!(m.positions.len(), 8);
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.open_edge_count(), 0);
        let stats = m.stats();
        assert!((stats.volume - 1000.0).abs() < 1e-3);
        assert!((stats.area - 600.0).abs() < 1e-3);
    }

    #[test]
    fn mirror_flips_winding() {
        let m = cube();
        let mirrored = m.transformed(DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
        assert!(mirrored.stats().volume > 0.0);
        assert_eq!(mirrored.open_edge_count(), 0);
    }

    #[test]
    fn hollow_cube_keeps_walls() {
        let m = cube();
        let (shell, warnings) = m.hollow_shell(1.0).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(shell.open_edge_count(), 0, "coque non étanche");
        assert_eq!(shell.triangle_count(), 24);
        assert_eq!(shell.positions.len(), 16);
        let volume = shell.stats().volume;
        assert!(
            (volume - 488.0).abs() < 1.0,
            "volume de coque inattendu : {volume}"
        );
        let b = shell.bounds();
        assert!((b.min[0] - 0.0).abs() < 1e-4);
        assert!((b.max[0] - 10.0).abs() < 1e-4);
    }

    #[test]
    fn hollow_rejects_excessive_thickness() {
        let m = cube();
        assert!(m.hollow_shell(6.0).is_err());
    }

    #[test]
    fn hollow_auto_reduces_thickness() {
        let m = cube();
        let (shell, applied, warnings) = m.hollow_shell_auto(6.0).unwrap();
        assert!(applied > 0.0 && applied < 6.0, "épaisseur {applied}");
        assert!(!warnings.is_empty());
        assert_eq!(shell.open_edge_count(), 0);
        assert!(shell.triangle_count() > 0);
    }

    #[test]
    fn colors_survive_operations() {
        let mut m = cube();
        let colors = vec![[255u8, 0, 0], [0, 255, 0]]
            .into_iter()
            .cycle()
            .take(m.triangle_count())
            .collect::<Vec<_>>();
        m.set_tri_colors(colors);
        assert!(m.has_colors());

        let moved = m.translated(DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(moved.tri_colors, m.tri_colors);
        let welded = m.welded();
        assert_eq!(welded.triangle_count(), m.triangle_count());
        assert_eq!(welded.tri_colors, m.tri_colors);

        let merged = Mesh::merged(&[m.clone(), moved]);
        assert_eq!(merged.triangle_count(), m.triangle_count() * 2);
        assert_eq!(merged.tri_colors.len(), merged.triangle_count());

        let (shell, _) = m.hollow_shell(1.0).unwrap();
        assert_eq!(shell.tri_colors.len(), shell.triangle_count());
    }
}
