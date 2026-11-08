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
    /// Arêtes de découpe orientées, dans l'ordre des polygones clippés.
    cap_edges: Vec<(u32, u32)>,
}

impl Side {
    fn push_tri(&mut self, tri: [u32; 3], color: [u8; 3]) {
        self.tris.push(tri);
        self.colors.push(color);
    }

    fn add_cap_edge(&mut self, a: u32, b: u32) {
        if a != b {
            self.cap_edges.push((a, b));
        }
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
    let (mut neg_loops, neg_remap) = build_cap_loops(&neg.cap_edges, &pos, plane);
    let (mut pos_loops, pos_remap) = build_cap_loops(&posi.cap_edges, &pos, plane);
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

    let neg_regions = nest_loops(&mut neg_loops, -1.0);
    let pos_regions = nest_loops(&mut pos_loops, 1.0);

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
        // La taille demandée par l'utilisateur est toujours respectée ; le
        // placement automatique ne détermine que le nombre de tenons.
        let (_, auto_count) = auto_connector_params(cap_area);
        let radius = opts.connectors.radius.clamp(0.5, 50.0);
        let count = if opts.connectors.auto {
            auto_count.max(1)
        } else {
            opts.connectors.count.clamp(1, 32)
        };

        let points = sample_points(&pos_loops, &pos_regions, radius, count);

        if points.is_empty() {
            warnings.push(
                "Connecteurs ignorés : la section de découpe est trop étroite pour la taille demandée."
                    .into(),
            );
        } else {
            connector_count = points.len();
            connector_radius = radius;
            let clearance = opts.connectors.clearance.clamp(0.0, 2.0);
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

    // Les sommets fusionnés pour équilibrer la section doivent l'être aussi
    // dans la surface : bouchons et surface partagent alors les mêmes indices.
    remap_tris(&mut neg_tris, &neg_remap);
    remap_tris(&mut pos_tris, &pos_remap);


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

/// Reconstruit les boucles de découpe à partir des arêtes orientées.
///
/// Les arêtes proviennent directement des polygones clippés : leurs extrémités
/// sont des indices de sommets existants, partagés par les triangles voisins.
/// On n'effectue donc **aucun recalage** de position.
///
/// Sur un modèle multicolore, les surfaces de composants accolés produisent des
/// paires de sommets quasi confondus dont les déséquilibres sont opposés (±1) :
/// le graphe de section n'est alors pas équilibré et les boucles restent
/// ouvertes. Ces paires sont fusionnées (les plus proches d'abord) ; la table de
/// fusion est renvoyée pour être appliquée aux triangles de surface, afin que
/// bouchons et surface partagent les mêmes identifiants.
///
/// Une fois le graphe équilibré (autant d'arêtes entrantes que sortantes en
/// chaque sommet), un parcours glouton se referme toujours : chaque arête est
/// bouchée par exactement une boucle.
fn build_cap_loops(
    edges: &[(u32, u32)],
    pos: &[DVec3],
    plane: &Plane,
) -> (Vec<CapLoop>, HashMap<u32, u32>) {
    if edges.is_empty() {
        return (Vec::new(), HashMap::new());
    }

    // Arêtes orientées avec leur multiplicité. Les surfaces confondues des
    // modèles multicolores produisent des arêtes identiques : les fusionner
    // désequilibrerait le graphe, donc on conserve le nombre d'occurrences.
    let mut initial: HashMap<(u32, u32), u32> = HashMap::with_capacity(edges.len());
    for &(a, b) in edges {
        if a != b {
            *initial.entry((a, b)).or_default() += 1;
        }
    }

    // Deux sommets distincts peuvent occuper exactement la même position sur
    // le plan de coupe (contact entre deux lobes du contour). Les laisser
    // séparés produit des arêtes de longueur nulle que la soudure du maillage
    // élimine ensuite, laissant des fissures. On les fusionne d'abord.
    let remap_coincident = merge_coincident(&mut initial, pos, plane);
    let (mut remaining, remap_unbalanced) = merge_unbalanced(&initial, pos, plane);
    // Compose : les identifiants d'origine fusionnés par coïncidence doivent
    // pointer vers la cible finale de l'équilibrage.
    let mut remap: HashMap<u32, u32> = HashMap::with_capacity(remap_coincident.len() + remap_unbalanced.len());
    for (&k, &v) in &remap_coincident {
        remap.insert(k, *remap_unbalanced.get(&v).unwrap_or(&v));
    }
    for (&k, &v) in &remap_unbalanced {
        remap.entry(k).or_insert(v);
    }
    let total_edges: usize = remaining.values().map(|&c| c as usize).sum();
    if total_edges == 0 {
        return (Vec::new(), remap);
    }

    let mut starts: Vec<(u32, u32)> = remaining.keys().copied().collect();
    starts.sort_unstable();
    let mut out_edges: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(a, b) in &starts {
        out_edges.entry(a).or_default().push(b);
    }

    let mut loops: Vec<CapLoop> = Vec::new();

    for &(start_a, start_b) in &starts {
        if remaining.get(&(start_a, start_b)).copied().unwrap_or(0) == 0 {
            continue;
        }
        let mut ids: Vec<u32> = vec![start_a];
        let mut prev = start_a;
        let mut cur = start_b;
        *remaining.get_mut(&(start_a, start_b)).unwrap() -= 1;
        let mut closed = false;
        for _ in 0..=total_edges {
            if cur == start_a {
                closed = true;
                break;
            }
            ids.push(cur);
            let Some(candidates) = out_edges.get(&cur) else {
                break;
            };
            let p_prev = plane.to2d(pos[prev as usize]);
            let p_cur = plane.to2d(pos[cur as usize]);
            let reverse = p_prev - p_cur;
            let mut best: Option<(u32, f64)> = None;
            // Retour immédiat par l'arête inverse : à n'utiliser qu'en dernier
            // recours, sinon on refermerait une boucle de deux sommets (aire
            // nulle) au lieu de suivre le contour de la section.
            let mut fallback: Option<(u32, f64)> = None;
            for &cand in candidates {
                if remaining.get(&(cur, cand)).copied().unwrap_or(0) == 0 {
                    continue;
                }
                let d = plane.to2d(pos[cand as usize]) - p_cur;
                let cross = reverse.x * d.y - reverse.y * d.x;
                let dot = reverse.dot(d);
                let angle = cross.atan2(dot);
                // Distance angulaire parcourue dans le sens horaire depuis
                // l'arête inverse, dans [0, 2π].
                let rot = if angle <= 0.0 {
                    -angle
                } else {
                    std::f64::consts::TAU - angle
                };
                if cand == prev {
                    if fallback.map(|(_, a)| rot < a).unwrap_or(true) {
                        fallback = Some((cand, rot));
                    }
                    continue;
                }
                if best.map(|(_, a)| rot < a).unwrap_or(true) {
                    best = Some((cand, rot));
                }
            }
            let Some((next, _)) = best.or(fallback) else { break };
            *remaining.get_mut(&(cur, next)).unwrap() -= 1;
            prev = cur;
            cur = next;
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
        // Une boucle peut repasser par un même sommet (jonction
        // non-manifold). On la coupe en cycles simples, sinon la
        // triangulation du bouchon ne conserve pas toutes les arêtes.
        for simple in split_simple_cycles(&ids) {
            let pts2: Vec<DVec2> = simple
                .iter()
                .map(|&id| plane.to2d(pos[id as usize]))
                .collect();
            let area2 = signed_area2(&pts2);
            if area2.abs() < 1e-12 {
                continue;
            }
            loops.push(CapLoop {
                ids: simple,
                pts2,
                area2,
                depth: 0,
            });
        }
    }

    (loops, remap)
}

/// Coupe un circuit fermé en cycles simples aux sommets visités plusieurs fois.
///
/// Une section non-manifold peut passer deux fois par le même sommet (deux
/// lobes accolés). Chaque lobe doit être triangulé séparément pour que toutes
/// les arêtes du contour restent présentes dans le bouchon.
fn split_simple_cycles(ids: &[u32]) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    let mut pos: HashMap<u32, usize> = HashMap::new();
    for &v in ids {
        if let Some(&at) = pos.get(&v) {
            if stack.len() - at >= 3 {
                out.push(stack[at..].to_vec());
            }
            for &w in &stack[at + 1..] {
                pos.remove(&w);
            }
            stack.truncate(at + 1);
        } else {
            pos.insert(v, stack.len());
            stack.push(v);
        }
    }
    if stack.len() >= 3 {
        out.push(stack);
    }
    out
}

/// Fusionne les sommets du contour occupant la même position sur le plan.
///
/// Un contact ponctuel entre deux lobes de la section produit deux sommets
/// distincts à la même position. Le graphe reste équilibré, mais les arêtes de
/// longueur nulle disparaissent à la soudure du maillage et laissent des
/// fissures. Fusionner ces sommets rend le contour simple avant triangulation.
fn merge_coincident(
    initial: &mut HashMap<(u32, u32), u32>,
    pos: &[DVec3],
    plane: &Plane,
) -> HashMap<u32, u32> {
    let mut verts: Vec<u32> = Vec::new();
    {
        let mut seen: HashSet<u32> = HashSet::new();
        for &(a, b) in initial.keys() {
            if seen.insert(a) {
                verts.push(a);
            }
            if seen.insert(b) {
                verts.push(b);
            }
        }
    }
    verts.sort_unstable();
    let tol = 1e-7;
    let inv = 1.0 / tol;
    let mut buckets: HashMap<(i64, i64), usize> = HashMap::new();
    let mut canon: Vec<u32> = Vec::new();
    let mut canon_pts: Vec<DVec2> = Vec::new();
    let mut remap: HashMap<u32, u32> = HashMap::new();
    for id in verts {
        let p = plane.to2d(pos[id as usize]);
        let key = ((p.x * inv).round() as i64, (p.y * inv).round() as i64);
        let mut found: Option<usize> = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(&ci) = buckets.get(&(key.0 + dx, key.1 + dy)) {
                    if (canon_pts[ci] - p).length() <= tol {
                        found = Some(ci);
                        break 'search;
                    }
                }
            }
        }
        match found {
            Some(ci) => {
                remap.insert(id, canon[ci]);
            }
            None => {
                let ci = canon.len();
                canon.push(id);
                canon_pts.push(p);
                buckets.insert(key, ci);
            }
        }
    }
    if remap.is_empty() {
        return remap;
    }
    let mut out: HashMap<(u32, u32), u32> = HashMap::with_capacity(initial.len());
    for (&(a, b), &count) in initial.iter() {
        let a = *remap.get(&a).unwrap_or(&a);
        let b = *remap.get(&b).unwrap_or(&b);
        if a == b {
            continue;
        }
        *out.entry((a, b)).or_default() += count;
    }
    *initial = out;
    remap
}

/// Équilibre le graphe de section en fusionnant les sommets déséquilibrés.
///
/// Le déséquilibre d'un sommet vaut `sortantes - entrantes`. Deux sommets quasi
/// confondus de déséquilibre `+1` et `-1` proviennent de deux composants
/// accolés : les identifier referme l'interstice et rétablit l'équilibre. Les
/// paires les plus proches sont traitées d'abord, et la fusion se propage
/// jusqu'à ce qu'il ne reste plus aucun déséquilibre.
fn merge_unbalanced(
    edges: &HashMap<(u32, u32), u32>,
    pos: &[DVec3],
    plane: &Plane,
) -> (HashMap<(u32, u32), u32>, HashMap<u32, u32>) {
    fn find(parent: &mut HashMap<u32, u32>, x: u32) -> u32 {
        let mut root = x;
        while let Some(&p) = parent.get(&root) {
            if p == root {
                break;
            }
            root = p;
        }
        let mut cur = x;
        while let Some(&p) = parent.get(&cur) {
            if p == cur {
                break;
            }
            parent.insert(cur, root);
            cur = p;
        }
        root
    }

    let mut parent: HashMap<u32, u32> = HashMap::new();
    for &(a, b) in edges.keys() {
        parent.entry(a).or_insert(a);
        parent.entry(b).or_insert(b);
    }

    let max_unions = parent.len().max(1);
    for _ in 0..max_unions {
        let mut delta: HashMap<u32, i64> = HashMap::new();
        for (&(a, b), &count) in edges {
            let ra = find(&mut parent, a);
            let rb = find(&mut parent, b);
            *delta.entry(ra).or_default() += count as i64;
            *delta.entry(rb).or_default() -= count as i64;
        }

        let mut positives: Vec<u32> = delta
            .iter()
            .filter(|(_, &d)| d > 0)
            .map(|(&v, _)| v)
            .collect();
        let mut negatives: Vec<u32> = delta
            .iter()
            .filter(|(_, &d)| d < 0)
            .map(|(&v, _)| v)
            .collect();
        if positives.is_empty() || negatives.is_empty() {
            break;
        }
        // Tri pour un résultat déterministe (l'ordre des HashMap varie).
        positives.sort_unstable();
        negatives.sort_unstable();

        let mut best: Option<(f64, u32, u32)> = None;
        for &p in &positives {
            let pp = plane.to2d(pos[p as usize]);
            for &n in &negatives {
                let q = plane.to2d(pos[n as usize]);
                let dist = (pp - q).length_squared();
                if best.map(|(d, _, _)| dist < d).unwrap_or(true) {
                    best = Some((dist, p, n));
                }
            }
        }
        let Some((_dist, p, n)) = best else { break };
        // Représentant : le plus petit identifiant, pour rester déterministe.
        let (keep, gone) = if p < n { (p, n) } else { (n, p) };
        parent.insert(gone, keep);
    }

    let mut merged: HashMap<(u32, u32), u32> = HashMap::with_capacity(edges.len());
    for (&(a, b), &count) in edges {
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        if ra != rb {
            *merged.entry((ra, rb)).or_default() += count;
        }
    }

    let mut remap: HashMap<u32, u32> = HashMap::new();
    let keys: Vec<u32> = parent.keys().copied().collect();
    for v in keys {
        let r = find(&mut parent, v);
        if r != v {
            remap.insert(v, r);
        }
    }
    (merged, remap)
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

/// Classe les boucles en régions de matière.
///
/// L'orientation d'une boucle indique de quel côté se trouve la matière : une
/// boucle « pleine » délimite un solide, une boucle « creuse » délimite une
/// cavité. `solid_sign` traduit la convention de parcours : le côté négatif
/// produit des boucles horaires, le côté positif des boucles antihoraires.
fn nest_loops(loops: &mut [CapLoop], solid_sign: f64) -> Vec<Region> {
    let n = loops.len();
    if n == 0 {
        return Vec::new();
    }
    let areas: Vec<f64> = loops.iter().map(|l| l.abs_area()).collect();
    let solid: Vec<bool> = loops
        .iter()
        .map(|l| l.area2 * solid_sign > 0.0)
        .collect();

    // Matrice de contenance (j contient i), avec filtre par aire.
    let mut contains = vec![vec![false; n]; n];
    for i in 0..n {
        let p = loops[i].pts2[0];
        for j in 0..n {
            if i == j || areas[j] <= areas[i] * (1.0 + 1e-9) {
                continue;
            }
            if point_in_polygon(p, &loops[j].pts2) {
                contains[j][i] = true;
            }
        }
    }

    let mut solid_inside = vec![0usize; n];
    let mut cavity_inside = vec![0usize; n];
    for i in 0..n {
        for j in 0..n {
            if contains[j][i] {
                if solid[j] {
                    solid_inside[i] += 1;
                } else {
                    cavity_inside[i] += 1;
                }
            }
        }
        loops[i].depth = solid_inside[i] + cavity_inside[i];
    }

    // Toute boucle pleine délimite une région de matière ; les solides
    // entièrement contenus dans une autre matière sont redondants mais restent
    // bouchés (ils correspondent à des pièces imbriquées du modèle d'origine).
    let mut regions: Vec<Region> = Vec::new();
    for i in 0..n {
        if solid[i] {
            regions.push(Region {
                outer: i,
                holes: Vec::new(),
            });
        }
    }

    // Chaque cavité est retirée de la plus petite région qui la contient.
    for i in 0..n {
        if solid[i] {
            continue;
        }
        let mut best: Option<usize> = None;
        let mut best_area = f64::INFINITY;
        for (ri, r) in regions.iter().enumerate() {
            if contains[r.outer][i] && areas[r.outer] < best_area {
                best_area = areas[r.outer];
                best = Some(ri);
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

/// Décale légèrement les sommets exactement colinéaires ou confondus d'un
/// contour, vers l'extérieur de celui-ci.
///
/// Cela limite les cas dégénérés soumis à `earcut` sans changer la topologie
/// du bouchon : les triangles produits référencent toujours les sommets
/// d'origine. Les fissures résiduelles sont refermées ensuite par
/// `Mesh::seal_open_boundaries`.
fn jitter_degenerate(pts: &mut [DVec2]) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let mut min = DVec2::splat(f64::INFINITY);
    let mut max = DVec2::splat(f64::NEG_INFINITY);
    for p in pts.iter() {
        min = min.min(*p);
        max = max.max(*p);
    }
    let scale = (max - min).length().max(1.0);
    let eps = scale * 1e-7;
    let area2 = signed_area2(pts);
    let orient = if area2 >= 0.0 { 1.0 } else { -1.0 };
    let orig = pts.to_vec();
    for i in 0..n {
        let prev = orig[(i + n - 1) % n];
        let cur = orig[i];
        let next = orig[(i + 1) % n];
        let d1 = cur - prev;
        let d2 = next - cur;
        let cross = d1.x * d2.y - d1.y * d2.x;
        let dup = (next - cur).length_squared() <= eps * eps;
        if cross != 0.0 && !dup {
            continue;
        }
        let mut dir = next - prev;
        if dir.length_squared() <= 1e-24 {
            dir = d2;
        }
        if dir.length_squared() <= 1e-24 {
            dir = d1;
        }
        if dir.length_squared() <= 1e-24 {
            continue;
        }
        dir = dir.normalize();
        // Extérieur du contour : à droite du sens de parcours s'il est
        // antihoraire, à gauche sinon.
        let outward = if orient > 0.0 {
            DVec2::new(dir.y, -dir.x)
        } else {
            DVec2::new(-dir.y, dir.x)
        };
        pts[i] = cur + outward * eps;
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
        let mut rings: Vec<&[u32]> = Vec::with_capacity(1 + region.holes.len());
        rings.push(outer.ids.as_slice());
        for &h in &region.holes {
            rings.push(loops[h].ids.as_slice());
        }
        for (hole_region, ring) in extra_holes {
            if *hole_region == ri {
                rings.push(ring.as_slice());
            }
        }

        let mut coords: Vec<f64> = Vec::new();
        let mut ids: Vec<u32> = Vec::new();
        let mut hole_starts: Vec<usize> = Vec::new();
        for (k, ring) in rings.iter().enumerate() {
            if ring.len() < 3 {
                continue;
            }
            if k > 0 {
                hole_starts.push(ids.len());
            }
            let mut pts2: Vec<DVec2> = ring
                .iter()
                .map(|&id| plane.to2d(pos[id as usize]))
                .collect();
            // Limite les cas dégénérés que `earcut` écarte (points confondus
            // ou exactement colinéaires), sans modifier les identifiants
            // produits. Les fissures éventuelles sont comblées à la soudure.
            jitter_degenerate(&mut pts2);
            for p in &pts2 {
                coords.push(p.x);
                coords.push(p.y);
            }
            ids.extend_from_slice(ring);
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

/// Applique une table de fusion de sommets aux triangles.
fn remap_tris(tris: &mut [[u32; 3]], remap: &HashMap<u32, u32>) {
    if remap.is_empty() {
        return;
    }
    for tri in tris.iter_mut() {
        for id in tri.iter_mut() {
            if let Some(&r) = remap.get(id) {
                *id = r;
            }
        }
    }
}

fn build_mesh(tris: &[[u32; 3]], colors: &[[u8; 3]], pos: &[DVec3], colored: bool) -> Mesh {
    // Soudure par position : le recalage des boucles de bouchon crée des
    // sommets distincts à la même position que ceux de la surface clippée.
    // Sans cette soudure, les bouchons ne referment pas le maillage.
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for t in tris {
        for &i in t {
            let p = pos[i as usize];
            min = min.min(p);
            max = max.max(p);
        }
    }
    let diag = if min.is_finite() {
        (max - min).length().max(1.0)
    } else {
        1.0
    };
    let eps = (diag * 1e-6).max(1e-5);
    let inv = 1.0 / eps;
    let mut remap: HashMap<(i64, i64, i64), u32> = HashMap::with_capacity(pos.len());
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(pos.len());
    let mut indices: Vec<u32> = Vec::with_capacity(tris.len() * 3);
    let track_colors = colored && colors.len() == tris.len();
    let mut out_colors: Vec<[u8; 3]> = if track_colors {
        Vec::with_capacity(tris.len())
    } else {
        Vec::new()
    };
    for (ti, t) in tris.iter().enumerate() {
        let mut ids = [0u32; 3];
        for (k, &i) in t.iter().enumerate() {
            let p = pos[i as usize];
            let key = (
                (p.x * inv).round() as i64,
                (p.y * inv).round() as i64,
                (p.z * inv).round() as i64,
            );
            ids[k] = *remap.entry(key).or_insert_with(|| {
                let id = positions.len() as u32;
                positions.push([p.x as f32, p.y as f32, p.z as f32]);
                id
            });
        }
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
            continue;
        }
        indices.extend_from_slice(&ids);
        if track_colors {
            out_colors.push(colors[ti]);
        }
    }
    let mut mesh = Mesh::from_indexed(positions, indices);
    mesh.compact();
    if track_colors {
        mesh.set_tri_colors(out_colors);
    }
    for _ in 0..4 {
        if mesh.seal_open_boundaries() == 0 {
            break;
        }
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
