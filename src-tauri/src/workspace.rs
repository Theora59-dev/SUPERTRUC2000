//! Espace de travail : pièces, historique, découpe, transformations, export.

use crate::cut::{cut_mesh, ConnectorOptions, CutOptions, Plane};
use crate::io_3mf::write_3mf;
use crate::io_obj::write_obj;
use crate::io_stl::write_stl_binary;
use crate::mesh::{Bounds, Mesh, MeshStats};
use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const HISTORY_LIMIT: usize = 16;
const MAX_PIECES: usize = 256;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bed {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub margin: f64,
}

impl Default for Bed {
    fn default() -> Self {
        Self {
            x: 220.0,
            y: 220.0,
            z: 250.0,
            margin: 5.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn normal(self) -> DVec3 {
        match self {
            Axis::X => DVec3::X,
            Axis::Y => DVec3::Y,
            Axis::Z => DVec3::Z,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }
}

#[derive(Clone)]
pub struct Part {
    pub id: u64,
    pub name: String,
    pub mesh: Arc<Mesh>,
    pub stats: MeshStats,
    pub bounds: Bounds,
    pub revision: u64,
}

impl Part {
    fn new(id: u64, name: String, mesh: Mesh) -> Self {
        let stats = mesh.stats();
        let bounds = mesh.bounds();
        Self {
            id,
            name,
            mesh: Arc::new(mesh),
            stats,
            bounds,
            revision: 1,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartMeta {
    pub id: u64,
    pub name: String,
    pub triangles: usize,
    pub vertices: usize,
    pub volume: f64,
    pub area: f64,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub size: [f64; 3],
    pub center: [f64; 3],
    pub revision: u64,
}

impl From<&Part> for PartMeta {
    fn from(p: &Part) -> Self {
        Self {
            id: p.id,
            name: p.name.clone(),
            triangles: p.stats.triangles,
            vertices: p.stats.vertices,
            volume: p.stats.volume,
            area: p.stats.area,
            min: p.bounds.min,
            max: p.bounds.max,
            size: p.bounds.size,
            center: p.bounds.center,
            revision: p.revision,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub parts: Vec<PartMeta>,
    pub bed: Bed,
    pub undo_depth: usize,
    pub redo_depth: usize,
    pub total_triangles: usize,
    pub total_volume: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub workspace: WorkspaceInfo,
    pub message: String,
    pub warnings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransformOp {
    Translate { x: f64, y: f64, z: f64 },
    Rotate { axis: Axis, degrees: f64 },
    Scale { factor: f64 },
    Size { height: f64 },
    FitBed,
    DropToBed,
    Center,
    Mirror { axis: Axis },
}

pub struct Workspace {
    pub parts: Vec<Part>,
    next_id: u64,
    next_name: u64,
    pub bed: Bed,
    undo: Vec<Vec<Part>>,
    redo: Vec<Vec<Part>>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            parts: Vec::new(),
            next_id: 1,
            next_name: 1,
            bed: Bed::default(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl Workspace {
    pub fn info(&self) -> WorkspaceInfo {
        WorkspaceInfo {
            parts: self.parts.iter().map(PartMeta::from).collect(),
            bed: self.bed,
            undo_depth: self.undo.len(),
            redo_depth: self.redo.len(),
            total_triangles: self.parts.iter().map(|p| p.stats.triangles).sum(),
            total_volume: self.parts.iter().map(|p| p.stats.volume).sum(),
        }
    }

    fn snapshot(&mut self) {
        self.undo.push(self.parts.clone());
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(self.parts.clone());
            self.parts = prev;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.parts.clone());
            self.parts = next;
            true
        } else {
            false
        }
    }

    pub fn set_bed(&mut self, bed: Bed) {
        self.bed = Bed {
            x: bed.x.clamp(20.0, 5000.0),
            y: bed.y.clamp(20.0, 5000.0),
            z: bed.z.clamp(20.0, 5000.0),
            margin: bed.margin.clamp(0.0, 100.0),
        };
    }

    pub fn clear(&mut self) {
        if self.parts.is_empty() {
            return;
        }
        self.snapshot();
        self.parts.clear();
    }

    pub fn import_mesh(&mut self, base_name: String, mut mesh: Mesh) -> Result<u64, String> {
        if mesh.is_empty() {
            return Err("Le modèle importé ne contient aucun triangle.".into());
        }
        mesh.compact();
        // Poser sur le plateau et centrer l'emprise.
        let b = mesh.bounds();
        let delta = DVec3::new(-b.center[0], -b.center[1], -b.min[2]);
        let mesh = mesh.translated(delta);
        self.snapshot();
        let id = self.alloc_id();
        let name = self.alloc_name(&base_name);
        self.parts.push(Part::new(id, name, mesh));
        Ok(id)
    }

    pub fn add_mesh(&mut self, name: &str, mesh: Mesh) -> u64 {
        let id = self.alloc_id();
        let name = self.alloc_name(name);
        self.parts.push(Part::new(id, name, mesh));
        id
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn alloc_name(&mut self, base: &str) -> String {
        let base = base.split(" #").next().unwrap_or(base).trim();
        let base = if base.is_empty() { "Pièce" } else { base };
        let n = self.next_name;
        self.next_name += 1;
        format!("{base} #{n}")
    }

    fn find(&self, id: u64) -> Result<usize, String> {
        self.parts
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| "Pièce introuvable.".to_string())
    }

    pub fn delete(&mut self, id: u64) -> Result<(), String> {
        let idx = self.find(id)?;
        self.snapshot();
        self.parts.remove(idx);
        Ok(())
    }

    pub fn duplicate(&mut self, id: u64) -> Result<u64, String> {
        let idx = self.find(id)?;
        let part = self.parts[idx].clone();
        let offset = (part.bounds.size[0] + 10.0) as f64;
        let mesh = part
            .mesh
            .translated(DVec3::new(offset, 0.0, 0.0));
        self.snapshot();
        let new_id = self.alloc_id();
        let name = self.alloc_name(&format!("{} copie", part.name));
        self.parts.push(Part::new(new_id, name, mesh));
        Ok(new_id)
    }

    pub fn cut(
        &mut self,
        id: u64,
        axis: Axis,
        position: f64,
        connectors: ConnectorOptions,
    ) -> Result<ActionResult, String> {
        let idx = self.find(id)?;
        let base_name = self.parts[idx]
            .name
            .split(" #")
            .next()
            .unwrap_or("Pièce")
            .to_string();
        let part = self.parts[idx].clone();
        let point = axis.normal() * position;
        let plane = Plane::new(axis.normal(), point)?;
        let result = cut_mesh(
            &part.mesh,
            &plane,
            &CutOptions {
                connectors: ConnectorOptions {
                    depth: connectors.depth.max(1.0),
                    ..connectors
                },
            },
        )?;

        self.snapshot();
        self.parts.remove(idx);
        let neg_name = self.alloc_name(&format!("{base_name} {}", side_label(axis, false)));
        let pos_name = self.alloc_name(&format!("{base_name} {}", side_label(axis, true)));
        let neg_id = self.alloc_id();
        let pos_id = self.alloc_id();
        let neg = Part::new(neg_id, neg_name.clone(), result.negative);
        let pos = Part::new(pos_id, pos_name.clone(), result.positive);
        self.parts.insert(idx, neg);
        self.parts.insert(idx + 1, pos);

        let mut warnings = result.warnings;
        if result.connector_count > 0 {
            warnings.push(format!(
                "{} connecteur(s) de {:.1} mm de rayon ajouté(s).",
                result.connector_count, result.connector_radius
            ));
        }
        Ok(self.action(format!(
            "Découpe en 2 pièces : {neg_name} / {pos_name}.",
        ), warnings))
    }

    pub fn auto_split(
        &mut self,
        ids: Option<Vec<u64>>,
        connectors: ConnectorOptions,
    ) -> Result<ActionResult, String> {
        if self.parts.is_empty() {
            return Err("Aucune pièce à découper.".into());
        }
        let bed = self.bed;
        let targets: Vec<u64> = match ids {
            Some(v) if !v.is_empty() => v,
            _ => self.parts.iter().map(|p| p.id).collect(),
        };
        let mut plan: Vec<(usize, Vec<Plane>)> = Vec::new();
        let mut total_pieces = 0usize;
        for (idx, part) in self.parts.iter().enumerate() {
            if !targets.contains(&part.id) {
                continue;
            }
            let (planes, pieces) = split_planes(&part.bounds, bed);
            total_pieces += pieces;
            if !planes.is_empty() {
                plan.push((idx, planes));
            }
        }
        if plan.is_empty() {
            return Ok(self.action(
                "Toutes les pièces tiennent déjà dans le volume d'impression.".into(),
                Vec::new(),
            ));
        }
        if total_pieces > MAX_PIECES {
            return Err(format!(
                "La découpe produirait {total_pieces} pièces (maximum {MAX_PIECES}). Augmentez la taille du plateau ou découpez manuellement."
            ));
        }

        self.snapshot();
        let mut warnings = Vec::new();
        let mut new_parts: Vec<Part> = Vec::new();
        let mut piece_total = 0usize;
        let mut cut_count = 0usize;

        let old_parts = std::mem::take(&mut self.parts);
        for (idx, part) in old_parts.into_iter().enumerate() {
            let Some((_, planes)) = plan.iter().find(|(i, _)| *i == idx) else {
                new_parts.push(part);
                continue;
            };
            let base_name = part
                .name
                .split(" #")
                .next()
                .unwrap_or("Pièce")
                .to_string();
            let opts = CutOptions {
                connectors: ConnectorOptions {
                    depth: connectors.depth.clamp(1.0, 100.0),
                    radius: connectors.radius.clamp(0.5, 50.0),
                    clearance: connectors.clearance.clamp(0.0, 2.0),
                    ..connectors
                },
            };
            let mut pieces: Vec<Mesh> = Vec::new();
            apply_planes(&part.mesh, planes, &opts, &mut pieces, &mut warnings);
            cut_count += pieces.len().saturating_sub(1);
            for mesh in pieces {
                piece_total += 1;
                let id = self.alloc_id();
                let name = self.alloc_name(&format!("{base_name} p{}", piece_total));
                new_parts.push(Part::new(id, name, mesh));
            }
        }
        self.parts = new_parts;

        Ok(self.action(
            format!(
                "{piece_total} pièce(s) prêtes pour le plateau ({cut_count} coupe(s))."
            ),
            warnings,
        ))
    }

    pub fn transform(&mut self, id: u64, op: TransformOp) -> Result<ActionResult, String> {
        let idx = self.find(id)?;
        let part = self.parts[idx].clone();
        let b = part.bounds;
        let center = DVec3::from_array(b.center);
        let size = DVec3::from_array(b.size);
        let bed = self.bed;
        let (mesh, message) = match op {
            TransformOp::Translate { x, y, z } => (
                part.mesh.translated(DVec3::new(x, y, z)),
                "Pièce déplacée.".to_string(),
            ),
            TransformOp::Rotate { axis, degrees } => {
                let angle = degrees.to_radians();
                let m = DAffine3::from_translation(center)
                    * DAffine3::from_axis_angle(axis.normal(), angle)
                    * DAffine3::from_translation(-center);
                (
                    part.mesh.transformed(m),
                    format!("Rotation de {}° autour de {}.", degrees, axis.label()),
                )
            }
            TransformOp::Scale { factor } => {
                if !(factor.is_finite() && factor > 0.0) {
                    return Err("Le facteur d'échelle doit être supérieur à 0.".into());
                }
                let m = DAffine3::from_translation(center)
                    * DAffine3::from_scale(DVec3::splat(factor))
                    * DAffine3::from_translation(-center);
                (
                    part.mesh.transformed(m),
                    format!("Échelle appliquée : {:.0} %.", factor * 100.0),
                )
            }
            TransformOp::Size { height } => {
                if !(height.is_finite() && height > 0.0) || size.z < 1e-9 {
                    return Err("Hauteur cible invalide.".into());
                }
                let factor = height / size.z;
                let m = DAffine3::from_translation(center)
                    * DAffine3::from_scale(DVec3::splat(factor))
                    * DAffine3::from_translation(-center);
                (
                    part.mesh.transformed(m),
                    format!("Hauteur cible : {:.1} mm.", height),
                )
            }
            TransformOp::FitBed => {
                let usable = DVec3::new(
                    (bed.x - 2.0 * bed.margin).max(10.0),
                    (bed.y - 2.0 * bed.margin).max(10.0),
                    (bed.z - 2.0 * bed.margin).max(10.0),
                );
                let factor = (usable.x / size.x.max(1e-9))
                    .min(usable.y / size.y.max(1e-9))
                    .min(usable.z / size.z.max(1e-9));
                let m = DAffine3::from_translation(center)
                    * DAffine3::from_scale(DVec3::splat(factor))
                    * DAffine3::from_translation(-center);
                let scaled = part.mesh.transformed(m);
                let b2 = scaled.bounds();
                let dropped = scaled.translated(DVec3::new(0.0, 0.0, -b2.min[2]));
                (
                    dropped,
                    format!("Modèle ajusté au plateau ({:.0} %).", factor * 100.0),
                )
            }
            TransformOp::DropToBed => (
                part.mesh
                    .translated(DVec3::new(0.0, 0.0, -b.min[2])),
                "Pièce posée sur le plateau.".to_string(),
            ),
            TransformOp::Center => {
                let delta = DVec3::new(-b.center[0], -b.center[1], -b.min[2]);
                (
                    part.mesh.translated(delta),
                    "Pièce centrée sur le plateau.".to_string(),
                )
            }
            TransformOp::Mirror { axis } => {
                let mut s = DVec3::ONE;
                match axis {
                    Axis::X => s.x = -1.0,
                    Axis::Y => s.y = -1.0,
                    Axis::Z => s.z = -1.0,
                }
                let m = DAffine3::from_translation(center)
                    * DAffine3::from_scale(s)
                    * DAffine3::from_translation(-center);
                (
                    part.mesh.transformed(m),
                    format!("Miroir selon {}.", axis.label()),
                )
            }
        };
        self.snapshot();
        self.parts[idx].mesh = Arc::new(mesh);
        self.parts[idx].stats = self.parts[idx].mesh.stats();
        self.parts[idx].bounds = self.parts[idx].mesh.bounds();
        self.parts[idx].revision += 1;
        Ok(self.action(message, Vec::new()))
    }

    pub fn repair(&mut self, id: u64) -> Result<ActionResult, String> {
        let idx = self.find(id)?;
        let part = self.parts[idx].clone();
        let before_tri = part.stats.triangles;
        let before_vert = part.stats.vertices;
        let repaired = part.mesh.welded();
        if repaired.is_empty() {
            return Err("La réparation a supprimé toute la géométrie.".into());
        }
        let stats = repaired.stats();
        self.snapshot();
        self.parts[idx].mesh = Arc::new(repaired);
        self.parts[idx].stats = stats;
        self.parts[idx].bounds = self.parts[idx].mesh.bounds();
        self.parts[idx].revision += 1;
        Ok(self.action(
            format!(
                "Maillage réparé : {} → {} triangles, {} → {} sommets.",
                before_tri, stats.triangles, before_vert, stats.vertices
            ),
            Vec::new(),
        ))
    }

    /// Évide une pièce : ne conserve qu'une coque d'épaisseur `thickness`.
    /// L'épaisseur est réduite automatiquement si la géométrie l'exige.
    pub fn hollow(&mut self, id: u64, thickness: f64) -> Result<ActionResult, String> {
        let idx = self.find(id)?;
        let part = self.parts[idx].clone();
        let before = part.stats.volume;
        let (mesh, applied, warnings) = part.mesh.hollow_shell_auto(thickness)?;
        if mesh.is_empty() {
            return Err("L'évidement a supprimé toute la géométrie.".into());
        }
        let stats = mesh.stats();
        self.snapshot();
        self.parts[idx].mesh = Arc::new(mesh);
        self.parts[idx].stats = stats;
        self.parts[idx].bounds = self.parts[idx].mesh.bounds();
        self.parts[idx].revision += 1;
        let stem = part
            .name
            .split(" #")
            .next()
            .unwrap_or("Pièce")
            .to_string();
        let thickness_message = if applied + 1e-6 < thickness {
            format!(
                "parois réduites à {applied:.2} mm (épaisseur demandée : {thickness:.2} mm)"
            )
        } else {
            format!("parois de {applied:.2} mm")
        };
        Ok(self.action(
            format!(
                "{stem} évidé : {thickness_message}, volume {:.1} cm³ → {:.1} cm³.",
                before / 1000.0,
                stats.volume / 1000.0
            ),
            warnings,
        ))
    }

    pub fn arrange(&mut self, gap: f64) -> Result<ActionResult, String> {        if self.parts.is_empty() {
            return Err("Aucune pièce à disposer.".into());
        }
        let bed = self.bed;
        let gap = gap.clamp(0.0, 50.0);
        let mut sorted: Vec<usize> = (0..self.parts.len()).collect();
        sorted.sort_by(|a, b| {
            self.parts[*b]
                .stats
                .volume
                .partial_cmp(&self.parts[*a].stats.volume)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut placements: Vec<(usize, DVec3)> = Vec::with_capacity(self.parts.len());
        let mut x = -bed.x * 0.5 + bed.margin;
        let mut y = -bed.y * 0.5 + bed.margin;
        let mut row_depth = 0.0f64;
        let mut overflow = false;
        for idx in sorted {
            let b = self.parts[idx].bounds;
            let w = b.size[0];
            let d = b.size[1];
            if x + w > bed.x * 0.5 - bed.margin + 1e-6 && x > -bed.x * 0.5 + bed.margin + 1e-6 {
                x = -bed.x * 0.5 + bed.margin;
                y += row_depth + gap;
                row_depth = 0.0;
            }
            placements.push((idx, DVec3::new(x - b.min[0], y - b.min[1], -b.min[2])));
            x += w + gap;
            row_depth = row_depth.max(d);
            if y + d > bed.y * 0.5 - bed.margin + 1e-6 {
                overflow = true;
            }
        }

        self.snapshot();
        for (idx, delta) in placements {
            let mesh = self.parts[idx].mesh.translated(delta);
            self.parts[idx].mesh = Arc::new(mesh);
            self.parts[idx].bounds = self.parts[idx].mesh.bounds();
        }
        let mut warnings = Vec::new();
        if overflow {
            warnings.push(
                "Certaines pièces dépassent du plateau : réduisez-les ou découpez davantage."
                    .into(),
            );
        }
        Ok(self.action("Pièces disposées sur le plateau.".into(), warnings))
    }

    pub fn export(
        &self,
        ids: Option<Vec<u64>>,
        path: &str,
        format: ExportFormat,
    ) -> Result<Vec<PathBuf>, String> {
        let selected: Vec<&Part> = match ids {
            Some(v) if !v.is_empty() => self.parts.iter().filter(|p| v.contains(&p.id)).collect(),
            _ => self.parts.iter().collect(),
        };
        if selected.is_empty() {
            return Err("Aucune pièce à exporter.".into());
        }
        let path = PathBuf::from(path);
        match format {
            ExportFormat::Stl => {
                if selected.len() == 1 {
                    write_stl_binary(&selected[0].mesh, &path)?;
                    Ok(vec![path])
                } else {
                    std::fs::create_dir_all(&path)
                        .map_err(|e| format!("Impossible de créer le dossier : {e}"))?;
                    let mut written = Vec::new();
                    for (i, part) in selected.iter().enumerate() {
                        let name = format!(
                            "{:02}_{}.stl",
                            i + 1,
                            sanitize_filename(
                                part.name.split(" #").next().unwrap_or("piece")
                            )
                        );
                        let file = path.join(name);
                        write_stl_binary(&part.mesh, &file)?;
                        written.push(file);
                    }
                    Ok(written)
                }
            }
            ExportFormat::Obj => {
                let arranged = arrange_for_export(&selected, self.bed);
                let refs: Vec<(String, &Mesh)> = arranged
                    .iter()
                    .map(|(n, m)| (n.clone(), m))
                    .collect();
                write_obj(&refs, &path)?;
                Ok(vec![path])
            }
            ExportFormat::ThreeMf => {
                let arranged = arrange_for_export(&selected, self.bed);
                let refs: Vec<(String, &Mesh)> = arranged
                    .iter()
                    .map(|(n, m)| (n.clone(), m))
                    .collect();
                write_3mf(&refs, &path)?;
                Ok(vec![path])
            }
        }
    }
}

impl Workspace {
    fn action(&self, message: String, warnings: Vec<String>) -> ActionResult {
        ActionResult {
            workspace: self.info(),
            message,
            warnings,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Stl,
    Obj,
    #[serde(rename = "3mf")]
    ThreeMf,
}

fn side_label(axis: Axis, positive: bool) -> &'static str {
    match (axis, positive) {
        (Axis::X, false) => "−X",
        (Axis::X, true) => "+X",
        (Axis::Y, false) => "−Y",
        (Axis::Y, true) => "+Y",
        (Axis::Z, false) => "bas",
        (Axis::Z, true) => "haut",
    }
}

fn split_planes(bounds: &Bounds, bed: Bed) -> (Vec<Plane>, usize) {
    let usable = [
        (bed.x - 2.0 * bed.margin).max(10.0),
        (bed.y - 2.0 * bed.margin).max(10.0),
        (bed.z - 2.0 * bed.margin).max(10.0),
    ];
    let mut planes = Vec::new();
    let mut pieces = 1usize;
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let i = axis.index();
        let size = bounds.size[i];
        let n = (size / usable[i]).ceil().max(1.0) as usize;
        if n > 1 {
            pieces *= n;
            for k in 1..n {
                let pos = bounds.min[i] + size * k as f64 / n as f64;
                if let Ok(plane) = Plane::new(axis.normal(), axis.normal() * pos) {
                    planes.push(plane);
                }
            }
        }
    }
    (planes, pieces)
}

fn apply_planes(
    mesh: &Mesh,
    planes: &[Plane],
    opts: &CutOptions,
    out: &mut Vec<Mesh>,
    warnings: &mut Vec<String>,
) {
    if planes.is_empty() {
        out.push(mesh.clone());
        return;
    }
    let plane = planes[0];
    match cut_mesh(mesh, &plane, opts) {
        Ok(result) => {
            warnings.extend(result.warnings);
            apply_planes(&result.negative, &planes[1..], opts, out, warnings);
            apply_planes(&result.positive, &planes[1..], opts, out, warnings);
        }
        Err(_) => {
            // Le plan ne traverse pas cette pièce : on continue.
            apply_planes(mesh, &planes[1..], opts, out, warnings);
        }
    }
}

fn arrange_for_export(parts: &[&Part], bed: Bed) -> Vec<(String, Mesh)> {
    let gap = 4.0;
    let mut x = -bed.x * 0.5 + bed.margin;
    let mut y = -bed.y * 0.5 + bed.margin;
    let mut row_depth = 0.0f64;
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let b = part.bounds;
        if x + b.size[0] > bed.x * 0.5 - bed.margin + 1e-6
            && x > -bed.x * 0.5 + bed.margin + 1e-6
        {
            x = -bed.x * 0.5 + bed.margin;
            y += row_depth + gap;
            row_depth = 0.0;
        }
        let delta = DVec3::new(x - b.min[0], y - b.min[1], -b.min[2]);
        out.push((part.name.clone(), part.mesh.translated(delta)));
        x += b.size[0] + gap;
        row_depth = row_depth.max(b.size[1]);
    }
    out
}

fn sanitize_filename(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    s = s.trim().to_string();
    if s.is_empty() {
        s = "piece".into();
    }
    s.replace(' ', "_")
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Modele")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(size: f64) -> Mesh {
        let s = size as f32;
        let mut soup = Vec::new();
        let faces = [
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
    fn cut_cube_is_watertight() {
        let m = cube(20.0);
        let plane = Plane::new(DVec3::Z, DVec3::new(0.0, 0.0, 10.0)).unwrap();
        let mut opts = CutOptions::default();
        opts.connectors.enabled = false;
        let res = cut_mesh(&m, &plane, &opts).unwrap();
        assert_eq!(res.negative.open_edge_count(), 0, "moitié basse ouverte");
        assert_eq!(res.positive.open_edge_count(), 0, "moitié haute ouverte");
        let total = res.negative.stats().volume + res.positive.stats().volume;
        assert!((total - 8000.0).abs() < 1.0, "volume total {total}");

        let res = cut_mesh(&m, &plane, &CutOptions::default()).unwrap();
        assert_eq!(
            res.negative.open_edge_count(),
            0,
            "moitié basse avec tenons ouverte"
        );
        assert_eq!(
            res.positive.open_edge_count(),
            0,
            "moitié haute avec logements ouverte"
        );
        assert!(res.connector_count > 0);
        assert!(res.cap_area > 300.0);
    }

    #[test]
    fn auto_split_fits_bed() {
        let mut ws = Workspace::default();
        ws.bed = Bed {
            x: 50.0,
            y: 50.0,
            z: 50.0,
            margin: 2.0,
        };
        ws.import_mesh("Cube".into(), cube(100.0)).unwrap();
        let res = ws.auto_split(None, ConnectorOptions::default()).unwrap();
        assert!(res.workspace.parts.len() >= 8);
        for p in &res.workspace.parts {
            assert!(p.size[0] <= 50.0 + 1e-6);
            assert!(p.size[1] <= 50.0 + 1e-6);
            assert!(p.size[2] <= 50.0 + 1e-6);
        }
    }

    #[test]
    fn hollow_part_keeps_walls() {
        let mut ws = Workspace::default();
        let id = ws.import_mesh("Cube".into(), cube(20.0)).unwrap();
        let res = ws.hollow(id, 2.0).unwrap();
        let meta = &res.workspace.parts[0];
        assert_eq!(meta.triangles, 24);
        assert!(
            meta.volume > 3000.0 && meta.volume < 5000.0,
            "volume inattendu : {}",
            meta.volume
        );
        assert!(res.warnings.is_empty());
        assert_eq!(ws.parts[0].mesh.open_edge_count(), 0);
    }

    #[test]
    fn hollow_part_reduces_thickness_instead_of_failing() {
        let mut ws = Workspace::default();
        let id = ws.import_mesh("Cube".into(), cube(20.0)).unwrap();
        let res = ws.hollow(id, 15.0).unwrap();
        assert!(
            !res.warnings.is_empty(),
            "la réduction d'épaisseur doit être signalée"
        );
        assert!(res.message.contains("réduites"));
        assert_eq!(ws.parts[0].mesh.open_edge_count(), 0);
        assert!(ws.parts[0].stats.volume > 0.0);
    }

    #[test]
    fn export_roundtrip_all_formats() {
        let dir = std::env::temp_dir().join(format!(
            "supertruc-export-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut ws = Workspace::default();
        let id = ws.import_mesh("Cube".into(), cube(30.0)).unwrap();

        let stl_path = dir.join("cube.stl");
        let files = ws
            .export(None, stl_path.to_str().unwrap(), ExportFormat::Stl)
            .unwrap();
        assert_eq!(files.len(), 1);
        let stl_mesh = crate::io_stl::load_stl(&std::fs::read(&stl_path).unwrap()).unwrap();
        assert!(stl_mesh.triangle_count() > 0);

        let obj_path = dir.join("scene.obj");
        let files = ws
            .export(None, obj_path.to_str().unwrap(), ExportFormat::Obj)
            .unwrap();
        assert_eq!(files.len(), 1);
        let obj_mesh = crate::io_obj::load_obj(&std::fs::read(&obj_path).unwrap()).unwrap();
        assert!(obj_mesh.triangle_count() > 0);

        let mf_path = dir.join("scene.3mf");
        let files = ws
            .export(None, mf_path.to_str().unwrap(), ExportFormat::ThreeMf)
            .unwrap();
        assert_eq!(files.len(), 1);
        let mf = crate::io_3mf::load_3mf(&std::fs::read(&mf_path).unwrap()).unwrap();
        assert!(mf.mesh.triangle_count() > 0);

        ws.duplicate(id).unwrap();
        let pieces_dir = dir.join("pieces");
        let files = ws
            .export(None, pieces_dir.to_str().unwrap(), ExportFormat::Stl)
            .unwrap();
        assert_eq!(files.len(), 2);
        for file in files {
            assert!(std::fs::metadata(&file).unwrap().len() > 84);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
