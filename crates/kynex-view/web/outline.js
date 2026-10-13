// The scene is drawn once into a target, and then onto the screen with its
// edges darkened: wherever the depth jumps — a silhouette, one part in
// front of another, a part on the floor.
//
// Edges used to come from a second pass that drew every part's normals.
// With 50,000 parts that pass cost more than the scene itself (RTX 4060 Ti,
// Chrome on Direct3D 11: 43 fps with it, 84 without), because a pass over a
// BatchedMesh there costs by the part, not by the triangle. Depth from the
// one pass finds the same silhouettes; creases show in the flat shading.

import * as THREE from 'three';

const VERTEX = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}`;

const FRAGMENT = /* glsl */ `
uniform sampler2D tColor;
uniform sampler2D tDepth;
uniform vec2 texel;
uniform float near;
uniform float far;
uniform vec3 edgeColor;
uniform float strength;
varying vec2 vUv;

// The distance in front of the camera a depth-buffer value stands for.
float distanceAt(vec2 uv) {
  float d = texture2D(tDepth, uv).x;
  return (near * far) / (far - d * (far - near));
}

void main() {
  vec3 color = texture2D(tColor, vUv).rgb;
  float d0 = texture2D(tDepth, vUv).x;
  // The background is the clear colour as it was chosen: tone mapping is for
  // the lit scene, and turned the grey to navy.
  #ifdef TONE_MAPPING
  if (d0 < 1.0) color = toneMapping(color);
  #endif
  float z0 = distanceAt(vUv);
  float edge = 0.0;
  if (strength > 0.0) {
    float zl = distanceAt(vUv - vec2(texel.x, 0.0));
    float zr = distanceAt(vUv + vec2(texel.x, 0.0));
    float zd = distanceAt(vUv - vec2(0.0, texel.y));
    float zu = distanceAt(vUv + vec2(0.0, texel.y));
    // A step large for how far away it is, and not the steady slope of a
    // surface seen at a glancing angle: the two sides disagree.
    float dx = abs(zl + zr - 2.0 * z0);
    float dy = abs(zd + zu - 2.0 * z0);
    edge = smoothstep(0.004, 0.02, max(dx, dy) / z0);
  }
  gl_FragColor = vec4(mix(color, edgeColor, edge * strength), 1.0);
  #include <colorspace_fragment>
}`;

export class Outline {
  constructor() {
    this.target = new THREE.WebGLRenderTarget(1, 1, {
      // Light before tone mapping, which the composite applies.
      type: THREE.HalfFloatType,
      samples: 4,
      depthTexture: new THREE.DepthTexture(1, 1),
    });
    this.material = new THREE.ShaderMaterial({
      vertexShader: VERTEX,
      fragmentShader: FRAGMENT,
      uniforms: {
        tColor: { value: this.target.texture },
        tDepth: { value: this.target.depthTexture },
        texel: { value: new THREE.Vector2(1, 1) },
        near: { value: 0.01 },
        far: { value: 2000 },
        // A hex colour is taken as sRGB and kept linear, as the scene is
        // before its colour space is applied.
        edgeColor: { value: new THREE.Color(0x0b0d10) },
        strength: { value: 0.55 },
      },
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

  // Draws `scene` once, and onto the screen with its edges.
  render(renderer, scene, camera) {
    this.material.uniforms.near.value = camera.near;
    this.material.uniforms.far.value = camera.far;
    this.material.uniforms.strength.value = this.enabled ? 0.55 : 0;
    renderer.setRenderTarget(this.target);
    renderer.render(scene, camera);
    renderer.setRenderTarget(null);
    renderer.render(this.quadScene, this.quadCamera);
  }
}
