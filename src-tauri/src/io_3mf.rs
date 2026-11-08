//! Lecture / écriture 3MF (package OPC + XML 3D Manufacturing Format).

use crate::mesh::{Mesh, DEFAULT_COLOR};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MODEL_PART: &str = "3D/3dmodel.model";

#[derive(Clone, Copy, Debug)]
struct Mat4 {
    // 4x4, convention vecteur ligne
    m: [[f64; 4]; 4],
}

impl Default for Mat4 {
    fn default() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

impl Mat4 {
    fn from_3mf(s: &str) -> Self {
        let vals: Vec<f64> = s
            .split_whitespace()
            .filter_map(|v| v.parse::<f64>().ok())
            .collect();
        if vals.len() != 12 {
            return Self::default();
        }
        Self {
            m: [
                [vals[0], vals[1], vals[2], 0.0],
                [vals[3], vals[4], vals[5], 0.0],
                [vals[6], vals[7], vals[8], 0.0],
                [vals[9], vals[10], vals[11], 1.0],
            ],
        }
    }

    fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        [
            p[0] * self.m[0][0] + p[1] * self.m[1][0] + p[2] * self.m[2][0] + self.m[3][0],
            p[0] * self.m[0][1] + p[1] * self.m[1][1] + p[2] * self.m[2][1] + self.m[3][1],
            p[0] * self.m[0][2] + p[1] * self.m[1][2] + p[2] * self.m[2][2] + self.m[3][2],
        ]
    }

    /// self appliqué d'abord, puis parent (vecteurs lignes).
    fn then(&self, parent: &Mat4) -> Mat4 {
        let mut out = [[0.0f64; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                let mut s = 0.0;
                for k in 0..4 {
                    s += self.m[r][k] * parent.m[k][c];
                }
                out[r][c] = s;
            }
        }
        Mat4 { m: out }
    }
}

#[derive(Clone, Default)]
struct ObjectDef {
    vertices: Vec<[f64; 3]>,
    triangles: Vec<Tri>,
    components: Vec<ComponentRef>,
    /// Groupe de couleurs par défaut de l'objet (`pid`).
    pid: Option<usize>,
    /// Indice de couleur par défaut de l'objet (`pindex`).
    pindex: usize,
}

#[derive(Clone)]
struct ComponentRef {
    id: usize,
    transform: Mat4,
    /// Partie de modèle externe (`p:path`), si le composant n'est pas local.
    path: Option<String>,
}

#[derive(Clone, Copy, Default)]
struct Tri {
    v: [u32; 3],
    /// Groupe de couleurs propre au triangle (`pid`).
    pid: Option<usize>,
    /// Indice de couleur du triangle (`p1`, éventuellement `p2`/`p3`).
    p1: Option<usize>,
    /// Indice dans la table des codes de peinture Bambu/Orca (`paint_color`).
    paint: Option<u32>,
}

pub struct Loaded3mf {
    pub mesh: Mesh,
    pub warnings: Vec<String>,
}

pub fn load_3mf(bytes: &[u8]) -> Result<Loaded3mf, String> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("Archive 3MF invalide : {e}"))?;
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let root_name = find_root_part(&mut archive, &names)?;
    let bambu = read_bambu_colors(&mut archive, &names);

    let mut warnings = Vec::new();
    let mut parts: HashMap<String, ParsedModel> = HashMap::new();
    load_part(&mut archive, &root_name, &names, &mut parts, &mut warnings)?;
    let root = parts
        .get(&root_name)
        .ok_or_else(|| "Partie de modèle 3MF racine introuvable.".to_string())?;

    let mut positions: Vec<[f64; 3]> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut tri_colors: Vec<Option<[u8; 3]>> = Vec::new();

    let roots: Vec<(usize, Mat4)> = if root.items.is_empty() {
        let referenced: HashSet<usize> = root
            .objects
            .values()
            .flat_map(|o| {
                o.components
                    .iter()
                    .filter(|c| c.path.is_none())
                    .map(|c| c.id)
            })
            .collect();
        let mut ids: Vec<usize> = root
            .objects
            .keys()
            .copied()
            .filter(|id| !referenced.contains(id))
            .collect();
        ids.sort_unstable();
        ids.into_iter().map(|id| (id, Mat4::default())).collect()
    } else {
        root.items.clone()
    };

    for (id, transform) in &roots {
        let before = triangles.len();
        let default_color = bambu
            .default_extruders
            .get(id)
            .and_then(|index| bambu.palette.get(*index))
            .copied();
        collect_leaf(
            &root_name,
            *id,
            &parts,
            *transform,
            0,
            &bambu,
            default_color,
            &mut positions,
            &mut triangles,
            &mut tri_colors,
            &mut warnings,
        );
        if triangles.len() == before {
            warnings.push(format!("Objet 3MF #{id} introuvable ou vide."));
        }
    }

    if triangles.is_empty() {
        return Err("Le fichier 3MF ne contient aucun triangle exploitable.".into());
    }

    let soup: Vec<[f32; 3]> = triangles
        .iter()
        .flatten()
        .map(|&i| {
            let p = positions[i as usize];
            [p[0] as f32, p[1] as f32, p[2] as f32]
        })
        .collect();
    let colors: Vec<[u8; 3]> = if tri_colors.iter().any(|c| c.is_some()) {
        tri_colors
            .iter()
            .map(|c| c.unwrap_or(DEFAULT_COLOR))
            .collect()
    } else {
        Vec::new()
    };
    let mesh = Mesh::from_soup_colored(&soup, &colors);
    Ok(Loaded3mf { mesh, warnings })
}

/// Lit une partie `.model` et charge récursivement les parties externes
/// référencées par des composants (`p:path`, extension production 3MF).
fn load_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    names: &[String],
    parts: &mut HashMap<String, ParsedModel>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if parts.contains_key(name) {
        return Ok(());
    }
    let xml = read_part(archive, name)?;
    let parsed = parse_model(&xml)?;
    let external: Vec<String> = parsed
        .objects
        .values()
        .flat_map(|o| o.components.iter().filter_map(|c| c.path.clone()))
        .collect();
    warnings.extend(parsed.warnings.iter().cloned());
    parts.insert(name.to_string(), parsed);

    for path in external {
        match resolve_archive_name(names, &path) {
            Some(part_name) => {
                if !parts.contains_key(&part_name) {
                    load_part(archive, &part_name, names, parts, warnings)?;
                }
            }
            None => warnings.push(format!(
                "Partie 3MF externe introuvable : {path} (le modèle peut être incomplet)."
            )),
        }
    }
    Ok(())
}

fn read_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<String, String> {
    let mut xml = String::new();
    let mut file = archive
        .by_name(name)
        .map_err(|e| format!("Lecture du modèle 3MF impossible ({name}) : {e}"))?;
    file.read_to_string(&mut xml)
        .map_err(|e| format!("Modèle 3MF illisible ({name}) : {e}"))?;
    Ok(xml)
}

fn resolve_archive_name(names: &[String], requested: &str) -> Option<String> {
    let target = requested.trim_start_matches('/');
    names
        .iter()
        .find(|name| {
            name.trim_start_matches('/')
                .eq_ignore_ascii_case(target)
        })
        .cloned()
}

fn resolve_part_key<'a>(
    parts: &'a HashMap<String, ParsedModel>,
    requested: &str,
) -> Option<&'a str> {
    let target = requested.trim_start_matches('/');
    if let Some((name, _)) = parts.get_key_value(target) {
        return Some(name.as_str());
    }
    parts
        .keys()
        .find(|name| name.trim_start_matches('/').eq_ignore_ascii_case(target))
        .map(String::as_str)
}

/// Détermine la partie racine du package à partir de `_rels/.rels`.
fn find_root_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    names: &[String],
) -> Result<String, String> {
    if let Some(rels_name) = resolve_archive_name(names, "_rels/.rels") {
        if let Ok(rels) = read_part(archive, &rels_name) {
            let mut reader = Reader::from_str(&rels);
            reader.config_mut().trim_text(true);
            loop {
                match reader.read_event() {
                    Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                        if e.local_name().as_ref() == b"Relationship" {
                            let kind = attr(&e, b"Type").unwrap_or_default();
                            if kind.to_ascii_lowercase().contains("3dmodel") {
                                if let Some(target) = attr(&e, b"Target") {
                                    if let Some(name) = resolve_archive_name(names, &target) {
                                        return Ok(name);
                                    }
                                }
                            }
                        }
                    }
                    Ok(Event::Eof) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        }
    }

    if let Some(name) = resolve_archive_name(names, MODEL_PART) {
        return Ok(name);
    }
    names
        .iter()
        .find(|name| name.to_ascii_lowercase().ends_with(".model"))
        .cloned()
        .ok_or_else(|| "Aucun modèle 3D trouvé dans l'archive.".to_string())
}

struct ParsedModel {
    objects: HashMap<usize, ObjectDef>,
    items: Vec<(usize, Mat4)>,
    /// Groupes de couleurs (`basematerials` et `m:colorgroup`), par identifiant.
    palettes: HashMap<usize, Vec<[u8; 3]>>,
    /// Codes `paint_color` dédupliqués (Bambu Studio / Orca Slicer).
    paint_codes: Vec<String>,
    unit: f64,
    warnings: Vec<String>,
}

/// Palette et extruder par défaut lus dans les métadonnées Bambu Studio.
#[derive(Default)]
struct BambuColors {
    palette: Vec<[u8; 3]>,
    /// Extruder par défaut (index 0) par identifiant d'objet.
    default_extruders: HashMap<usize, usize>,
}

fn parse_model(xml: &str) -> Result<ParsedModel, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut objects: HashMap<usize, ObjectDef> = HashMap::new();
    let mut items: Vec<(usize, Mat4)> = Vec::new();
    let mut palettes: HashMap<usize, Vec<[u8; 3]>> = HashMap::new();
    let mut paint_codes: Vec<String> = Vec::new();
    let mut paint_lookup: HashMap<String, u32> = HashMap::new();
    let mut current_group: Option<usize> = None;
    let mut unit = 1.0f64;
    let mut warnings = Vec::new();
    let mut current: Option<(usize, ObjectDef)> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"model" => {
                    if let Some(u) = attr(&e, b"unit") {
                        unit = match u.as_str() {
                            "micron" => 0.001,
                            "millimeter" => 1.0,
                            "centimeter" => 10.0,
                            "inch" => 25.4,
                            "foot" => 304.8,
                            "meter" => 1000.0,
                            _ => 1.0,
                        };
                    }
                }
                b"basematerials" | b"colorgroup" => {
                    if let Some(id) = attr(&e, b"id").and_then(|s| s.parse::<usize>().ok()) {
                        palettes.entry(id).or_default();
                        current_group = Some(id);
                    }
                }
                b"base" => {
                    if let Some(gid) = current_group {
                        if let Some(color) = attr(&e, b"displaycolor")
                            .and_then(|s| parse_color(&s))
                        {
                            palettes.entry(gid).or_default().push(color);
                        }
                    }
                }
                b"color" => {
                    if let Some(gid) = current_group {
                        if let Some(color) = attr(&e, b"color").and_then(|s| parse_color(&s)) {
                            palettes.entry(gid).or_default().push(color);
                        }
                    }
                }
                b"object" => {
                    if let Some(id) = attr(&e, b"id").and_then(|s| s.parse::<usize>().ok()) {
                        let obj = ObjectDef {
                            pid: attr(&e, b"pid").and_then(|s| s.parse::<usize>().ok()),
                            pindex: attr(&e, b"pindex")
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(0),
                            ..ObjectDef::default()
                        };
                        current = Some((id, obj));
                    }
                }
                b"vertex" => {
                    if let Some((_, obj)) = current.as_mut() {
                        let x = attr(&e, b"x").and_then(|s| s.parse::<f64>().ok());
                        let y = attr(&e, b"y").and_then(|s| s.parse::<f64>().ok());
                        let z = attr(&e, b"z").and_then(|s| s.parse::<f64>().ok());
                        if let (Some(x), Some(y), Some(z)) = (x, y, z) {
                            obj.vertices.push([x, y, z]);
                        }
                    }
                }
                b"triangle" => {
                    if let Some((_, obj)) = current.as_mut() {
                        let v1 = attr(&e, b"v1").and_then(|s| s.parse::<u32>().ok());
                        let v2 = attr(&e, b"v2").and_then(|s| s.parse::<u32>().ok());
                        let v3 = attr(&e, b"v3").and_then(|s| s.parse::<u32>().ok());
                        if let (Some(v1), Some(v2), Some(v3)) = (v1, v2, v3) {
                            let p1 = attr(&e, b"p1")
                                .and_then(|s| s.parse::<usize>().ok())
                                .or_else(|| {
                                    attr(&e, b"p2").and_then(|s| s.parse::<usize>().ok())
                                })
                                .or_else(|| {
                                    attr(&e, b"p3").and_then(|s| s.parse::<usize>().ok())
                                });
                            obj.triangles.push(Tri {
                                v: [v1, v2, v3],
                                pid: attr(&e, b"pid").and_then(|s| s.parse::<usize>().ok()),
                                p1,
                                paint: attr(&e, b"paint_color").map(|code| {
                                    intern_paint(&mut paint_codes, &mut paint_lookup, &code)
                                }),
                            });
                        }
                    }
                }
                b"component" => {
                    if let Some((_, obj)) = current.as_mut() {
                        if let Some(id) =
                            attr(&e, b"objectid").and_then(|s| s.parse::<usize>().ok())
                        {
                            let m = attr(&e, b"transform")
                                .map(|s| Mat4::from_3mf(&s))
                                .unwrap_or_default();
                            let path = attr(&e, b"path");
                            obj.components.push(ComponentRef {
                                id,
                                transform: m,
                                path,
                            });
                        }
                    }
                }
                b"item" => {
                    if let Some(id) = attr(&e, b"objectid").and_then(|s| s.parse::<usize>().ok()) {
                        let m = attr(&e, b"transform")
                            .map(|s| Mat4::from_3mf(&s))
                            .unwrap_or_default();
                        items.push((id, m));
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"object" => {
                    if let Some((id, obj)) = current.take() {
                        objects.insert(id, obj);
                    }
                }
                b"basematerials" | b"colorgroup" => current_group = None,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML 3MF invalide : {e}")),
            _ => {}
        }
    }

    if let Some((id, _)) = current {
        warnings.push(format!("Objet 3MF #{id} non terminé."));
    }

    Ok(ParsedModel {
        objects,
        items,
        palettes,
        paint_codes,
        unit,
        warnings,
    })
}

/// Ajoute un code `paint_color` à la table dédupliquée et renvoie son indice.
fn intern_paint(
    codes: &mut Vec<String>,
    lookup: &mut HashMap<String, u32>,
    value: &str,
) -> u32 {
    let key = value.trim().to_ascii_uppercase();
    if let Some(index) = lookup.get(&key) {
        return *index;
    }
    let index = codes.len() as u32;
    codes.push(key.clone());
    lookup.insert(key, index);
    index
}

fn collect_leaf(
    part_name: &str,
    id: usize,
    parts: &HashMap<String, ParsedModel>,
    transform: Mat4,
    depth: usize,
    bambu: &BambuColors,
    default_color: Option<[u8; 3]>,
    positions: &mut Vec<[f64; 3]>,
    triangles: &mut Vec<[u32; 3]>,
    tri_colors: &mut Vec<Option<[u8; 3]>>,
    warnings: &mut Vec<String>,
) {
    if depth > 32 {
        warnings.push("Hiérarchie 3MF trop profonde.".into());
        return;
    }
    let Some(part) = parts.get(part_name) else {
        return;
    };
    let Some(obj) = part.objects.get(&id) else {
        return;
    };
    for component in &obj.components {
        let child_part = match &component.path {
            Some(path) => match resolve_part_key(parts, path) {
                Some(name) => name,
                None => {
                    warnings.push(format!(
                        "Partie 3MF externe introuvable : {path} (composant ignoré)."
                    ));
                    continue;
                }
            },
            None => part_name,
        };
        collect_leaf(
            child_part,
            component.id,
            parts,
            component.transform.then(&transform),
            depth + 1,
            bambu,
            default_color,
            positions,
            triangles,
            tri_colors,
            warnings,
        );
    }
    if !obj.triangles.is_empty() {
        let base = positions.len() as u32;
        for v in &obj.vertices {
            let p = transform.apply(*v);
            positions.push([
                p[0] * part.unit,
                p[1] * part.unit,
                p[2] * part.unit,
            ]);
        }
        for t in &obj.triangles {
            if t.v.iter().all(|&i| (i as usize) < obj.vertices.len()) {
                triangles.push([t.v[0] + base, t.v[1] + base, t.v[2] + base]);
                let standard = t
                    .pid
                    .or(obj.pid)
                    .and_then(|gid| part.palettes.get(&gid))
                    .and_then(|palette| palette.get(t.p1.unwrap_or(obj.pindex)))
                    .copied();
                let painted = t
                    .paint
                    .and_then(|index| part.paint_codes.get(index as usize))
                    .and_then(|code| decode_paint_state(code))
                    .and_then(|state| {
                        if state == 0 {
                            default_color
                        } else {
                            bambu.palette.get(state - 1).copied()
                        }
                    });
                tri_colors.push(painted.or(standard).or(default_color));
            }
        }
    }
}

/// Décode l'état de couleur (extruder 1-based, 0 = défaut) d'un code
/// `paint_color` Bambu Studio / Orca Slicer.
///
/// Le code est un flux de quartets lu de droite à gauche. Pour un triangle
/// subdivisé, on retient la première feuille (approximation du rendu).
fn decode_paint_state(code: &str) -> Option<usize> {
    let mut nibbles: Vec<u8> = Vec::with_capacity(code.len());
    for ch in code.trim().chars().rev() {
        nibbles.push(ch.to_digit(16)? as u8);
    }
    if nibbles.is_empty() {
        return None;
    }
    let mut pos = 0usize;
    decode_paint_node(&nibbles, &mut pos)
}

fn decode_paint_node(nibbles: &[u8], pos: &mut usize) -> Option<usize> {
    let token = *nibbles.get(*pos)?;
    *pos += 1;
    let split_sides = (token & 0b11) as usize;
    if split_sides == 0 {
        let state = (token >> 2) as usize;
        if state < 3 {
            return Some(state);
        }
        let z = *nibbles.get(*pos)? as usize;
        *pos += 1;
        if z < 14 {
            return Some(3 + z);
        }
        if z == 14 {
            let low = *nibbles.get(*pos)? as usize;
            *pos += 1;
            let high = *nibbles.get(*pos)? as usize;
            *pos += 1;
            return Some(17 + (high << 4) + low);
        }
        return None;
    }
    // Nœud subdivisé : les enfants suivent. On garde la première couleur.
    let mut first = None;
    for _ in 0..(split_sides + 1) {
        let state = decode_paint_node(nibbles, pos)?;
        if first.is_none() {
            first = Some(state);
        }
    }
    first
}

/// Lit la palette `filament_colour` et les extruders par défaut des
/// métadonnées Bambu Studio (`Metadata/project_settings.config` et
/// `Metadata/model_settings.config`).
fn read_bambu_colors(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    names: &[String],
) -> BambuColors {
    let mut result = BambuColors::default();

    if let Some(name) = resolve_archive_name(names, "Metadata/project_settings.config") {
        if let Ok(text) = read_part(archive, &name) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(list) = json.get("filament_colour").and_then(|v| v.as_array()) {
                    result.palette = list
                        .iter()
                        .filter_map(|v| v.as_str())
                        .filter_map(parse_color)
                        .collect();
                }
            }
        }
    }

    if let Some(name) = resolve_archive_name(names, "Metadata/model_settings.config") {
        if let Ok(text) = read_part(archive, &name) {
            result.default_extruders = parse_default_extruders(&text);
        }
    }

    result
}

/// Extrait l'extruder par défaut (0-based) de chaque objet de
/// `model_settings.config`.
fn parse_default_extruders(xml: &str) -> HashMap<usize, usize> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut map = HashMap::new();
    let mut current_object: Option<usize> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                match e.local_name().as_ref() {
                    b"object" => {
                        current_object = attr(&e, b"id").and_then(|s| s.parse::<usize>().ok());
                    }
                    b"metadata" => {
                        let key = attr(&e, b"key").unwrap_or_default();
                        if key.eq_ignore_ascii_case("extruder") {
                            if let Some(object) = current_object {
                                if let Some(index) = attr(&e, b"value")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                                    .and_then(|n| n.checked_sub(1))
                                {
                                    map.entry(object).or_insert(index);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"object" {
                    current_object = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Analyse une couleur 3MF (`#RRGGBB` ou `#RRGGBBAA`).
fn parse_color(value: &str) -> Option<[u8; 3]> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() < 6 || !hex.is_char_boundary(6) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r, g, b])
}

pub fn write_3mf(meshes: &[(String, &Mesh)], path: &Path) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("Impossible de créer {}: {e}", path.display()))?;
    let mut zip = ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let content_types = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>"#;

    let rels = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;

    zip.start_file("[Content_Types].xml", stored)
        .map_err(|e| format!("Écriture 3MF impossible : {e}"))?;
    zip.write_all(content_types.as_bytes()).map_err(zip_err)?;
    zip.start_file("_rels/.rels", stored)
        .map_err(|e| format!("Écriture 3MF impossible : {e}"))?;
    zip.write_all(rels.as_bytes()).map_err(zip_err)?;
    zip.start_file(MODEL_PART, stored)
        .map_err(|e| format!("Écriture 3MF impossible : {e}"))?;

    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str(r#"<model unit="millimeter" xml:lang="fr-FR" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02">"#);
    xml.push('\n');
    xml.push_str("<metadata name=\"Application\">Polyforma</metadata>\n<resources>\n");

    // Un groupe `basematerials` par pièce colorée, avec les identifiants
    // n+1..2n pour ne pas entrer en collision avec les objets 1..n.
    let mut mesh_groups: Vec<Option<(usize, HashMap<[u8; 3], usize>)>> =
        Vec::with_capacity(meshes.len());
    let mut group_defs: Vec<(usize, Vec<[u8; 3]>)> = Vec::new();
    for (i, (_, mesh)) in meshes.iter().enumerate() {
        if mesh.has_colors() {
            let gid = meshes.len() + i + 1;
            let mut unique: Vec<[u8; 3]> = Vec::new();
            let mut index: HashMap<[u8; 3], usize> = HashMap::new();
            for &color in &mesh.tri_colors {
                if !index.contains_key(&color) {
                    index.insert(color, unique.len());
                    unique.push(color);
                }
            }
            mesh_groups.push(Some((gid, index)));
            group_defs.push((gid, unique));
        } else {
            mesh_groups.push(None);
        }
    }

    for (gid, colors) in &group_defs {
        xml.push_str(&format!("  <basematerials id=\"{gid}\">\n"));
        for (j, c) in colors.iter().enumerate() {
            xml.push_str(&format!(
                "    <base name=\"C{}\" displaycolor=\"#{:02X}{:02X}{:02X}\"/>\n",
                j + 1,
                c[0],
                c[1],
                c[2]
            ));
        }
        xml.push_str("  </basematerials>\n");
    }

    for (i, (name, mesh)) in meshes.iter().enumerate() {
        let oid = i + 1;
        xml.push_str(&format!(
            "  <object id=\"{oid}\" type=\"model\" name=\"{}\">\n    <mesh>\n      <vertices>\n",
            escape_xml(name)
        ));
        for p in &mesh.positions {
            xml.push_str(&format!(
                "        <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>\n",
                fnum(p[0]),
                fnum(p[1]),
                fnum(p[2])
            ));
        }
        xml.push_str("      </vertices>\n      <triangles>\n");
        let group = mesh_groups.get(i).and_then(|g| g.as_ref());
        for (ti, t) in mesh.iter_triangles().enumerate() {
            if let Some((gid, index)) = group {
                let color_index = index.get(&mesh.tri_color(ti)).copied().unwrap_or(0);
                xml.push_str(&format!(
                    "        <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\" pid=\"{gid}\" p1=\"{color_index}\"/>\n",
                    t[0], t[1], t[2]
                ));
            } else {
                xml.push_str(&format!(
                    "        <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>\n",
                    t[0], t[1], t[2]
                ));
            }
        }
        xml.push_str("      </triangles>\n    </mesh>\n  </object>\n");
    }
    xml.push_str("</resources>\n<build>\n");
    for i in 0..meshes.len() {
        xml.push_str(&format!("  <item objectid=\"{}\"/>\n", i + 1));
    }
    xml.push_str("</build>\n</model>\n");
    zip.write_all(xml.as_bytes()).map_err(zip_err)?;
    zip.finish().map_err(|e| format!("Finalisation 3MF impossible : {e}"))?;
    Ok(())
}

fn attr(e: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.local_name().as_ref() == name {
            return a.unescape_value().ok().map(|v| v.into_owned());
        }
    }
    None
}

fn fnum(v: f32) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn zip_err(e: std::io::Error) -> String {
    format!("Erreur d'écriture 3MF : {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored_cube() -> Mesh {
        let s = 10.0f32;
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
        let mut soup = Vec::new();
        for f in faces {
            soup.extend_from_slice(&f);
        }
        let mut mesh = Mesh::from_soup(&soup);
        let colors = (0..mesh.triangle_count())
            .map(|i| if i % 2 == 0 { [255, 0, 0] } else { [0, 255, 0] })
            .collect::<Vec<_>>();
        mesh.set_tri_colors(colors);
        mesh
    }

    #[test]
    fn color_roundtrip() {
        let mesh = colored_cube();
        let path = std::env::temp_dir().join(format!(
            "polyforma-color-test-{}.3mf",
            std::process::id()
        ));
        write_3mf(&[("Cube".to_string(), &mesh)], &path).unwrap();
        let loaded = load_3mf(&std::fs::read(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(loaded.mesh.has_colors());
        let red = loaded
            .mesh
            .tri_colors
            .iter()
            .filter(|c| **c == [255, 0, 0])
            .count();
        let green = loaded
            .mesh
            .tri_colors
            .iter()
            .filter(|c| **c == [0, 255, 0])
            .count();
        assert_eq!(red, 6);
        assert_eq!(green, 6);
    }

    #[test]
    fn reads_material_colorgroup() {
        let xml = r##"<?xml version="1.0"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02">
  <resources>
    <m:colorgroup id="7">
      <m:color color="#112233FF"/>
      <m:color color="#445566FF"/>
    </m:colorgroup>
    <object id="1">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="10" y="0" z="0"/>
          <vertex x="0" y="10" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2" pid="7" p1="1"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build><item objectid="1"/></build>
</model>"##;
        let parsed = parse_model(xml).unwrap();
        assert_eq!(parsed.palettes.get(&7).unwrap()[1], [0x44, 0x55, 0x66]);
    }

    #[test]
    fn reads_external_object_parts() {
        let root = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" requiredextensions="p">
  <resources>
    <object id="2" type="model">
      <components>
        <component p:path="/3D/Objects/object_1.model" objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/>
      </components>
    </object>
  </resources>
  <build>
    <item objectid="2" transform="1 0 0 0 1 0 0 0 1 1 2 3"/>
  </build>
</model>"##;
        let child = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="10" y="0" z="0"/>
          <vertex x="0" y="10" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build/>
</model>"##;
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("3D/3dmodel.model", options).unwrap();
        zip.write_all(root.as_bytes()).unwrap();
        zip.start_file("3D/Objects/object_1.model", options)
            .unwrap();
        zip.write_all(child.as_bytes()).unwrap();
        let bytes = zip.finish().unwrap().into_inner();

        let loaded = load_3mf(&bytes).unwrap();
        assert_eq!(loaded.mesh.triangle_count(), 1);
        let bounds = loaded.mesh.bounds();
        assert!((bounds.min[0] - 6.0).abs() < 1e-4, "{:?}", bounds.min);
        assert!((bounds.min[1] - 2.0).abs() < 1e-4, "{:?}", bounds.min);
        assert!((bounds.min[2] - 3.0).abs() < 1e-4, "{:?}", bounds.min);
    }

    #[test]
    fn reads_bambu_paint_colors() {
        let root = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" requiredextensions="p">
  <resources>
    <object id="2" type="model">
      <components>
        <component p:path="/3D/Objects/object_1.model" objectid="1"/>
      </components>
    </object>
  </resources>
  <build>
    <item objectid="2"/>
  </build>
</model>"##;
        let child = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/><vertex x="10" y="0" z="0"/><vertex x="0" y="10" z="0"/>
          <vertex x="20" y="0" z="0"/><vertex x="30" y="0" z="0"/><vertex x="20" y="10" z="0"/>
          <vertex x="40" y="0" z="0"/><vertex x="50" y="0" z="0"/><vertex x="40" y="10" z="0"/>
          <vertex x="60" y="0" z="0"/><vertex x="70" y="0" z="0"/><vertex x="60" y="10" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2" paint_color="8"/>
          <triangle v1="3" v2="4" v3="5" paint_color="0C"/>
          <triangle v1="6" v2="7" v3="8" paint_color="1C"/>
          <triangle v1="9" v2="10" v3="11"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build/>
</model>"##;
        let project = r##"{
  "filament_colour": ["#FF0000", "#FFFFFF", "#000000", "#FFFF00"]
}"##;
        let settings = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <object id="2">
    <metadata key="name" value="Delibird.stl"/>
    <metadata key="extruder" value="1"/>
    <part id="1" subtype="normal_part">
      <metadata key="name" value="Delibird.stl"/>
    </part>
  </object>
</config>"#;
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("3D/3dmodel.model", options).unwrap();
        zip.write_all(root.as_bytes()).unwrap();
        zip.start_file("3D/Objects/object_1.model", options)
            .unwrap();
        zip.write_all(child.as_bytes()).unwrap();
        zip.start_file("Metadata/project_settings.config", options)
            .unwrap();
        zip.write_all(project.as_bytes()).unwrap();
        zip.start_file("Metadata/model_settings.config", options)
            .unwrap();
        zip.write_all(settings.as_bytes()).unwrap();
        let bytes = zip.finish().unwrap().into_inner();

        let loaded = load_3mf(&bytes).unwrap();
        assert_eq!(loaded.mesh.triangle_count(), 4);
        assert!(loaded.mesh.has_colors());
        let colors = &loaded.mesh.tri_colors;
        assert!(colors.contains(&[0xFF, 0xFF, 0xFF]), "{colors:?}");
        assert!(colors.contains(&[0x00, 0x00, 0x00]), "{colors:?}");
        assert!(colors.contains(&[0xFF, 0xFF, 0x00]), "{colors:?}");
        assert!(colors.contains(&[0xFF, 0x00, 0x00]), "{colors:?}");
    }
}
