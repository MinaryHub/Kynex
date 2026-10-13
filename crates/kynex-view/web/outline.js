// Edges drawn from the picture rather than from geometry: one pass renders
// the parts' normals and depth, and a full-screen pass darkens the pixels
// where either jumps — a silhouette, a crease, one part in front of another.
//
// Edge lines per part were one draw call each, half of every frame's
// calls; this is two calls whatever the scene holds. It also finds edges
// the geometry never had, where two parts meet.

import * as THREE from 'three';

const VERTEX = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}`;

const FRAGMENT = /* glsl */ `
uniform sampler2D tNormal;
uniform sampler2D tDepth;
uniform vec2 texel;
uniform float near;
uniform float far;
uniform vec3 color;
uniform float strength;
varying vec2 vUv;

// The distance in front of the camera a depth-buffer value stands for.
float distanceAt(vec2 uv) {
  float d = texture2D(tDepth, uv).x;
  return (near * far) / (far - d * (far - near));
}

void main() {
  float d0 = texture2D(tDepth, vUv).x;
  vec3 n0 = texture2D(tNormal, vUv).xyz * 2.0 - 1.0;
  float z0 = distanceAt(vUv);
  float edge = 0.0;
  bool anything = d0 < 1.0;
  vec2 offsets[4];
  offsets[0] = vec2(texel.x, 0.0);
  offsets[1] = vec2(-texel.x, 0.0);
  offsets[2] = vec2(0.0, texel.y);
  offsets[3] = vec2(0.0, -texel.y);
  for (int i = 0; i < 4; i++) {
    vec2 uv = vUv + offsets[i];
    float d = texture2D(tDepth, uv).x;
    anything = anything || d < 1.0;
    float z = distanceAt(uv);
    // Depth: a step large for how far away it is.
    edge = max(edge, smoothstep(0.015, 0.05, abs(z - z0) / min(z, z0)));
    // Normals: a crease, on the same surface depth.
    if (d0 < 1.0 && d < 1.0) {
      vec3 n = texture2D(tNormal, uv).xyz * 2.0 - 1.0;
      edge = max(edge, smoothstep(0.2, 0.5, 1.0 - dot(n0, n)));
    }
  }
  if (!anything || edge < 0.01) discard;
  gl_FragColor = vec4(color, edge * strength);
}`;

export class Outline {
  constructor() {
    this.target = new THREE.WebGLRenderTarget(1, 1, {
      depthTexture: new THREE.DepthTexture(1, 1),
      minFilter: THREE.NearestFilter,
      magFilter: THREE.NearestFilter,
    });
    this.normals = new THREE.MeshNormalMaterial({ flatShading: true });
    this.material = new THREE.ShaderMaterial({
      vertexShader: VERTEX,
      fragmentShader: FRAGMENT,
      uniforms: {
        tNormal: { value: this.target.texture },
        tDepth: { value: this.target.depthTexture },
        texel: { value: new THREE.Vector2(1, 1) },
        near: { value: 0.01 },
        far: { value: 2000 },
        color: { value: new THREE.Color(0x0b0d10) },
        strength: { value: 0.55 },
      },
      transparent: true,
      depthTest: false,
      depthWrite: false,
    });
    this.quad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), this.material);
    this.quad.frustumCulled = false;
    this.quadScene = new THREE.Scene();
    this.quadScene.add(this.quad);
    this.quadCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
    this.enabled = true;
  }

  setSize(width, height) {
    this.target.setSize(width, height);
    this.material.uniforms.texel.value.set(1 / width, 1 / height);
  }

  // Renders the normals and depth of what `layer` holds. Call before the
  // scene itself is drawn.
  prepare(renderer, scene, camera, layer) {
    if (!this.enabled) return;
    const mask = camera.layers.mask;
    const background = scene.background;
    camera.layers.set(layer);
    scene.overrideMaterial = this.normals;
    scene.background = null;
    const clear = renderer.getClearColor(new THREE.Color());
    const alpha = renderer.getClearAlpha();
    renderer.setClearColor(0x808080, 0);
    renderer.setRenderTarget(this.target);
    renderer.clear();
    renderer.render(scene, camera);
    renderer.setRenderTarget(null);
    renderer.setClearColor(clear, alpha);
    scene.overrideMaterial = null;
    scene.background = background;
    camera.layers.mask = mask;
    this.material.uniforms.near.value = camera.near;
    this.material.uniforms.far.value = camera.far;
  }

  // Lays the edges over what was drawn.
  draw(renderer) {
    if (!this.enabled) return;
    const auto = renderer.autoClear;
    renderer.autoClear = false;
    renderer.render(this.quadScene, this.quadCamera);
    renderer.autoClear = auto;
  }
}
