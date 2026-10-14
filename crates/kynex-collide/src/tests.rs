use kreate_math::DQuat;

use super::*;

fn cube(size: f32) -> Arc<Shape> {
    Arc::new(Shape::of(&kreate_mesh::primitives::cube(size)))
}

fn at(uid: u64, shape: &Arc<Shape>, x: f64, y: f64, z: f64) -> Body {
    Body {
        uid,
        shape: shape.clone(),
        world: DAffine3::from_translation(DVec3::new(x, y, z)),
    }
}

#[test]
fn cubes_meet_when_they_overlap_and_not_when_they_only_touch() {
    let unit = cube(1.0);
    let a = at(1, &unit, 0.0, 0.0, 0.0);
    // Half a cube in from each side: overlapping.
    let hit = contact(&a, &at(2, &unit, 0.5, 0.0, 0.0)).expect("they overlap");
    assert!(hit.x > 0.0 && hit.x < 1.0, "{hit}");
    // A tenth apart.
    assert!(contact(&a, &at(2, &unit, 1.1, 0.0, 0.0)).is_none());
    // Face to face, flush: resting, not running into.
    assert!(contact(&a, &at(2, &unit, 1.0, 0.0, 0.0)).is_none());
    // A millimetre in.
    assert!(contact(&a, &at(2, &unit, 0.999, 0.0, 0.0)).is_some());
}

#[test]
fn a_corner_turned_into_a_face_is_found() {
    let unit = cube(1.0);
    let a = at(1, &unit, 0.0, 0.0, 0.0);
    // Turned 45° about two axes, a corner leading; the centres 1.3 apart is
    // past the faces' 1.0 but short of the corner's reach.
    let turned = Body {
        uid: 2,
        shape: unit.clone(),
        world: DAffine3::from_rotation_translation(
            DQuat::from_rotation_z(0.785) * DQuat::from_rotation_y(0.615),
            DVec3::new(1.3, 0.0, 0.0),
        ),
    };
    assert!(contact(&a, &turned).is_some());
    let further = Body {
        world: DAffine3::from_rotation_translation(
            DQuat::from_rotation_z(0.785) * DQuat::from_rotation_y(0.615),
            DVec3::new(1.4, 0.0, 0.0),
        ),
        ..turned
    };
    assert!(contact(&a, &further).is_none());
}

#[test]
fn a_thin_plate_through_a_cube_is_found_with_no_corner_inside() {
    // A square 3 m on a side, flat, through the middle of a unit cube: its
    // corners are all well outside, only its face crosses the cube's sides.
    let plate = Arc::new(Shape::new(
        vec![
            Vec3::new(-1.5, -1.5, 0.0),
            Vec3::new(1.5, -1.5, 0.0),
            Vec3::new(1.5, 1.5, 0.0),
            Vec3::new(-1.5, 1.5, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    ));
    let unit = cube(1.0);
    let box_ = at(1, &unit, 0.0, 0.0, 0.0);
    assert!(contact(&box_, &at(2, &plate, 0.0, 0.0, 0.1)).is_some());
    assert!(contact(&box_, &at(2, &plate, 0.0, 0.0, 0.6)).is_none());
}

#[test]
fn a_mesh_wholly_inside_another_is_not_seen() {
    // Documented: no surfaces cross. A path catches it on the way in.
    let (big, small) = (cube(4.0), cube(0.5));
    assert!(contact(&at(1, &big, 0.0, 0.0, 0.0), &at(2, &small, 0.0, 0.0, 0.0)).is_none());
}

#[test]
fn obstacles_find_what_a_body_runs_into() {
    let unit = cube(0.5);
    // A 10 × 10 floor of blocks a metre apart.
    let mut bodies = Vec::new();
    for i in 0..10 {
        for j in 0..10 {
            bodies.push(at(100 + i * 10 + j, &unit, i as f64, j as f64, 0.0));
        }
    }
    let obstacles = Obstacles::new(bodies);
    assert_eq!(obstacles.len(), 100);
    // Between four blocks: clear of all.
    let probe = cube(0.3);
    assert!(obstacles
        .contacts(&at(1, &probe, 3.5, 3.5, 0.0), |_| false)
        .is_empty());
    // Into the block at (3, 4), and only it.
    let hits = obstacles.contacts(&at(1, &probe, 3.3, 4.0, 0.0), |_| false);
    assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![134]);
    // Unless it is to be left out.
    assert!(obstacles
        .contacts(&at(1, &probe, 3.3, 4.0, 0.0), |u| u == 134)
        .is_empty());
}
