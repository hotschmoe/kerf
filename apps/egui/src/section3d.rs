//! Section cut for the 3D view: slice every mesh part with the plane z = cut_z and build the
//! cut caps (drawn manila) as even-odd polygon groups. Solids in front of the plane (z > cut_z)
//! are clipped away by the shader; the caps close them.

use crate::ir::{Mesh, P2, even_odd_groups, polygon_area, triangulate};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Cap {
    pub src: String,
    pub material: String,
    /// outer loop first, holes after (model inches, XY)
    pub groups: Vec<Vec<Vec<P2>>>,
    pub z: f32,
}

type Key = (i64, i64);

fn q(p: P2) -> Key {
    ((p[0] * 2000.0).round() as i64, (p[1] * 2000.0).round() as i64)
}

pub fn build_caps(mesh: &Mesh, cut_z: f32) -> Vec<Cap> {
    let mut caps = Vec::new();
    let cz = cut_z as f64;
    for part in &mesh.parts {
        let pos = |i: u32| -> [f64; 3] { [part.positions[i as usize * 3] as f64, part.positions[i as usize * 3 + 1] as f64, part.positions[i as usize * 3 + 2] as f64] };
        let mut segs: Vec<(P2, P2)> = Vec::new();
        for t in part.indices.chunks_exact(3) {
            let v = [pos(t[0]), pos(t[1]), pos(t[2])];
            let above = [v[0][2] > cz + 1e-6, v[1][2] > cz + 1e-6, v[2][2] > cz + 1e-6];
            let n_above = above.iter().filter(|a| **a).count();
            if n_above == 0 || n_above == 3 {
                continue;
            }
            let mut pts: Vec<P2> = Vec::new();
            for e in 0..3 {
                let (a, b) = (v[e], v[(e + 1) % 3]);
                if above[e] != above[(e + 1) % 3] {
                    let k = (cz - a[2]) / (b[2] - a[2]);
                    pts.push([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]);
                }
            }
            if pts.len() == 2 && (pts[0][0] - pts[1][0]).abs() + (pts[0][1] - pts[1][1]).abs() > 1e-9 {
                segs.push((pts[0], pts[1]));
            }
        }
        if segs.is_empty() {
            continue;
        }
        // chain segments into loops through quantized endpoints
        let mut by_end: HashMap<Key, Vec<usize>> = HashMap::new();
        for (i, (a, b)) in segs.iter().enumerate() {
            by_end.entry(q(*a)).or_default().push(i);
            by_end.entry(q(*b)).or_default().push(i);
        }
        let mut used = vec![false; segs.len()];
        let mut loops: Vec<Vec<P2>> = Vec::new();
        for start in 0..segs.len() {
            if used[start] {
                continue;
            }
            used[start] = true;
            let mut lp = vec![segs[start].0];
            let mut cur = segs[start].1;
            let mut guard = 0;
            loop {
                guard += 1;
                if guard > segs.len() + 2 {
                    break;
                }
                let k = q(cur);
                if k == q(lp[0]) && lp.len() > 2 {
                    break;
                }
                lp.push(cur);
                let next = by_end.get(&k).and_then(|l| l.iter().copied().find(|&j| !used[j]));
                let Some(j) = next else { break };
                used[j] = true;
                let (a, b) = segs[j];
                cur = if q(a) == k { b } else { a };
            }
            if lp.len() >= 3 && polygon_area(&lp).abs() > 1e-6 {
                loops.push(lp);
            }
        }
        if loops.is_empty() {
            continue;
        }
        caps.push(Cap { src: part.src.clone(), material: part.material.clone(), groups: even_odd_groups(loops), z: cut_z });
    }
    caps
}

/// Triangulated cap geometry: (xy verts, indices) per group.
pub fn tris(cap: &Cap) -> Vec<(Vec<[f32; 2]>, Vec<u32>)> {
    cap.groups.iter().map(|g| triangulate(g)).filter(|(_, i)| !i.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::MeshPart;

    fn boxm(x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32) -> MeshPart {
        let p = vec![x0, y0, z0, x1, y0, z0, x1, y1, z0, x0, y1, z0, x0, y0, z1, x1, y0, z1, x1, y1, z1, x0, y1, z1];
        let idx: Vec<u32> = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
        MeshPart { src: "b".into(), positions: p, indices: idx, ..Default::default() }
    }

    #[test]
    fn slicing_a_box_gives_one_rectangle() {
        let m = Mesh { parts: vec![boxm(0.0, 0.0, -2.0, 4.0, 3.0, 2.0)] };
        let caps = build_caps(&m, 0.0);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].groups.len(), 1);
        assert!((polygon_area(&caps[0].groups[0][0]).abs() - 12.0).abs() < 1e-6);
        // a plane outside the solid produces nothing
        assert!(build_caps(&m, 5.0).is_empty());
    }
}
