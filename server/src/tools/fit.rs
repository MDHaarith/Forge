//! Geometry for the board-in-enclosure fit check: STL parsing, placements and triangle-box overlap.
use super::ToolError;
use crate::jsfmt::parse_number;
use serde::{Deserialize, Serialize};

pub type Vec3 = [f64; 3];

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Placement {
    /// Millimetres.
    pub position: Vec3,
    /// Degrees about X, then Y, then Z of the rotated frame (three.js "XYZ" order, as the 3D view uses).
    pub rotation: Vec3,
}

/// Triangles of an STL file, binary or ASCII, as 9 numbers per triangle.
pub fn parse_stl(data: &[u8]) -> Result<Vec<f32>, ToolError> {
    if data.len() >= 84 {
        let count = u32::from_le_bytes(data[80..84].try_into().unwrap()) as usize;
        if count.checked_mul(50).and_then(|n| n.checked_add(84)) == Some(data.len()) {
            let mut out = Vec::with_capacity(count * 9);
            for i in 0..count {
                for k in 0..9 {
                    let offset = 84 + i * 50 + 12 + k * 4;
                    out.push(f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()));
                }
            }
            return Ok(out);
        }
    }
    let text: String = data.iter().map(|b| char::from(*b)).collect();
    let whitespace = |c: char| crate::jsfmt::trim(&c.to_string()).is_empty();
    let mut rest = text.as_str();
    let mut out = Vec::new();
    while let Some(at) = rest.find("vertex") {
        rest = &rest[at + 6..];
        if !rest.starts_with(whitespace) { continue; }
        let mut tail = rest;
        let mut vertex = Vec::new();
        for _ in 0..3 {
            if !tail.starts_with(whitespace) { break; }
            tail = tail.trim_start_matches(whitespace);
            let end = tail.find(whitespace).unwrap_or(tail.len());
            if end == 0 { break; }
            vertex.push(parse_number(&tail[..end]));
            tail = &tail[end..];
        }
        if vertex.len() == 3 {
            out.extend(vertex);
            rest = tail;
        }
    }
    if out.is_empty() || out.len() % 9 != 0 || out.iter().any(|v| v.is_nan()) {
        return Err(ToolError::unexpected("not an STL file"));
    }
    Ok(out.into_iter().map(|v| v as f32).collect())
}

/// Row-major 3×3 rotation for a placement: Rx · Ry · Rz.
pub fn rotation_matrix([ax, ay, az]: Vec3) -> [f64; 9] {
    let [a, b, c] = [ax, ay, az].map(|d| d * std::f64::consts::PI / 180.0);
    let (ca, sa, cb, sb, cc, sc) = (a.cos(), a.sin(), b.cos(), b.sin(), c.cos(), c.sin());
    [cb * cc, -cb * sc, sb,
        ca * sc + sa * sb * cc, ca * cc - sa * sb * sc, -sa * cb,
        sa * sc - ca * sb * cc, sa * cc + ca * sb * sc, ca * cb]
}

pub fn apply(m: [f64; 9], t: Vec3, p: Vec3) -> Vec3 {
    [m[0]*p[0] + m[1]*p[1] + m[2]*p[2] + t[0], m[3]*p[0] + m[4]*p[1] + m[5]*p[2] + t[1], m[6]*p[0] + m[7]*p[1] + m[8]*p[2] + t[2]]
}

/// The inverse of apply(m, t, ·) for a rotation m.
pub fn unapply(m: [f64; 9], t: Vec3, p: Vec3) -> Vec3 {
    let q = std::array::from_fn::<_, 3, _>(|i| p[i] - t[i]);
    [m[0]*q[0] + m[3]*q[1] + m[6]*q[2], m[1]*q[0] + m[4]*q[1] + m[7]*q[2], m[2]*q[0] + m[5]*q[1] + m[8]*q[2]]
}

/// Separating-axis test of a triangle against an axis-aligned box given by centre and half sizes (Akenine-Möller).
pub fn triangle_hits_box(c: Vec3, h: Vec3, a: Vec3, b: Vec3, d: Vec3) -> bool {
    let v = [a,b,d].map(|p| std::array::from_fn::<_,3,_>(|i| p[i] - c[i]));
    for i in 0..3 {
        let lo = v[0][i].min(v[1][i]).min(v[2][i]);
        let hi = v[0][i].max(v[1][i]).max(v[2][i]);
        if lo > h[i] || hi < -h[i] { return false; }
    }
    let e = std::array::from_fn::<_,3,_>(|i| std::array::from_fn::<_,3,_>(|k| v[(i+1)%3][k] - v[i][k]));
    // The nine cross products of the box axes with the triangle edges.
    for edge in e {
        for ax in [[0.0,-edge[2],edge[1]], [edge[2],0.0,-edge[0]], [-edge[1],edge[0],0.0]] {
            let p = v.map(|x| x[0]*ax[0] + x[1]*ax[1] + x[2]*ax[2]);
            let r = h[0]*ax[0].abs() + h[1]*ax[1].abs() + h[2]*ax[2].abs();
            if p[0].min(p[1]).min(p[2]) > r || p[0].max(p[1]).max(p[2]) < -r { return false; }
        }
    }
    // The triangle's plane.
    let n = [e[0][1]*e[1][2]-e[0][2]*e[1][1], e[0][2]*e[1][0]-e[0][0]*e[1][2], e[0][0]*e[1][1]-e[0][1]*e[1][0]];
    let dist = n[0]*v[0][0] + n[1]*v[0][1] + n[2]*v[0][2];
    let r = h[0]*n[0].abs() + h[1]*n[1].abs() + h[2]*n[2].abs();
    dist.abs() <= r
}

#[derive(Clone, Debug, Deserialize)]
pub struct ComponentBox { pub name: String, pub layer: String, pub min: Vec3, pub max: Vec3 }
#[derive(Clone, Debug, Deserialize)]
pub struct BoardBoxes { pub size: Vec3, pub center: [f64; 2], pub components: Vec<ComponentBox> }
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FitIssue {
    pub board: String,
    /// "board" for the board itself, else a part reference such as J1.
    pub item: String,
    pub part: String,
    pub status: &'static str,
}
pub struct BoardItem { pub item: String, pub min: Vec3, pub max: Vec3 }
pub struct FitBoard { pub name: String, pub placement: Placement, pub board: BoardBoxes }
pub struct FitSolid { pub name: String, pub placement: Placement, pub triangles: Vec<f32> }

/// Items of a board to check, in the board's own frame: the board itself, then every part on it.
pub fn board_items(board: &BoardBoxes) -> Vec<BoardItem> {
    let [w,h,t] = board.size;
    let [cx,cy] = board.center;
    let mut items = vec![BoardItem { item: "board".into(), min: [cx-w/2.0,cy-h/2.0,-t/2.0], max: [cx+w/2.0,cy+h/2.0,t/2.0] }];
    items.extend(board.components.iter().map(|c| BoardItem { item:c.name.clone(), min:c.min, max:c.max }));
    items
}

/// Which parts of each board touch an enclosure solid. A box that cuts into a solid is a collision; one
/// that only comes within `clearance` is tight. The board itself may rest on standoffs, so it only
/// collides when it sinks more than 0.05 mm into a solid.
pub fn fit_check(boards: &[FitBoard], solids: &[FitSolid], clearance: f64) -> Vec<FitIssue> {
    let mut issues = Vec::new();
    for b in boards {
        let bm = rotation_matrix(b.placement.rotation);
        let items = board_items(&b.board);
        for s in solids {
            let sm = rotation_matrix(s.placement.rotation);
            // Every triangle in the board's frame, once per pair.
            let tris: Vec<[Vec3;3]> = s.triangles.chunks_exact(9).map(|tri| std::array::from_fn(|k| {
                unapply(bm,b.placement.position,apply(sm,s.placement.position,[tri[k*3] as f64,tri[k*3+1] as f64,tri[k*3+2] as f64]))
            })).collect();
            for it in &items {
                let shrink = if it.item == "board" { 0.05 } else { 0.001 };
                let test = |grow:f64| {
                    let c = std::array::from_fn(|i| (it.min[i]+it.max[i])/2.0);
                    let h = std::array::from_fn(|i| ((it.max[i]-it.min[i])/2.0+grow).max(1e-6));
                    tris.iter().any(|[p,q,r]| triangle_hits_box(c,h,*p,*q,*r))
                };
                let status = if test(-shrink) { Some("collision") } else if clearance > 0.0 && test(clearance) { Some("tight") } else { None };
                if let Some(status) = status { issues.push(FitIssue { board:b.name.clone(), item:it.item.clone(), part:s.name.clone(), status }); }
            }
        }
    }
    issues
}

/// The world-space bounds of a board with its parts, for sizing an enclosure around it.
pub fn board_bounds(board: &BoardBoxes, placement: &Placement) -> (Vec3, Vec3) {
    let m = rotation_matrix(placement.rotation);
    let mut min = [f64::INFINITY;3];
    let mut max = [f64::NEG_INFINITY;3];
    for it in board_items(board) {
        for corner in 0..8 {
            let p = apply(m,placement.position,std::array::from_fn(|i| if (corner>>i)&1 != 0 {it.max[i]} else {it.min[i]}));
            for i in 0..3 { min[i]=min[i].min(p[i]); max[i]=max[i].max(p[i]); }
        }
    }
    (min,max)
}
