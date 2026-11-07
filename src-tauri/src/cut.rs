//! Découpe d'un maillage par un plan, avec bouchons étanches et connecteurs
//! (tenons cylindriques d'un côté, logements de l'autre).

use crate::mesh::{Mesh, DEFAULT_COLOR};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const MIN_CAP_AREA: f64 = 1e-7;

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub normal: DVec3,
    pub d: f64,
    pub origin: DVec3,
    pub u: DVec3,
    pub v: DVec3,
}

impl Plane {
    pub fn new(normal: DVec3, point: DVec3) -> Result<Self, String> {
        let len = normal.length();
        if len < 1e-12 {
            return Err("Normale de plan invalide.".into());
        }
        let n = normal / len;
        let helper = if n.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
        let u = n.cross(helper).normalize();
        let v = n.cross(u).normalize();
        let d = n.dot(point);
        Ok(Self {
            normal: n,
            d,
            origin: n * d,
            u,
            v,
        })
    }

    pub fn signed(&self, p: DVec3) -> f64 {
        self.normal.dot(p) - self.d
    }

    pub fn to2d(&self, p: DVec3) -> DVec2 {
        let rel = p - self.origin;
        DVec2::new(rel.dot(self.u), rel.dot(self.v))
    }

    pub fn to3d(&self, p: DVec2, depth: f64) -> DVec3 {
        self.origin + self.u * p.x + self.v * p.y + self.normal * depth
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorOptions {
    pub enabled: bool,
    pub auto: bool,
    pub count: usize,
    pub radius: f64,
    pub depth: f64,
    pub clearance: f64,
}

impl Default for ConnectorOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            auto: true,
            count: 4,
            radius: 3.0,
            depth: 8.0,
            clearance: 0.2,
        }
    }
}

#[derive(Default)]
pub struct CutOptions {
    pub connectors: ConnectorOptions,
}

pub struct CutResult {
    pub negative: Mesh,
    pub positive: Mesh,
    pub warnings: Vec<String>,
    pub connector_count: usize,
    pub connector_radius: f64,
    pub cap_area: f64,
}

#[derive(Default)]
struct Side {
    tris: Vec<[u32; 3]>,
    colors: Vec<[u8; 3]>,
    cap_edges: HashSet<(u32, u32)>,
}

impl Side {
    fn push_tri(&mut self, tri: [u32; 3], color: [u8; 3]) {
        self.tris.push(tri);
        self.colors.push(color);
    }

    fn add_cap_edge(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        self.cap_edges.insert(key);
    }
}

#[derive(Clone)]
struct CapLoop {
    ids: Vec<u32>,
    pts2: Vec<DVec2>,
    area2: f64,
    depth: usize,
}

impl CapLoop {
    fn abs_area(&self) -> f64 {
        self.area2.abs() * 0.5
    }
}

struct Region {
    outer: usize,
    holes: Vec<usize>,
}

pub fn cut_mesh(mesh: &Mesh, plane: &Plane, opts: &CutOptions) -> Result<CutResult, String> {
    if mesh.is_empty() {
        return Err("La pièce est vide.".into());
    }
    let diag = {
        let b = mesh.bounds();
        DVec3::from_array(b.size).length().max(1.0)
    };
    let eps = (diag * 1e-7).max(1e-6);

    let mut pos: Vec<DVec3> = mesh
        .positions
        .iter()
        .map(|p| DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
        .collect();
    let mut cache: HashMap<(u32, u32), u32> = HashMap::new();
    let colored = mesh.has_colors();
    let fallback = dominant_color(mesh);

    let mut neg = Side::default();
    let mut posi = Side::default();

    for (tri_index, tri) in mesh.iter_triangles().enumerate() {
        let color = mesh.tri_color(tri_index);
        let d = [
            plane.signed(pos[tri[0] as usize]),
            plane.signed(pos[tri[1] as usize]),
            plane.signed(pos[tri[2] as usize]),
        ];
        let dmin = d[0].min(d[1]).min(d[2]);
        let dmax = d[0].max(d[1]).max(d[2]);

        if dmin > eps {
            posi.push_tri(tri, color);
            continue;
        }
        if dmax < -eps {
            neg.push_tri(tri, color);
            continue;
        }
        if dmin.abs() <= eps && dmax.abs() <= eps {
            // Coplanaire au plan : reconstruit par le bouchon.
            continue;
        }

        let poly_pos = clip_polygon(&tri, &d, true, &mut pos, plane, &mut cache, eps);
        add_polygon(&mut posi, &poly_pos, &pos, plane, eps, diag, color);
        let poly_neg = clip_polygon(&tri, &d, false, &mut pos, plane, &mut cache, eps);
        add_polygon(&mut neg, &poly_neg, &pos, plane, eps, diag, color);
    }

    if posi.tris.is_empty() || neg.tris.is_empty() {
        return Err("Le plan ne traverse pas la pièce : une des deux moitiés est vide.".into());
    }

    let mut warnings = Vec::new();

    // Bouchons : normale +n côté négatif, -n côté positif.
    let mut neg_loops = build_cap_loops(&neg.cap_edges, &pos, plane, 1.0);
    let mut pos_loops = build_cap_loops(&posi.cap_edges, &pos, plane, -1.0);
    if neg_loops.is_empty() {
        warnings.push(
            "Aucune boucle de découpe détectée côté inférieur (maillage ouvert ?).".into(),
        );
    }
    if pos_loops.is_empty() {
        warnings.push(
            "Aucune boucle de découpe détectée côté supérieur (maillage ouvert ?).".into(),
        );
    }

    let neg_regions = nest_loops(&mut neg_loops);
    let pos_regions = nest_loops(&mut pos_loops);

    let cap_area: f64 = pos_regions
        .iter()
        .map(|r| region_area(&pos_loops, r))
        .sum::<f64>()
        .max(0.0);

    let mut connector_count = 0usize;
    let mut connector_radius = 0.0f64;
    let mut extra_holes: Vec<(usize, Vec<u32>)> = Vec::new();
    let mut used_connector_options: Option<(f64, f64, f64)> = None; // radius, depth, clearance

    if opts.connectors.enabled && !pos_regions.is_empty() {
        let (mut radius, mut count) = if opts.connectors.auto {
            auto_connector_params(cap_area)
        } else {
            (
                opts.connectors.radius.clamp(0.5, 50.0),
                opts.connectors.count.clamp(1, 32),
            )
        };

        let mut points = sample_points(&pos_loops, &pos_regions, radius, count);
        if points.is_empty() && opts.connectors.auto && radius > 1.0 {
            radius = (radius * 0.6).max(1.0);
            count = count.min(4);
            points = sample_points(&pos_loops, &pos_regions, radius, count);
        }

        if points.is_empty() {
            warnings.push(
                "Connecteurs ignorés : la section de découpe est trop étroite pour la taille demandée."
                    .into(),
            );
        } else {
            connector_count = points.len();
            connector_radius = radius;
            let clearance = opts.connectors.clearance.clamp(0.0, 1.0);
            let depth = opts.connectors.depth.clamp(1.0, 100.0);
            let hole_radius = radius + clearance;
            let hole_segments = circle_segments(hole_radius);
            used_connector_options = Some((radius, depth, clearance));

            for p in &points {
                let region = region_at(*p, &pos_loops, &pos_regions).unwrap_or(0);
                let ring = add_circle(&mut pos, plane, *p, hole_radius, 0.0, hole_segments);
                extra_holes.push((region, ring));
            }

            // Tenons sur la pièce négative (ils dépassent vers +n).
            let peg_segments = circle_segments(radius);
            for p in &points {
                let bottom = add_circle(&mut pos, plane, *p, radius, -1.0, peg_segments);
                let top = add_circle(&mut pos, plane, *p, radius, depth, peg_segments);
                add_peg(&mut neg.tris, &pos, plane, *p, bottom, top);
            }
        }
    }

    let mut neg_tris = neg.tris;
    let mut neg_colors = neg.colors;
    let mut pos_tris = posi.tris;
    let mut pos_colors = posi.colors;

    if !neg_loops.is_empty() {
        let mut buf = Vec::new();
        triangulate_cap(
            &neg_loops,
            &neg_regions,
            &[],
            &pos,
            plane,
            plane.normal,
            &mut buf,
            &mut warnings,
        );
        neg_tris.extend(buf);
    }

    if !pos_loops.is_empty() {
        let mut buf = Vec::new();
        triangulate_cap(
            &pos_loops,
            &pos_regions,
            &extra_holes,
            &pos,
            plane,
            -plane.normal,
            &mut buf,
            &mut warnings,
        );
        pos_tris.extend(buf);
    }

    // Murs et fonds des logements.
    if let Some((radius, depth, clearance)) = used_connector_options {
        let hole_radius = radius + clearance;
        let hole_depth = depth + 0.4;
        for (_, top_ring) in &extra_holes {
            let center = ring_center(top_ring, &pos, plane);
            let bottom = add_circle(
                &mut pos,
                plane,
                center,
                hole_radius,
                hole_depth,
                top_ring.len(),
            );
            add_hole_walls(&mut pos_tris, &pos, plane, top_ring, &bottom, center);
        }
    }

    // Les triangles ajoutés (bouchons, tenons, logements) reprennent la
    // couleur dominante du modèle.
    if colored {
        if neg_colors.len() < neg_tris.len() {
            neg_colors.resize(neg_tris.len(), fallback);
        }
        if pos_colors.len() < pos_tris.len() {
            pos_colors.resize(pos_tris.len(), fallback);
        }
    }

    let negative = build_mesh(&neg_tris, &neg_colors, &pos, colored);
    let positive = build_mesh(&pos_tris, &pos_colors, &pos, colored);

    if negative.is_empty() || positive.is_empty() {
        return Err("La découpe n'a produit aucune pièce exploitable.".into());
    }

    Ok(CutResult {
        negative,
        positive,
        warnings,
        connector_count,
        connector_radius,
        cap_area,
    })
}

fn auto_connector_params(area: f64) -> (f64, usize) {
    let radius = (area.max(1.0).sqrt() / 28.0).clamp(1.5, 5.0);
    let count = ((area / 1400.0).round() as usize).clamp(1, 8);
    (radius, count)
}

fn clip_polygon(
    tri: &[u32; 3],
    d: &[f64; 3],
    keep_positive: bool,
    pos: &mut Vec<DVec3>,
    plane: &Plane,
    cache: &mut HashMap<(u32, u32), u32>,
    eps: f64,
) -> Vec<u32> {
    let inside = |v: f64| {
        if keep_positive {
            v >= -eps
        } else {
            v <= eps
        }
    };
    let mut out: Vec<u32> = Vec::with_capacity(4);
    for i in 0..3 {
        let j = (i + 1) % 3;
        let ca = tri[i];
        let cb = tri[j];
        let da = d[i];
        let db = d[j];
        let ai = inside(da);
        let bi = inside(db);
        if ai {
            out.push(ca);
        }
        if ai != bi {
            let id = if da.abs() <= eps {
                ca
            } else if db.abs() <= eps {
                cb
            } else {
                intersection(ca, cb, pos, plane, cache)
            };
            out.push(id);
        }
    }
    dedupe_polygon(out)
}

fn dedupe_polygon(mut poly: Vec<u32>) -> Vec<u32> {
    poly.dedup();
    if poly.len() > 1 && poly.first() == poly.last() {
        poly.pop();
    }
    poly
}

fn intersection(
    a: u32,
    b: u32,
    pos: &mut Vec<DVec3>,
    plane: &Plane,
    cache: &mut HashMap<(u32, u32), u32>,
) -> u32 {
    let key = if a < b { (a, b) } else { (b, a) };
    if let Some(&id) = cache.get(&key) {
        return id;
    }
    let pa = pos[a as usize];
    let pb = pos[b as usize];
    let da = plane.signed(pa);
    let db = plane.signed(pb);
    let denom = da - db;
    let t = if denom.abs() < 1e-30 { 0.5 } else { da / denom };
    let mut p = pa + (pb - pa) * t;
    p -= plane.normal * plane.signed(p);
    let id = pos.len() as u32;
    pos.push(p);
    cache.insert(key, id);
    id
}

fn add_polygon(
    side: &mut Side,
    poly: &[u32],
    pos: &[DVec3],
    plane: &Plane,
    eps: f64,
    diag: f64,
    color: [u8; 3],
) {
    if poly.len() < 3 {
        return;
    }
    let area = polygon_area(poly, pos);
    if area <= (diag * diag * 1e-12).max(MIN_CAP_AREA) {
        return;
    }
    for i in 1..poly.len() - 1 {
        side.push_tri([poly[0], poly[i], poly[i + 1]], color);
    }
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        if a != b
            && plane.signed(pos[a as usize]).abs() <= eps
            && plane.signed(pos[b as usize]).abs() <= eps
        {
            side.add_cap_edge(a, b);
        }
    }
}

fn polygon_area(poly: &[u32], pos: &[DVec3]) -> f64 {
    if poly.len() < 3 {
        return 0.0;
    }
    let mut sum = DVec3::ZERO;
    for i in 1..poly.len() - 1 {
        let a = pos[poly[0] as usize];
        let b = pos[poly[i] as usize];
        let c = pos[poly[i + 1] as usize];
        sum += (b - a).cross(c - a);
    }
    sum.length() * 0.5
}

fn build_cap_loops(
    edges: &HashSet<(u32, u32)>,
    pos: &[DVec3],
    plane: &Plane,
    desired_sign: f64,
) -> Vec<CapLoop> {
    if edges.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<(u32, u32)> = edges.iter().copied().collect();
    sorted.sort_unstable();
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(a, b) in &sorted {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    let mut used: HashSet<(u32, u32)> = HashSet::with_capacity(sorted.len());
    let mut loops = Vec::new();

    for &(start_a, start_b) in &sorted {
        let key0 = (start_a.min(start_b), start_a.max(start_b));
        if used.contains(&key0) {
            continue;
        }
        let mut ids = vec![start_a];
        let mut prev = start_a;
        let mut cur = start_b;
        used.insert(key0);
        let mut closed = false;
        for _ in 0..=sorted.len() {
            if cur == start_a {
                closed = true;
                break;
            }
            ids.push(cur);
            let neighbors = match adj.get(&cur) {
                Some(n) => n,
                None => break,
            };
            let mut next = None;
            for &n in neighbors {
                if n == prev {
                    continue;
                }
                let k = (cur.min(n), cur.max(n));
                if !used.contains(&k) {
                    next = Some(n);
                    break;
                }
            }
            if next.is_none() {
                for &n in neighbors {
                    let k = (cur.min(n), cur.max(n));
                    if !used.contains(&k) {
                        next = Some(n);
                        break;
                    }
                }
            }
            let Some(n) = next else { break };
            let k = (cur.min(n), cur.max(n));
            used.insert(k);
            prev = cur;
            cur = n;
        }
        if !closed || ids.len() < 3 {
            continue;
        }
        ids.dedup();
        if ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        if ids.len() < 3 {
            continue;
        }
        let mut pts2: Vec<DVec2> = ids
            .iter()
            .map(|&id| plane.to2d(pos[id as usize]))
            .collect();
        let mut area2 = signed_area2(&pts2);
        if area2.abs() < 1e-12 {
            continue;
        }
        if area2 * desired_sign < 0.0 {
            ids.reverse();
            pts2.reverse();
            area2 = -area2;
        }
        loops.push(CapLoop {
            ids,
            pts2,
            area2,
            depth: 0,
        });
    }
    loops
}

fn signed_area2(pts: &[DVec2]) -> f64 {
    let mut a = 0.0;
    let n = pts.len();
    for i in 0..n {
        let p = pts[i];
        let q = pts[(i + 1) % n];
        a += p.x * q.y - q.x * p.y;
    }
    a
}

fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let a = poly[i];
        let b = poly[j];
        if (a.y > p.y) != (b.y > p.y) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn dist_point_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let denom = ab.length_squared();
    if denom < 1e-20 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / denom).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

fn dist_to_polygon(p: DVec2, poly: &[DVec2]) -> f64 {
    let mut d = f64::INFINITY;
    let n = poly.len();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        d = d.min(dist_point_segment(p, a, b));
    }
    d
}

fn region_area(loops: &[CapLoop], region: &Region) -> f64 {
    let mut a = loops[region.outer].abs_area();
    for &h in &region.holes {
        a -= loops[h].abs_area();
    }
    a.max(0.0)
}

fn nest_loops(loops: &mut [CapLoop]) -> Vec<Region> {
    for i in 0..loops.len() {
        let p = loops[i].pts2[0];
        let ai = loops[i].abs_area();
        let mut depth = 0usize;
        for j in 0..loops.len() {
            if i == j {
                continue;
            }
            if loops[j].abs_area() > ai * (1.0 + 1e-9) && point_in_polygon(p, &loops[j].pts2) {
                depth += 1;
            }
        }
        loops[i].depth = depth;
    }
    let mut regions: Vec<Region> = Vec::new();
    for i in 0..loops.len() {
        if loops[i].depth % 2 == 0 {
            regions.push(Region {
                outer: i,
                holes: Vec::new(),
            });
        }
    }
    for i in 0..loops.len() {
        if loops[i].depth % 2 == 0 {
            continue;
        }
        let p = loops[i].pts2[0];
        let ai = loops[i].abs_area();
        let mut best: Option<usize> = None;
        let mut best_area = f64::INFINITY;
        for (ri, r) in regions.iter().enumerate() {
            let outer = &loops[r.outer];
            if outer.abs_area() > ai && point_in_polygon(p, &outer.pts2) {
                if outer.abs_area() < best_area {
                    best_area = outer.abs_area();
                    best = Some(ri);
                }
            }
        }
        if let Some(ri) = best {
            regions[ri].holes.push(i);
        }
    }
    regions
}

fn region_at(p: DVec2, loops: &[CapLoop], regions: &[Region]) -> Option<usize> {
    for (ri, r) in regions.iter().enumerate() {
        if !point_in_polygon(p, &loops[r.outer].pts2) {
            continue;
        }
        if r.holes
            .iter()
            .any(|&h| point_in_polygon(p, &loops[h].pts2))
        {
            continue;
        }
        return Some(ri);
    }
    None
}

fn sample_points(
    loops: &[CapLoop],
    regions: &[Region],
    radius: f64,
    count: usize,
) -> Vec<DVec2> {
    if count == 0 || regions.is_empty() {
        return Vec::new();
    }
    let margin = radius + 1.5;
    let mut candidates: Vec<DVec2> = Vec::new();

    for region in regions {
        if region_area(loops, region) < std::f64::consts::PI * radius * radius * 0.9 {
            continue;
        }
        let outer = &loops[region.outer];
        let mut min = DVec2::splat(f64::INFINITY);
        let mut max = DVec2::splat(f64::NEG_INFINITY);
        for p in &outer.pts2 {
            min = min.min(*p);
            max = max.max(*p);
        }
        let size = max - min;
        let mut spacing = (radius * 2.8).max(4.0);
        let mut nx = (size.x / spacing).floor() as i64 + 1;
        let mut ny = (size.y / spacing).floor() as i64 + 1;
        while (nx * ny) > 40_000 {
            spacing *= 1.4;
            nx = (size.x / spacing).floor() as i64 + 1;
            ny = (size.y / spacing).floor() as i64 + 1;
        }
        for iy in 0..ny {
            for ix in 0..nx {
                let p = DVec2::new(
                    min.x + spacing * (ix as f64 + 0.5),
                    min.y + spacing * (iy as f64 + 0.5),
                );
                if p.x < min.x || p.y < min.y || p.x > max.x || p.y > max.y {
                    continue;
                }
                if !point_in_polygon(p, &outer.pts2) {
                    continue;
                }
                if region
                    .holes
                    .iter()
                    .any(|&h| point_in_polygon(p, &loops[h].pts2))
                {
                    continue;
                }
                if dist_to_polygon(p, &outer.pts2) < margin {
                    continue;
                }
                if region
                    .holes
                    .iter()
                    .any(|&h| dist_to_polygon(p, &loops[h].pts2) < margin)
                {
                    continue;
                }
                candidates.push(p);
            }
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    let biggest = regions
        .iter()
        .max_by(|a, b| {
            region_area(loops, a)
                .partial_cmp(&region_area(loops, b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();
    let mut centroid = DVec2::ZERO;
    let outer = &loops[biggest.outer];
    for p in &outer.pts2 {
        centroid += *p;
    }
    centroid /= outer.pts2.len().max(1) as f64;
    let mut selected = vec![*candidates
        .iter()
        .min_by(|a, b| {
            (*a - centroid)
                .length_squared()
                .partial_cmp(&(*b - centroid).length_squared())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap()];

    let min_spacing = radius * 2.5;
    while selected.len() < count {
        let mut best: Option<(DVec2, f64)> = None;
        for &c in &candidates {
            if selected.iter().any(|s| (*s - c).length() < 1e-9) {
                continue;
            }
            let mut d = f64::INFINITY;
            for s in &selected {
                d = d.min((c - *s).length());
            }
            if best.map(|(_, bd)| d > bd).unwrap_or(true) {
                best = Some((c, d));
            }
        }
        match best {
            Some((p, d)) if d >= min_spacing => selected.push(p),
            _ => break,
        }
    }
    selected
}

fn circle_segments(radius: f64) -> usize {
    let target_chord = 0.8;
    ((2.0 * std::f64::consts::PI * radius / target_chord).ceil() as usize).clamp(16, 128)
}

fn add_circle(
    pos: &mut Vec<DVec3>,
    plane: &Plane,
    center: DVec2,
    radius: f64,
    depth: f64,
    segments: usize,
) -> Vec<u32> {
    let mut ids = Vec::with_capacity(segments);
    for i in 0..segments {
        let a = 2.0 * std::f64::consts::PI * i as f64 / segments as f64;
        let p = center + DVec2::new(a.cos(), a.sin()) * radius;
        let v = plane.to3d(p, depth);
        ids.push(pos.len() as u32);
        pos.push(v);
    }
    ids
}

fn ring_center(ring: &[u32], pos: &[DVec3], plane: &Plane) -> DVec2 {
    let mut c = DVec2::ZERO;
    for &id in ring {
        c += plane.to2d(pos[id as usize]);
    }
    c / ring.len().max(1) as f64
}

fn add_peg(
    out: &mut Vec<[u32; 3]>,
    pos: &[DVec3],
    plane: &Plane,
    center: DVec2,
    bottom: Vec<u32>,
    top: Vec<u32>,
) {
    let n = top.len().min(bottom.len());
    let axis = plane.to3d(center, 0.0);
    for i in 0..n {
        let j = (i + 1) % n;
        add_wall_quad(out, pos, plane, axis, top[i], top[j], bottom[j], bottom[i], false);
    }
    add_disk(out, pos, plane.normal, &top);
    add_disk(out, pos, -plane.normal, &bottom);
}

fn add_hole_walls(
    out: &mut Vec<[u32; 3]>,
    pos: &[DVec3],
    plane: &Plane,
    top: &[u32],
    bottom: &[u32],
    center: DVec2,
) {
    let n = top.len().min(bottom.len());
    let axis = plane.to3d(center, 0.0);
    for i in 0..n {
        let j = (i + 1) % n;
        add_wall_quad(out, pos, plane, axis, top[i], top[j], bottom[j], bottom[i], true);
    }
    add_disk(out, pos, -plane.normal, bottom);
}

fn add_wall_quad(
    out: &mut Vec<[u32; 3]>,
    pos: &[DVec3],
    plane: &Plane,
    axis: DVec3,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    inward: bool,
) {
    for tri in [[a, b, c], [a, c, d]] {
        let pa = pos[tri[0] as usize];
        let pb = pos[tri[1] as usize];
        let pc = pos[tri[2] as usize];
        let mut radial = (pa + pb + pc) / 3.0 - axis;
        radial -= plane.normal * radial.dot(plane.normal);
        if radial.length_squared() < 1e-20 {
            continue;
        }
        radial = radial.normalize();
        let desired = if inward { -radial } else { radial };
        push_oriented(out, pos, tri, desired);
    }
}

fn add_disk(out: &mut Vec<[u32; 3]>, pos: &[DVec3], normal: DVec3, ring: &[u32]) {
    if ring.len() < 3 {
        return;
    }
    let first = ring[0];
    let n = ring.len();
    for i in 1..n - 1 {
        push_oriented(out, pos, [first, ring[i], ring[i + 1]], normal);
    }
}

fn push_oriented(out: &mut Vec<[u32; 3]>, pos: &[DVec3], tri: [u32; 3], desired: DVec3) {
    let a = pos[tri[0] as usize];
    let b = pos[tri[1] as usize];
    let c = pos[tri[2] as usize];
    let n = (b - a).cross(c - a);
    if n.length_squared() < 1e-20 || desired.length_squared() < 1e-20 {
        return;
    }
    if n.dot(desired) < 0.0 {
        out.push([tri[0], tri[2], tri[1]]);
    } else {
        out.push(tri);
    }
}

fn triangulate_cap(
    loops: &[CapLoop],
    regions: &[Region],
    extra_holes: &[(usize, Vec<u32>)],
    pos: &[DVec3],
    plane: &Plane,
    cap_normal: DVec3,
    out: &mut Vec<[u32; 3]>,
    warnings: &mut Vec<String>,
) {
    for (ri, region) in regions.iter().enumerate() {
        let outer = &loops[region.outer];
        if outer.ids.len() < 3 {
            continue;
        }
        let mut coords: Vec<f64> = Vec::new();
        let mut ids: Vec<u32> = Vec::new();
        let mut hole_starts: Vec<usize> = Vec::new();

        for &id in &outer.ids {
            let p = plane.to2d(pos[id as usize]);
            coords.push(p.x);
            coords.push(p.y);
            ids.push(id);
        }
        let mut add_hole = |ring: &[u32], coords: &mut Vec<f64>, ids: &mut Vec<u32>| {
            if ring.len() < 3 {
                return;
            }
            hole_starts.push(ids.len());
            for &id in ring {
                let p = plane.to2d(pos[id as usize]);
                coords.push(p.x);
                coords.push(p.y);
                ids.push(id);
            }
        };
        for &h in &region.holes {
            add_hole(&loops[h].ids, &mut coords, &mut ids);
        }
        for (hole_region, ring) in extra_holes {
            if *hole_region == ri {
                add_hole(ring, &mut coords, &mut ids);
            }
        }

        match earcutr::earcut(&coords, &hole_starts, 2) {
            Ok(tris) if !tris.is_empty() => {
                for t in tris.chunks_exact(3) {
                    let tri = [ids[t[0]], ids[t[1]], ids[t[2]]];
                    push_oriented(out, pos, tri, cap_normal);
                }
            }
            _ => {
                warnings.push(
                    "Triangulation du bouchon impossible ; bouchon simplifié.".into(),
                );
                // Repli : éventail sur la boucle extérieure.
                let first = outer.ids[0];
                for i in 1..outer.ids.len() - 1 {
                    push_oriented(
                        out,
                        pos,
                        [first, outer.ids[i], outer.ids[i + 1]],
                        cap_normal,
                    );
                }
            }
        }
    }
}

fn build_mesh(tris: &[[u32; 3]], colors: &[[u8; 3]], pos: &[DVec3], colored: bool) -> Mesh {
    let positions: Vec<[f32; 3]> = pos
        .iter()
        .map(|p| [p.x as f32, p.y as f32, p.z as f32])
        .collect();
    let mut indices: Vec<u32> = Vec::with_capacity(tris.len() * 3);
    for t in tris {
        indices.extend_from_slice(t);
    }
    let mut mesh = Mesh::from_indexed(positions, indices);
    mesh.compact();
    if colored && colors.len() == tris.len() {
        mesh.set_tri_colors(colors.to_vec());
    }
    mesh
}

/// Couleur la plus fréquente du maillage (couleur par défaut si absente).
fn dominant_color(mesh: &Mesh) -> [u8; 3] {
    if !mesh.has_colors() {
        return DEFAULT_COLOR;
    }
    let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
    for color in &mesh.tri_colors {
        *counts.entry(*color).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(color, _)| color)
        .unwrap_or(DEFAULT_COLOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(size: f64) -> Mesh {
        let s = size as f32;
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
        let mut soup = Vec::new();
        for f in faces {
            soup.extend_from_slice(&f);
        }
        Mesh::from_soup(&soup)
    }

    #[test]
    fn cut_preserves_colors() {
        let mut mesh = cube(20.0);
        mesh.set_tri_colors(vec![[10, 20, 30]; mesh.triangle_count()]);
        let plane = Plane::new(DVec3::Z, DVec3::new(0.0, 0.0, 10.0)).unwrap();
        let mut opts = CutOptions::default();
        opts.connectors.enabled = false;
        let result = cut_mesh(&mesh, &plane, &opts).unwrap();
        assert!(result.negative.has_colors());
        assert!(result.positive.has_colors());
        assert!(result
            .negative
            .tri_colors
            .iter()
            .all(|c| *c == [10, 20, 30]));
        assert!(result
            .positive
            .tri_colors
            .iter()
            .all(|c| *c == [10, 20, 30]));
    }
}
