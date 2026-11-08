//! Lecture / écriture Wavefront OBJ (triangulation des polygones).

use crate::mesh::Mesh;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub fn load_obj(bytes: &[u8]) -> Result<Mesh, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let nums: Vec<f32> = it.take(3).filter_map(|v| v.parse::<f32>().ok()).collect();
                if nums.len() == 3 {
                    vertices.push([nums[0], nums[1], nums[2]]);
                }
            }
            Some("f") => {
                let mut idx: Vec<u32> = Vec::new();
                for tok in it {
                    let first = tok.split('/').next().unwrap_or("");
                    if let Ok(n) = first.parse::<i64>() {
                        if n > 0 {
                            idx.push((n - 1) as u32);
                        } else if n < 0 {
                            let abs = (-n) as usize;
                            if abs <= vertices.len() {
                                idx.push((vertices.len() - abs) as u32);
                            }
                        }
                    }
                }
                if idx.len() >= 3 {
                    for i in 1..idx.len() - 1 {
                        faces.push([idx[0], idx[i], idx[i + 1]]);
                    }
                }
            }
            _ => {}
        }
    }
    if faces.is_empty() {
        return Err("Aucun triangle trouvé dans ce fichier OBJ.".into());
    }
    let max_idx = faces.iter().flatten().copied().max().unwrap_or(0) as usize;
    if max_idx >= vertices.len() {
        return Err("Fichier OBJ invalide : indice de sommet hors limites.".into());
    }
    let mut indices = Vec::with_capacity(faces.len() * 3);
    for f in faces {
        indices.extend_from_slice(&f);
    }
    let mut mesh = Mesh::from_indexed(vertices, indices);
    mesh = mesh.welded();
    mesh.cleanup();
    Ok(mesh)
}

pub fn write_obj(meshes: &[(String, &Mesh)], path: &Path) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("Impossible de créer {}: {e}", path.display()))?;
    let mut w = BufWriter::new(file);
    writeln!(w, "# Export Polyforma").map_err(err)?;
    let mut base: u32 = 1;
    for (name, mesh) in meshes {
        writeln!(w, "o {}", sanitize(name)).map_err(err)?;
        for p in &mesh.positions {
            writeln!(w, "v {} {} {}", fmt(p[0]), fmt(p[1]), fmt(p[2])).map_err(err)?;
        }
        for t in mesh.iter_triangles() {
            writeln!(
                w,
                "f {} {} {}",
                t[0] + base,
                t[1] + base,
                t[2] + base
            )
            .map_err(err)?;
        }
        base += mesh.positions.len() as u32;
    }
    w.flush().map_err(err)?;
    Ok(())
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect()
}

fn fmt(v: f32) -> String {
    let s = format!("{:.4}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn err(e: std::io::Error) -> String {
    format!("Erreur d'écriture OBJ : {e}")
}
