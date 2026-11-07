//! Maillage triangulaire, réparation, statistiques et transformations.

use glam::{DAffine3, DVec3};
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

        let inner_positions = offset_surface_inward(&outer, thickness);
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
                "Épaisseur {thickness:.2} mm trop importante pour cette géométrie : les parois internes se croisent."
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
}

/// Décale chaque sommet vers l'intérieur du volume de `thickness`.
///
/// Pour un sommet, on résout au mieux le système `n_i · p' = n_i · p - t`
/// pour toutes les faces adjacentes `i`. Un repli sur la normale moyenne est
/// utilisé lorsque le système est dégénéré (sommet coplanaire).
fn offset_surface_inward(mesh: &Mesh, thickness: f64) -> Vec<[f32; 3]> {
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

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let p = mesh.pos(i as u32);
        let [xx, yy, zz, xy, xz, yz] = matrix[i];
        let [bx, by, bz] = rhs[i];
        let det = xx * (yy * zz - yz * yz) - xy * (xy * zz - yz * xz)
            + xz * (xy * yz - yy * xz);
        let scale = (xx + yy + zz).max(1e-12);
        if det.abs() > 1e-10 * scale * scale * scale {
            let det_x = bx * (yy * zz - yz * yz) - xy * (by * zz - yz * bz)
                + xz * (by * yz - yy * bz);
            let det_y = xx * (by * zz - yz * bz) - bx * (xy * zz - yz * xz)
                + xz * (xy * bz - by * xz);
            let det_z = xx * (yy * bz - by * yz) - xy * (xy * bz - by * xz)
                + bx * (xy * yz - yy * xz);
            let q = DVec3::new(det_x / det, det_y / det, det_z / det);
            if q.is_finite() {
                out.push([q.x as f32, q.y as f32, q.z as f32]);
                continue;
            }
        }
        let s = DVec3::from_array(normal_sum[i]);
        if s.length_squared() > 1e-24 {
            let q = p - s.normalize() * thickness;
            out.push([q.x as f32, q.y as f32, q.z as f32]);
        } else {
            out.push([p.x as f32, p.y as f32, p.z as f32]);
        }
    }
    out
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
