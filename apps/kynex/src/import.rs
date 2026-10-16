//! Models imported without Kreate: a CAD or mesh file the page sends, read
//! with Kreate's own readers (kreate-io) into a scene of its own, its parts
//! under one object standing on the floor at the model's middle — ready to
//! be kept as equipment of the user's own (`Library::save_custom`).

use kreate_io::ir::ImportScene;
use kreate_math::DVec3;
use kreate_scene::{Document, Object, ObjectData};

/// The files read, by extension.
pub const FORMATS: &[&str] = &["step", "stp", "stl", "obj", "glb", "gltf", "3mf", "ply"];

/// The most triangles a model may have: kept in the project file, and drawn
/// with everything else.
const MOST_TRIANGLES: usize = 2_000_000;

/// A model read: its scene, the identity of the object its parts hang
/// under, its name, and what the reader had to guess.
pub struct Model {
    pub doc: Document,
    pub root: u64,
    pub name: String,
    pub notes: Vec<String>,
}

impl Model {
    /// Its name and what the reader had to guess, for the page.
    pub fn name_notes(self) -> (String, Vec<String>) {
        (self.name, self.notes)
    }
}

/// What a file brings: a shape, kept as equipment of the user's own, or a
/// robot (its description, as a zip of its folder or a `.urdf`).
pub enum Imported {
    Shape(Model),
    Robot(crate::library::RobotModel),
}

/// File `name`'s bytes read as what it is (see [`read`] for shapes).
pub fn read_any(name: &str, bytes: &[u8], unit: Option<f64>) -> Result<Imported, String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") || lower.ends_with(".urdf") {
        return crate::library::RobotModel::from_file(String::new(), name, bytes)
            .map(Imported::Robot);
    }
    read(name, bytes, unit).map(Imported::Shape)
}

/// File `name`'s bytes read as a model. `unit` is metres per the file's
/// unit, for a file that does not say (STL, OBJ, PLY): or the reader's own
/// guess, millimetres for STL, metres for the others. A file that names its
/// units (STEP, glTF, 3MF) keeps them unless `unit` is given.
pub fn read(name: &str, bytes: &[u8], unit: Option<f64>) -> Result<Model, String> {
    let path = std::path::Path::new(name);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let stem: String = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("모델")
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(60)
        .collect();
    let stem = if stem.is_empty() {
        "모델".to_string()
    } else {
        stem
    };
    let text = || String::from_utf8_lossy(bytes).into_owned();
    let unit = unit.filter(|u| u.is_finite() && *u > 0.0);
    let mut scene: ImportScene = match ext.as_str() {
        "step" | "stp" => {
            kreate_io::step_read::parse_step(&text(), &stem).map_err(|e| e.to_string())?
        }
        "stl" => {
            kreate_io::stl::parse_stl(bytes, &Default::default()).map_err(|e| e.to_string())?
        }
        "obj" => {
            kreate_io::obj::parse_obj(&text(), Default::default()).map_err(|e| e.to_string())?
        }
        "glb" | "gltf" => kreate_io::gltf_import::parse_gltf(bytes, &Default::default())
            .map_err(|e| e.to_string())?,
        "3mf" => kreate_io::threemf::parse_3mf(bytes).map_err(|e| e.to_string())?,
        "ply" => kreate_io::ply::parse_ply(bytes, &stem).map_err(|e| e.to_string())?,
        _ => return Err(format!("읽을 수 있는 파일은 {} 입니다", FORMATS.join(", "))),
    };
    if let Some(u) = unit {
        scene.source.unit_scale = u as f32;
    }
    let mut notes: Vec<String> = scene
        .source
        .notes
        .iter()
        .map(|n| format!("{n:?}"))
        .collect();
    let mut doc = Document::new();
    let (_, built) = kreate_io::build::build_into(&scene, &mut doc);
    notes.extend(built.iter().map(|n| format!("{n:?}")));

    // Its size and where it lies: every vertex, where the file put it.
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    let mut triangles = 0usize;
    for (id, o) in doc.objects() {
        let Some(mesh) = o.mesh().and_then(|m| doc.mesh(m)) else {
            continue;
        };
        triangles += mesh.triangulate().indices.len() / 3;
        let world = doc.world_transform(id);
        for p in mesh.positions() {
            let p = world.transform_point3(p.as_dvec3());
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    if !lo.is_finite() {
        return Err("파일에 모양이 없습니다".into());
    }
    if triangles > MOST_TRIANGLES {
        return Err(format!(
            "삼각형이 {triangles}개로 너무 많습니다 ({MOST_TRIANGLES}개까지). CAD에서 거칠게 내보내세요"
        ));
    }
    let size = hi - lo;
    if size.max_element() > 1000.0 || size.max_element() < 1e-4 {
        return Err(format!(
            "크기가 {:.3} × {:.3} × {:.3} m 입니다. 단위를 확인하세요",
            size.x, size.y, size.z
        ));
    }

    // Under one object, on the floor at its middle: the parts as they were,
    // all of them moved together.
    let tops: Vec<_> = doc
        .hierarchy()
        .into_iter()
        .filter(|(_, depth)| *depth == 0)
        .map(|(id, _)| id)
        .collect();
    let root = doc.add_object(Object::new(stem.clone(), ObjectData::Empty));
    let shift = doc.add_object(Object::new(format!("{stem} 형상"), ObjectData::Empty));
    doc.set_parent(shift, Some(root))
        .map_err(|e| format!("{e:?}"))?;
    for id in tops {
        doc.set_parent(id, Some(shift))
            .map_err(|e| format!("{e:?}"))?;
    }
    let middle = DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z);
    doc.object_mut(shift)
        .ok_or("no shift")?
        .transform
        .translation = -middle;
    let root = doc.object_uid(root).ok_or("no root")?.raw();
    Ok(Model {
        doc,
        root,
        name: stem,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box `size` (mm) with its corner at `at` (mm), as an ASCII STL.
    fn stl_box(size: [f64; 3], at: [f64; 3]) -> Vec<u8> {
        let c = |i: usize| -> [f64; 3] {
            [
                at[0] + if i & 1 != 0 { size[0] } else { 0.0 },
                at[1] + if i & 2 != 0 { size[1] } else { 0.0 },
                at[2] + if i & 4 != 0 { size[2] } else { 0.0 },
            ]
        };
        let faces = [
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ];
        let mut out = String::from("solid box\n");
        for f in faces {
            out.push_str(" facet normal 0 0 0\n  outer loop\n");
            for i in f {
                let p = c(i);
                out.push_str(&format!("   vertex {} {} {}\n", p[0], p[1], p[2]));
            }
            out.push_str("  endloop\n endfacet\n");
        }
        out.push_str("endsolid box\n");
        out.into_bytes()
    }

    /// The bounds of everything below `root`, in the model's own scene.
    fn bounds(m: &Model) -> (DVec3, DVec3) {
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for (id, o) in m.doc.objects() {
            let Some(mesh) = o.mesh().and_then(|x| m.doc.mesh(x)) else {
                continue;
            };
            let w = m.doc.world_transform(id);
            for p in mesh.positions() {
                let p = w.transform_point3(p.as_dvec3());
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        (lo, hi)
    }

    /// A box drawn in millimetres away from the origin: read in metres,
    /// standing on the floor at its middle, under one object of its name.
    #[test]
    fn a_model_stands_on_the_floor_at_its_middle() {
        let bytes = stl_box([100.0, 200.0, 50.0], [1000.0, 2000.0, 300.0]);
        let m = read("받침 블록.stl", &bytes, None).unwrap();
        assert_eq!(m.name, "받침 블록");
        let (lo, hi) = bounds(&m);
        assert!((lo - DVec3::new(-0.05, -0.1, 0.0)).length() < 1e-6, "{lo}");
        assert!((hi - DVec3::new(0.05, 0.1, 0.05)).length() < 1e-6, "{hi}");
        let root = m
            .doc
            .object_by_uid(kreate_scene::Uid::from_raw(m.root))
            .unwrap();
        assert_eq!(m.doc.object(root).unwrap().name, "받침 블록");

        // In metres, as told: a thousand times as large.
        let big = read("block.stl", &bytes, Some(1.0)).unwrap();
        let (lo, hi) = bounds(&big);
        assert!(((hi - lo) - DVec3::new(100.0, 200.0, 50.0)).length() < 1e-3);
        // Kilometres: not a cell's.
        assert!(read("block.stl", &bytes, Some(1000.0)).is_err());
        // Inches.
        let inch = read(
            "block.stl",
            &stl_box([4.0, 2.0, 1.0], [0.0, 0.0, 0.0]),
            Some(0.0254),
        )
        .unwrap();
        let (lo, hi) = bounds(&inch);
        assert!(((hi - lo) - DVec3::new(0.1016, 0.0508, 0.0254)).length() < 1e-6);
    }

    /// A STEP file in millimetres and a glTF file in metres, as CAD and
    /// other tools write them: each read at its own unit, half a metre wide.
    #[test]
    fn step_and_gltf_files_are_read_at_their_units() {
        let cube = kreate_mesh::primitives::cube(0.5);
        let at = kreate_math::DAffine3::from_translation(DVec3::new(3.0, 0.0, 1.0));
        let step = kreate_io::step::write_step("cube", &[("cube".into(), &cube, at)], 1e-3);
        let m = read("cube.step", step.as_bytes(), None).unwrap();
        let (lo, hi) = bounds(&m);
        assert!(((hi - lo) - DVec3::splat(0.5)).length() < 1e-6, "{lo} {hi}");
        assert!(
            lo.z.abs() < 1e-9 && (lo.x + 0.25).abs() < 1e-9,
            "on the floor at its middle: {lo}"
        );

        let (gltf, _) =
            kreate_io::gltf_export::write_gltf(&[("cube".into(), &cube)], &[], &Default::default());
        let m = read("cube.glb", &gltf, None).unwrap();
        let (lo, hi) = bounds(&m);
        assert!(((hi - lo) - DVec3::splat(0.5)).length() < 1e-6, "{lo} {hi}");
    }

    #[test]
    fn what_is_not_a_model_is_refused() {
        assert!(read("notes.txt", b"hello", None).is_err());
        assert!(read("broken.stl", b"solid x\nendsolid x\n", None).is_err());
        assert!(read("broken.glb", b"not gltf", None).is_err());
    }
}
