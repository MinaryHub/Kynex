// Every part of the scene in one THREE.BatchedMesh: one draw call however
// many parts there are, each part still its own instance with its own
// matrix, colour and visibility.
//
// Measured before this (RTX 4060 Ti): one Mesh and one edge LineSegments per
// part made 10,000 draw calls at 5,000 parts and 15 fps, 40,000 at 20,000
// parts and 2.5 fps. The GPU was idle; the browser was spending the frame
// submitting draws. A factory is tens of thousands of parts.
//
// Each mesh comes in up to three levels of detail, and each part is drawn
// with the one its size on screen calls for — or not at all when it covers
// less than a couple of pixels. Seen whole, a factory is mostly parts a few
// pixels across; drawn in full they were most of the frame's triangles.
//
// Geometry is kept indexed and without normals: the material shades flat
// from screen-space derivatives, which is how a CAD part reads and a third
// of the vertex memory a faceted copy would take.

import * as THREE from 'three';

const GROW = 1.5;

// Screen sizes, in pixels across, at which a part steps down a level, and
// below which it is not drawn. Hiding only what is under a pixel: a factory
// seen whole is mostly parts one or two pixels across, and hiding those too
// left an empty screen.
const LEVEL_PX = [120, 25];
const HIDE_PX = 0.75;

export class Batch {
  constructor(material) {
    this.material = material;
    this.mesh = null;
    this.geoms = new Map(); // mesh uid -> { ids: [geometry id per level], triangles: [...], center, radius }
    this.inst = new Map(); // object uid -> { id, mesh, level, shown, small, center, radius }
    this.owner = []; // instance id -> object uid
    this.base = new Map(); // object uid -> THREE.Color, before any highlight
    this.hidden = new Set(); // object uids hidden on this page alone
    this.sources = new Map(); // mesh uid -> its finest BufferGeometry, to draw a part again on its own
    this.maxVerts = 0;
    this.maxIndices = 0;
    this.holes = 0; // vertices left behind by replaced geometry
    this._m = new THREE.Matrix4();
    this._v = new THREE.Vector3();
  }

  // Starts again with room for `meshes` (mesh uid -> [BufferGeometry per
  // level, finest first]) and about `instances` parts.
  reset(meshes, instances) {
    this.dispose();
    let verts = 0;
    let indices = 0;
    for (const levels of meshes.values()) {
      for (const g of levels) {
        verts += g.attributes.position.count;
        indices += g.index.count;
      }
    }
    this.maxVerts = Math.max(1024, Math.ceil(verts * 1.1));
    this.maxIndices = Math.max(3072, Math.ceil(indices * 1.1));
    const mesh = new THREE.BatchedMesh(Math.max(64, Math.ceil(instances * 1.1)), this.maxVerts, this.maxIndices, this.material);
    // No culling or sorting per part in JavaScript: with either on, three.js
    // tests every part and rebuilds the draw list on every pass of every
    // frame — 50,000 parts, three passes. The GPU clips what is off screen
    // for less than that costs, and the draw list is rebuilt only when a
    // part's level or visibility changes.
    mesh.perObjectFrustumCulled = false;
    mesh.sortObjects = false;
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    mesh.frustumCulled = false;
    this.mesh = mesh;
    for (const [uid, levels] of meshes) this.addGeometry(uid, levels);
  }

  dispose() {
    this.mesh?.dispose();
    this.mesh?.removeFromParent();
    this.mesh = null;
    this.geoms.clear();
    this.inst.clear();
    this.owner = [];
    this.base.clear();
    this.sources.clear();
    this.holes = 0;
  }

  hasGeometry(uid) {
    return this.geoms.has(uid);
  }

  addGeometry(uid, levels) {
    const usable = levels.filter((g) => g.attributes.position.count && g.index.count);
    if (!usable.length) return;
    usable[0].computeBoundingSphere();
    const ids = [];
    const triangles = [];
    for (const g of usable) {
      this._room(g.attributes.position.count, g.index.count);
      ids.push(this.mesh.addGeometry(g));
      triangles.push(g.index.count / 3);
    }
    const s = usable[0].boundingSphere;
    this.sources.set(uid, usable[0]);
    this.geoms.set(uid, { ids, triangles, center: s.center.clone(), radius: s.radius, verts: usable.reduce((n, g) => n + g.attributes.position.count, 0) });
  }

  // A mesh whose geometry was edited: the new one goes in, every part that
  // showed the old one shows it, and the old one's room is let go.
  replaceGeometry(uid, levels) {
    const old = this.geoms.get(uid);
    this.geoms.delete(uid);
    this.addGeometry(uid, levels);
    const fresh = this.geoms.get(uid);
    for (const [objUid, it] of this.inst) {
      if (it.mesh !== uid) continue;
      if (!fresh) {
        this.remove(objUid);
        continue;
      }
      it.level = Math.min(it.level, fresh.ids.length - 1);
      this.mesh.setGeometryIdAt(it.id, fresh.ids[it.level]);
      this._measure(it, fresh);
    }
    if (old) {
      for (const id of old.ids) this.mesh.deleteGeometry(id);
      this.holes += old.verts;
      // Compact when most of the buffer is gaps.
      if (this.holes > this.maxVerts / 2) {
        this.mesh.optimize();
        this.holes = 0;
      }
    }
  }

  // Places part `uid` showing mesh `meshUid`: added if new, moved to the
  // other geometry if its mesh changed.
  set(uid, meshUid, matrix, color, visible) {
    const g = this.geoms.get(meshUid);
    if (!g) {
      this.remove(uid);
      return;
    }
    let it = this.inst.get(uid);
    if (!it) {
      if (this.mesh.instanceCount >= this.mesh.maxInstanceCount) {
        this.mesh.setInstanceCount(Math.ceil(this.mesh.maxInstanceCount * GROW));
      }
      it = { id: this.mesh.addInstance(g.ids[0]), mesh: meshUid, level: 0, shown: true, small: false, center: new THREE.Vector3(), radius: 0 };
      this.inst.set(uid, it);
      this.owner[it.id] = uid;
    } else if (it.mesh !== meshUid) {
      it.mesh = meshUid;
      it.level = 0;
      this.mesh.setGeometryIdAt(it.id, g.ids[0]);
    }
    this.mesh.setMatrixAt(it.id, this._m.fromArray(matrix));
    this._measure(it, g);
    this.base.set(uid, color.clone());
    this.mesh.setColorAt(it.id, color);
    it.shown = visible;
    this.mesh.setVisibleAt(it.id, visible && !it.small && !this.hidden.has(uid));
  }

  // Parts hidden from view on this page alone (the scene keeps them), or
  // shown again.
  hide(uids, hidden) {
    for (const uid of uids) {
      if (hidden) this.hidden.add(uid);
      else this.hidden.delete(uid);
      const it = this.inst.get(uid);
      if (it) this.mesh.setVisibleAt(it.id, it.shown && !it.small && !this.hidden.has(uid));
    }
  }

  move(uid, matrix) {
    const it = this.inst.get(uid);
    if (!it) return;
    this.mesh.setMatrixAt(it.id, this._m.fromArray(matrix));
    this._measure(it, this.geoms.get(it.mesh));
  }

  remove(uid) {
    const it = this.inst.get(uid);
    if (!it) return;
    this.mesh.deleteInstance(it.id);
    this.owner[it.id] = undefined;
    this.inst.delete(uid);
    this.base.delete(uid);
  }

  has(uid) {
    return this.inst.has(uid);
  }

  tint(uid, color) {
    const it = this.inst.get(uid);
    if (it) this.mesh.setColorAt(it.id, color ?? this.base.get(uid));
  }

  // Part `uid` as drawn — its geometry, where it is, its colour — to draw it
  // on its own, as a see-through copy does; null if it is not drawn.
  xrayCopy(uid) {
    const it = this.inst.get(uid);
    const geometry = it && this.sources.get(it.mesh);
    if (!geometry) return null;
    const matrix = new THREE.Matrix4();
    this.mesh.getMatrixAt(it.id, matrix);
    return { geometry, matrix, color: this.base.get(uid) };
  }

  uidAt(instanceId) {
    return this.owner[instanceId];
  }

  // Chooses each part's level from how large it is on screen: `camera` and
  // the drawing's height in pixels. Only parts whose level or visibility
  // changes are touched.
  updateLevels(camera, heightPx, keep) {
    if (!this.mesh) return;
    const perUnit = heightPx / (2 * Math.tan((camera.fov * Math.PI) / 360));
    const eye = camera.position;
    for (const [uid, it] of this.inst) {
      const g = this.geoms.get(it.mesh);
      const distance = Math.max(it.center.distanceTo(eye) - it.radius, 1e-6);
      const px = ((2 * it.radius) / distance) * perUnit;
      // The selected part stays whole and in view however small it is.
      const kept = uid === keep;
      let level = 0;
      if (!kept) while (level < g.ids.length - 1 && px < LEVEL_PX[level]) level++;
      const small = !kept && px < HIDE_PX;
      if (level !== it.level) {
        it.level = level;
        this.mesh.setGeometryIdAt(it.id, g.ids[level]);
      }
      if (small !== it.small) {
        it.small = small;
        this.mesh.setVisibleAt(it.id, it.shown && !small && !this.hidden.has(uid));
      }
    }
  }

  // Part `uid`'s box in the world, into `box`; false when it has none.
  boxOf(uid, box) {
    const it = this.inst.get(uid);
    if (!it) return false;
    const g = this.geoms.get(it.mesh);
    this.mesh.getBoundingBoxAt(g.ids[0], box);
    this.mesh.getMatrixAt(it.id, this._m);
    box.applyMatrix4(this._m);
    return true;
  }

  // Every part's box, into `box`.
  boxAll(box) {
    if (!this.mesh || !this.inst.size) return false;
    for (const it of this.inst.values()) {
      box.expandByPoint(this._v.copy(it.center).addScalar(it.radius));
      box.expandByPoint(this._v.copy(it.center).addScalar(-it.radius));
    }
    return true;
  }

  // Triangles in the parts drawn, at the levels they are drawn at.
  triangles() {
    let n = 0;
    for (const it of this.inst.values()) {
      if (it.shown && !it.small) n += this.geoms.get(it.mesh)?.triangles[it.level] ?? 0;
    }
    return n;
  }

  // Where part `it` stands and how large it is, in the world, from its
  // matrix and its mesh's bounding sphere.
  _measure(it, g) {
    this.mesh.getMatrixAt(it.id, this._m);
    it.center.copy(g.center).applyMatrix4(this._m);
    it.radius = g.radius * this._m.getMaxScaleOnAxis();
  }

  // Room for one more geometry of this size: the buffers grow by half
  // again, so a scene growing an edit at a time does not copy every time.
  _room(verts, indices) {
    if (this.mesh.unusedVertexCount >= verts && this.mesh.unusedIndexCount >= indices) return;
    this.maxVerts = Math.ceil(Math.max(this.maxVerts * GROW, this.maxVerts + verts));
    this.maxIndices = Math.ceil(Math.max(this.maxIndices * GROW, this.maxIndices + indices));
    this.mesh.setGeometrySize(this.maxVerts, this.maxIndices);
  }
}
