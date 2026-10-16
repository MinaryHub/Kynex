//! Robots of the user's own: a robot description (URDF) and the mesh files
//! it names, brought in as a zip of its folder — or a lone `.urdf` drawn with
//! shapes of its own — and kept in the project as they came, so that the
//! project opens anywhere. They are read as the library's robots are: by
//! Kreate's URDF reader, the meshes by `kreate_io::scene_file::read_part`
//! (STL and OBJ in metres with Z up, as ROS has them; COLLADA and glTF as
//! they say). The reader finds meshes on disk, so the zip is unpacked once
//! into `%APPDATA%\kynex\robots\<its hash>`.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use base64::Engine;
use kreate_scene::robot::RobotPart;
use kreate_scene::Document;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{Built, Kind};

/// The most a robot's files may come to, unpacked, and how many there may
/// be: a robot's meshes, not a disk's worth.
const MOST_BYTES: u64 = 512 << 20;
const MOST_FILES: usize = 5000;

/// A robot of the user's own, as imported.
#[derive(Clone, Debug, PartialEq)]
pub struct RobotModel {
    pub key: String,
    pub name: String,
    /// The description's path in the zip.
    urdf: String,
    /// The zip, as it came (or made of the lone `.urdf`).
    zip: Vec<u8>,
}

impl RobotModel {
    /// A robot from file `name`'s bytes: a zip of its folder, or a `.urdf`.
    /// Read through once here, so that what cannot be is refused now.
    pub fn from_file(key: String, name: &str, bytes: &[u8]) -> Result<Self, String> {
        let lower = name.to_ascii_lowercase();
        let (zip, urdf) = if lower.ends_with(".urdf") {
            let file = Path::new(name)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("robot.urdf")
                .to_string();
            (zip_of(&file, bytes)?, file)
        } else if lower.ends_with(".zip") {
            (bytes.to_vec(), description_in(bytes)?)
        } else {
            return Err("로봇은 URDF 폴더를 묶은 .zip이나 .urdf 파일로 가져옵니다".into());
        };
        let stem = Path::new(&urdf)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("로봇")
            .chars()
            .filter(|c| !c.is_control())
            .take(40)
            .collect();
        let model = Self {
            key,
            name: stem,
            urdf,
            zip,
        };
        let built = model.build()?;
        let robots = kynex_model::robots(&built.doc);
        let moving = robots
            .iter()
            .flat_map(|r| &r.joints)
            .filter(|j| j.kind.moves())
            .count();
        if robots.is_empty() || moving == 0 {
            return Err("움직이는 관절이 있는 로봇이 아닙니다".into());
        }
        Ok(model)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "key": self.key,
            "name": self.name,
            "urdf": self.urdf,
            "zip": base64::engine::general_purpose::STANDARD.encode(&self.zip),
        })
    }

    pub fn of_json(v: &Value) -> Option<Self> {
        Some(Self {
            key: v["key"].as_str()?.to_string(),
            name: v["name"].as_str()?.to_string(),
            // A path within the zip, as one was found there: nothing else.
            urdf: v["urdf"].as_str().filter(|u| within(u))?.to_string(),
            zip: base64::engine::general_purpose::STANDARD
                .decode(v["zip"].as_str()?)
                .ok()?,
        })
    }

    /// The robot, made from its description: its root link the robot's root,
    /// its grip points (`grip_a`, `grip_b`) grippers, as the library's are.
    pub(super) fn build(&self) -> Result<Built, String> {
        // Only the robot's own folder is read: its description, and the
        // meshes it names — never a file elsewhere, whatever a project file
        // or a description says (`../`, `file://`, a package found above).
        let dir = self.folder()?;
        let root = dir
            .canonicalize()
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        let inside = |path: &Path| -> Result<PathBuf, String> {
            let real = path
                .canonicalize()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if real.starts_with(&root) {
                Ok(real)
            } else {
                Err(format!(
                    "{}: 로봇 폴더 밖의 파일은 읽지 않습니다",
                    path.display()
                ))
            }
        };
        let path = inside(&dir.join(&self.urdf))?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", self.urdf))?;
        let base = path.parent().unwrap_or(&root).to_path_buf();
        let load = |part: &Path| kreate_io::scene_file::read_part(&inside(part)?);
        let scene = kreate_io::urdf::parse_urdf(&text, &base, &load).map_err(|e| e.to_string())?;
        let mut doc = Document::new();
        kreate_io::build::build_into(&scene, &mut doc);
        let root_link = doc
            .objects()
            .find(|(_, o)| o.parent.is_none() && matches!(o.robot, Some(RobotPart::Link { .. })))
            .map(|(id, _)| id);
        let roles = doc
            .objects()
            .filter(|(_, o)| o.name == "grip_a" || o.name == "grip_b")
            .map(|(id, _)| (id, Kind::Gripper))
            .collect();
        let order = doc.hierarchy().into_iter().map(|(id, _)| id).collect();
        Ok(Built {
            doc,
            order,
            roles,
            root_link,
            props: Vec::new(),
        })
    }

    /// Where its files are on disk: unpacked once, by what they hold.
    fn folder(&self) -> Result<PathBuf, String> {
        let hash: String = Sha256::digest(&self.zip)
            .iter()
            .take(12)
            .map(|b| format!("{b:02x}"))
            .collect();
        // Tests leave the user's settings alone.
        let home = if cfg!(test) {
            std::env::temp_dir().join("kynex-test")
        } else {
            crate::paths::app_dir()
        };
        let dir = home.join("robots").join(hash);
        if dir.join(".done").is_file() {
            return Ok(dir);
        }
        let _ = std::fs::remove_dir_all(&dir);
        unpack(&self.zip, &dir)?;
        std::fs::write(dir.join(".done"), b"").map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(dir)
    }
}

/// Whether `path` names a file within a folder: relative, with no `..`.
fn within(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// The robot description in a zip: the `.urdf` nearest its top, if there
/// is one. A `.xacro` alone is refused: it must be made a URDF first.
fn description_in(zip: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| format!("zip: {e}"))?;
    let mut found: Vec<String> = Vec::new();
    let mut xacro = false;
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| format!("zip: {e}"))?;
        let Some(path) = file.enclosed_name() else {
            continue;
        };
        let lower = path.to_string_lossy().to_ascii_lowercase();
        if lower.ends_with(".urdf") {
            found.push(path.to_string_lossy().replace('\\', "/"));
        } else if lower.ends_with(".xacro") {
            xacro = true;
        }
    }
    found.sort_by_key(|p| (p.matches('/').count(), p.clone()));
    found.into_iter().next().ok_or_else(|| {
        if xacro {
            "zip에 .xacro만 있습니다. xacro로 URDF를 만들어 함께 넣으세요".into()
        } else {
            "zip에 .urdf 파일이 없습니다".into()
        }
    })
}

/// A zip of one file.
fn zip_of(name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    out.start_file(name, zip::write::SimpleFileOptions::default())
        .map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut out, bytes).map_err(|e| e.to_string())?;
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// `zip` unpacked into `dir`: each file within it (none outside, whatever
/// its name says), within [`MOST_FILES`] and [`MOST_BYTES`].
fn unpack(zip: &[u8], dir: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| format!("zip: {e}"))?;
    if archive.len() > MOST_FILES {
        return Err(format!("zip의 파일이 {MOST_FILES}개를 넘습니다"));
    }
    let mut total = 0u64;
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| format!("zip: {e}"))?;
        // A name leading out of the folder is not followed.
        let Some(inside) = file.enclosed_name() else {
            continue;
        };
        let to = dir.join(inside);
        if file.is_dir() {
            std::fs::create_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        // Counted as read, not as the zip says: a zip can lie.
        let mut bytes = Vec::new();
        file.take(MOST_BYTES - total + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("zip: {e}"))?;
        total += bytes.len() as u64;
        if total > MOST_BYTES {
            return Err(format!("zip의 내용이 {} MB를 넘습니다", MOST_BYTES >> 20));
        }
        std::fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-link arm as a ROS package would have it: a description naming
    /// its meshes by `package://` and by a path beside it, the meshes STL.
    fn arm_zip(xacro_only: bool) -> Vec<u8> {
        let stl = |s: f64| {
            let c = [[0., 0., 0.], [s, 0., 0.], [0., s, 0.], [0., 0., s]];
            let f = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
            let mut out = String::from(
                "solid p
",
            );
            for t in f {
                out.push_str(
                    " facet normal 0 0 0
  outer loop
",
                );
                for i in t {
                    out.push_str(&format!(
                        "   vertex {} {} {}
",
                        c[i][0], c[i][1], c[i][2]
                    ));
                }
                out.push_str(
                    "  endloop
 endfacet
",
                );
            }
            out + "endsolid p
"
        };
        let urdf = r#"<robot name="arm2">
  <link name="base_link"><visual><geometry><mesh filename="package://arm2/meshes/base.stl"/></geometry></visual></link>
  <link name="upper"><visual><geometry><mesh filename="../meshes/upper.stl"/></geometry></visual></link>
  <joint name="j1" type="revolute">
    <parent link="base_link"/><child link="upper"/>
    <origin xyz="0 0 0.3"/><axis xyz="0 0 1"/>
    <limit lower="-3" upper="3" effort="10" velocity="1"/>
  </joint>
</robot>"#;
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, bytes: &[u8]| {
            out.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut out, bytes).unwrap();
        };
        if xacro_only {
            put("arm2/urdf/arm2.urdf.xacro", urdf.as_bytes());
        } else {
            put("arm2/urdf/arm2.urdf", urdf.as_bytes());
        }
        put("arm2/meshes/base.stl", stl(0.2).as_bytes());
        put("arm2/meshes/upper.stl", stl(0.1).as_bytes());
        // A name leading out of the folder: never written there.
        put("../escape.txt", b"no");
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn a_robot_comes_in_a_zip_with_its_meshes() {
        let model = RobotModel::from_file("robot_1".into(), "arm2.zip", &arm_zip(false)).unwrap();
        assert_eq!(model.name, "arm2");
        let built = model.build().unwrap();
        let robots = kynex_model::robots(&built.doc);
        assert_eq!(robots.len(), 1);
        assert_eq!(
            robots[0].joints.iter().filter(|j| j.kind.moves()).count(),
            1
        );
        // Both meshes found: the package's and the one beside it.
        let meshes = built
            .doc
            .objects()
            .filter(|(_, o)| o.mesh().is_some())
            .count();
        assert!(meshes >= 2, "{meshes} meshes");
        assert!(built.root_link.is_some());
        // Kept and read back whole.
        let back = RobotModel::of_json(&model.to_json()).unwrap();
        assert_eq!(back, model);
        // Nothing written outside its folder.
        assert!(!model
            .folder()
            .unwrap()
            .parent()
            .unwrap()
            .join("escape.txt")
            .exists());
    }

    /// A description naming a mesh outside its folder — by `../`, or by an
    /// absolute `file://` path — and a project naming a description outside
    /// it: nothing outside is read.
    #[test]
    fn nothing_outside_the_robot_s_folder_is_read() {
        let outside = std::env::temp_dir().join(format!("kynex-secret-{}.stl", std::process::id()));
        std::fs::write(
            &outside,
            "solid s
 facet normal 0 0 0
  outer loop
   vertex 0 0 0
   vertex 1 0 0
   vertex 0 1 0
  endloop
 endfacet
endsolid s
",
        )
        .unwrap();
        let url = format!("file://{}", outside.display()).replace('\\', "/");
        for mesh in ["../../../../../../secret.stl".to_string(), url] {
            let urdf = format!(
                r#"<robot name="sneaky"><link name="a"><visual><geometry><mesh filename="{mesh}"/></geometry></visual></link><link name="b"/>
                <joint name="j" type="revolute"><parent link="a"/><child link="b"/><axis xyz="0 0 1"/><limit lower="-1" upper="1" effort="1" velocity="1"/></joint></robot>"#
            );
            let model = RobotModel::from_file("r".into(), "sneaky.urdf", urdf.as_bytes());
            // Refused, or made without the file: never with it.
            if let Ok(model) = model {
                let built = model.build().unwrap();
                assert_eq!(
                    built
                        .doc
                        .objects()
                        .filter(|(_, o)| o.mesh().is_some())
                        .count(),
                    0,
                    "{mesh}"
                );
            }
        }
        let mut v = RobotModel::from_file("r".into(), "arm2.zip", &arm_zip(false))
            .unwrap()
            .to_json();
        for bad in ["../../secret.urdf", "C:/Windows/win.ini", "/etc/passwd", ""] {
            v["urdf"] = json!(bad);
            assert!(RobotModel::of_json(&v).is_none(), "{bad}");
        }
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn what_is_no_robot_is_refused() {
        let xacro = RobotModel::from_file("r".into(), "arm2.zip", &arm_zip(true));
        assert!(xacro.unwrap_err().contains("xacro"));
        assert!(RobotModel::from_file("r".into(), "arm2.zip", b"not a zip").is_err());
        let fixed = r#"<robot name="post"><link name="a"/></robot>"#;
        assert!(RobotModel::from_file("r".into(), "post.urdf", fixed.as_bytes()).is_err());
        assert!(RobotModel::from_file("r".into(), "arm.step", b"x").is_err());
    }
}
