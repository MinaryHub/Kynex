use super::*;

/// A box `s` on a side about the origin, each face its own four corners
/// (as a mesh splits them, for each face its own normals).
fn cube(s: f64) -> (Vec<DVec3>, Vec<[u32; 3]>) {
    let h = s / 2.0;
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let n = DVec3::from_array({
                let mut a = [0.0; 3];
                a[axis] = sign;
                a
            });
            let u = DVec3::from_array({
                let mut a = [0.0; 3];
                a[(axis + 1) % 3] = 1.0;
                a
            });
            let v = n.cross(u);
            let base = positions.len() as u32;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                positions.push((n + u * a + v * b) * h);
            }
            triangles.push([base, base + 1, base + 2]);
            triangles.push([base, base + 2, base + 3]);
        }
    }
    (positions, triangles)
}

/// A round post `r` in radius and `h` high, its round side
/// in `n` facets.
fn post(r: f64, h: f64, n: u32) -> (Vec<DVec3>, Vec<[u32; 3]>) {
    let mut positions = vec![DVec3::ZERO, DVec3::Z * h];
    for i in 0..n {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        let p = DVec3::new(r * a.cos(), r * a.sin(), 0.0);
        positions.push(p);
        positions.push(p + DVec3::Z * h);
    }
    let mut triangles = Vec::new();
    for i in 0..n {
        let (b0, t0) = (2 + 2 * i, 3 + 2 * i);
        let (b1, t1) = (2 + 2 * ((i + 1) % n), 3 + 2 * ((i + 1) % n));
        triangles.push([0, b1, b0]);
        triangles.push([1, t0, t1]);
        triangles.push([b0, b1, t1]);
        triangles.push([b0, t1, t0]);
    }
    (positions, triangles)
}

#[test]
fn a_box_has_twelve_edges_each_pointing_out_between_its_faces() {
    let (positions, triangles) = cube(0.2);
    let found = chains(&positions, &triangles);
    assert_eq!(found.len(), 12, "{found:?}");
    for c in &found {
        assert_eq!(c.points.len(), 2);
        assert!(!c.closed);
        assert!((c.length() - 0.2).abs() < 1e-9);
        // Out between the two faces: two of its parts, equal, outward.
        let mid = (c.points[0] + c.points[1]) / 2.0;
        for n in &c.normals {
            assert!((n.length() - 1.0).abs() < 1e-9);
            assert!(n.dot(mid) > 0.0, "{n} {mid}");
            let parts: Vec<f64> = n
                .to_array()
                .into_iter()
                .filter(|x| x.abs() > 1e-9)
                .collect();
            assert_eq!(parts.len(), 2, "{n}");
            assert!((parts[0].abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
        }
    }
}

#[test]
fn a_round_post_has_its_rims_as_loops_and_no_edges_down_its_side() {
    let (positions, triangles) = post(0.1, 0.05, 32);
    let found = chains(&positions, &triangles);
    assert_eq!(found.len(), 2, "{found:?}");
    for c in &found {
        assert!(c.closed);
        assert_eq!(c.points.len(), 33);
        let rim = 2.0 * 32.0 * 0.1 * (std::f64::consts::PI / 32.0).sin();
        assert!((c.length() - rim).abs() < 1e-9);
    }
    // The top rim picked by a point beside it, and walked within half a
    // millimetre: a point at each facet's corner, the round being coarser.
    let (top, gap) = nearest(&found, DVec3::new(0.105, 0.0, 0.052)).unwrap();
    assert!(gap < 0.006, "{gap}");
    assert!(top.points.iter().all(|p| (p.z - 0.05).abs() < 1e-12));
    let along = poses(top, TOLERANCE);
    assert_eq!(along.len(), 33);
    // Into the rim, down and in at 45°, along it round.
    for (p, turn) in &along {
        let z = *turn * DVec3::Z;
        let out = DVec3::new(p.x, p.y, 0.0).normalize();
        assert!((z - (-(out + DVec3::Z).normalize())).length() < 0.1, "{z}");
    }
}

#[test]
fn along_a_straight_edge_the_tool_goes_end_to_end_pointing_into_it() {
    let (positions, triangles) = cube(0.2);
    let found = chains(&positions, &triangles);
    let (edge, _) = nearest(&found, DVec3::new(0.0, -0.11, 0.11)).unwrap();
    // The top front edge.
    assert!(edge
        .points
        .iter()
        .all(|p| (p.y + 0.1).abs() < 1e-12 && (p.z - 0.1).abs() < 1e-12));
    let along = poses(edge, TOLERANCE);
    assert_eq!(along.len(), 2);
    let into = DVec3::new(0.0, 1.0, -1.0).normalize();
    let way = (along[1].0 - along[0].0).normalize();
    for (_, turn) in &along {
        assert!((*turn * DVec3::Z - into).length() < 1e-9);
        assert!((*turn * DVec3::X - way).length() < 1e-9);
    }
}
