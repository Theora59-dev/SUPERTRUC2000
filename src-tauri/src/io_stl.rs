//! Lecture / écriture STL binaire et ASCII.

use crate::mesh::Mesh;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

pub fn load_stl(bytes: &[u8]) -> Result<Mesh, String> {
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if bytes.len() == 84 + count * 50 {
            return load_binary(bytes, count);
        }
    }
    load_ascii(bytes)
}

fn load_binary(bytes: &[u8], count: usize) -> Result<Mesh, String> {
    let mut soup: Vec<[f32; 3]> = Vec::with_capacity(count * 3);
    let mut off = 84;
    for _ in 0..count {
        if off + 50 > bytes.len() {
            return Err("Fichier STL binaire tronqué.".into());
        }
        off += 12; // normale, recalculée plus tard
        for _ in 0..3 {
            let x = f32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            let y = f32::from_le_bytes([bytes[off + 4], bytes[off + 5], bytes[off + 6], bytes[off + 7]]);
            let z = f32::from_le_bytes([bytes[off + 8], bytes[off + 9], bytes[off + 10], bytes[off + 11]]);
            soup.push([x, y, z]);
            off += 12;
        }
        off += 2; // attribut
    }
    Ok(Mesh::from_soup(&soup))
}

fn load_ascii(bytes: &[u8]) -> Result<Mesh, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut soup: Vec<[f32; 3]> = Vec::new();
    let mut pending: Vec<[f32; 3]> = Vec::with_capacity(3);
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("vertex ") {
            let nums: Vec<f32> = rest
                .split_whitespace()
                .filter_map(|v| v.parse::<f32>().ok())
                .collect();
            if nums.len() >= 3 {
                pending.push([nums[0], nums[1], nums[2]]);
                if pending.len() == 3 {
                    soup.extend_from_slice(&pending);
                    pending.clear();
                }
            }
        }
    }
    if soup.is_empty() {
        return Err(
            "Impossible de lire ce STL : format non reconnu ou fichier vide.".into(),
        );
    }
    Ok(Mesh::from_soup(&soup))
}

pub fn write_stl_binary(mesh: &Mesh, path: &Path) -> Result<(), String> {
    let tri_count = mesh.triangle_count();
    let file = File::create(path).map_err(|e| format!("Impossible de créer {}: {e}", path.display()))?;
    let mut w = BufWriter::new(file);
    let mut header = [0u8; 80];
    let tag = b"SUPERTRUC2000 binary STL";
    header[..tag.len()].copy_from_slice(tag);
    w.write_all(&header).map_err(io_err)?;
    w.write_all(&(tri_count as u32).to_le_bytes()).map_err(io_err)?;
    let mut buf = [0u8; 50];
    for t in mesh.iter_triangles() {
        let a = mesh.pos(t[0]);
        let b = mesh.pos(t[1]);
        let c = mesh.pos(t[2]);
        let mut n = (b - a).cross(c - a).normalize_or_zero();
        if !n.is_finite() {
            n = glam::DVec3::ZERO;
        }
        buf[0..4].copy_from_slice(&(n.x as f32).to_le_bytes());
        buf[4..8].copy_from_slice(&(n.y as f32).to_le_bytes());
        buf[8..12].copy_from_slice(&(n.z as f32).to_le_bytes());
        let mut off = 12;
        for p in [a, b, c] {
            buf[off..off + 4].copy_from_slice(&(p.x as f32).to_le_bytes());
            buf[off + 4..off + 8].copy_from_slice(&(p.y as f32).to_le_bytes());
            buf[off + 8..off + 12].copy_from_slice(&(p.z as f32).to_le_bytes());
            off += 12;
        }
        buf[48] = 0;
        buf[49] = 0;
        w.write_all(&buf).map_err(io_err)?;
    }
    w.flush().map_err(io_err)?;
    Ok(())
}

pub fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut f = File::open(path).map_err(|e| format!("Impossible d'ouvrir {}: {e}", path.display()))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(io_err)?;
    Ok(buf)
}

fn io_err(e: std::io::Error) -> String {
    format!("Erreur d'écriture : {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_binary() {
        let mut soup = Vec::new();
        soup.extend_from_slice(&[[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        let mesh = Mesh::from_soup(&soup);
        let dir = std::env::temp_dir().join("supertruc2000-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tri.stl");
        write_stl_binary(&mesh, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let loaded = load_stl(&bytes).unwrap();
        assert_eq!(loaded.triangle_count(), 1);
        assert_eq!(loaded.positions.len(), 3);
    }
}
