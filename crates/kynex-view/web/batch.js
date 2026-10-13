// Every part of the scene in one THREE.BatchedMesh: one draw call however
// many parts there are, each part still its own instance with its own
// matrix, colour and visibility.
//
// Measured before this (RTX 4060 Ti): one Mesh and one edge LineSegments per
// part made 10,000 draw calls at 5,000 parts and 15 fps, 40,000 at 20,000
// parts and 2.5 fps. The GPU was idle; the browser was spending the frame
// submitting draws. A factory is tens of thousands of parts.
//
// Geometry is kept indexed and without normals: the material shades flat
// from screen-space derivatives, which is how a CAD part reads and a third
// of the vertex memory a faceted copy would take.

import * as THREE from 'three';

const GROW = 1.5;

export class Batch {
  constructor(material) {
    this.material = material;
    this.mesh = null;
    this.geoms = new Map(); // mesh uid -> { id, verts, indices, triangles }
    this.inst = new Map(); // object uid -> { id, mesh }
    this.owner = []; // instance id -> object uid
    this.base = new Map(); // object uid -> THREE.Color, before any highlight
    this.maxVerts = 0;
    this.maxIndices = 0;
    this.holes = 0; // vertices left behind by replaced geometry
    this._m = new THREE.Matrix4();
  }

  // Starts again with room for `meshes` (mesh uid -> BufferGeometry) and
  // about `instances` parts.
  reset(meshes, instances) {
    this.dispose();
    let verts = 0;
    let indices = 0;
    for (const g of meshes.values()) {
      verts += g.attributes.position.count;
      indices += g.index.count;
    }
    this.maxVerts = Math.max(1024, Math.ceil(verts * 1.1));
    this.maxIndices = Math.max(3072, Math.ceil(indices * 1.1));
    const mesh = new THREE.BatchedMesh(Math.max(64, Math.ceil(instances * 1.1)), this.maxVerts, this.maxIndices, this.material);
    mesh.perObjectFrustumCulled = true;
    // Opaque parts: sorting them would cost a sort of every part a frame for
    // a little less overdraw.
    mesh.sortObjects = false;
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    mesh.frustumCulled = false;
    this.mesh = mesh;
    for (const [uid, g] of meshes) this.addGeometry(uid, g);
  }

  dispose() {
    this.mesh?.dispose();
    this.mesh?.removeFromParent();
    this.mesh = null;
    this.geoms.clear();
    this.inst.clear();
    this.owner = [];
    this.base.clear();
    this.holes = 0;
  }

  hasGeometry(uid) {
    return this.geoms.has(uid);
  }

  addGeometry(uid, geometry) {
    const verts = geometry.attributes.position.count;
    const indices = geometry.index.count;
    if (!verts || !indices) return;
    this._room(verts, indices);
    const id = this.mesh.addGeometry(geometry);
    this.geoms.set(uid, { id, verts, indices, triangles: indices / 3 });
  }

  // A mesh whose geometry was edited: the new one goes in, every part that
  // showed the old one shows it, and the old one's room is let go.
  replaceGeometry(uid, geometry) {
    const old = this.geoms.get(uid);
    this.geoms.delete(uid);
    this.addGeometry(uid, geometry);
    const fresh = this.geoms.get(uid);
    for (const [objUid, it] of this.inst) {
      if (it.mesh !== uid) continue;
      if (fresh) this.mesh.setGeometryIdAt(it.id, fresh.id);
      else this.remove(objUid);
    }
    if (old) {
      this.mesh.deleteGeometry(old.id);
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
      it = { id: this.mesh.addInstance(g.id), mesh: meshUid };
      this.inst.set(uid, it);
      this.owner[it.id] = uid;
    } else if (it.mesh !== meshUid) {
      this.mesh.setGeometryIdAt(it.id, g.id);
      it.mesh = meshUid;
    }
    this.mesh.setMatrixAt(it.id, this._m.fromArray(matrix));
    this.base.set(uid, color.clone());
    this.mesh.setColorAt(it.id, color);
    this.mesh.setVisibleAt(it.id, visible);
  }

  move(uid, matrix) {
    const it = this.inst.get(uid);
    if (it) this.mesh.setMatrixAt(it.id, this._m.fromArray(matrix));
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

  uidAt(instanceId) {
    return this.owner[instanceId];
  }

  // Part `uid`'s box in the world, into `box`; false when it has none.
  boxOf(uid, box) {
    const it = this.inst.get(uid);
    if (!it) return false;
    const g = this.geoms.get(it.mesh);
    this.mesh.getBoundingBoxAt(g.id, box);
    this.mesh.getMatrixAt(it.id, this._m);
    box.applyMatrix4(this._m);
    return true;
  }

  // Every visible part's box, into `box`.
  boxAll(box) {
    if (!this.mesh || !this.inst.size) return false;
    this.mesh.computeBoundingBox();
    box.union(this.mesh.boundingBox);
    return true;
  }

  triangles() {
    let n = 0;
    for (const it of this.inst.values()) n += this.geoms.get(it.mesh)?.triangles ?? 0;
    return n;
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
