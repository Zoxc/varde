// The varde UI mock's shared part: the shell's chrome, the side panel, the
// viewport (drawn from a fixed camera) and the input they share. Each page
// (welcome.html, model.html, sketch.html) sets up `st`, fills in `page`, its
// hooks, and calls `start`:
//   welcome(): the welcome screen's content;
//   doOp(id): an operation from the toolbar, the Alt bar or the rail;
//   act(k, v): a data-act the shared part doesn't handle, true if it did;
//   escape(): Esc with nothing else to back out of;
//   faceClick(id): a click on a face, a hole or an origin plane;
//   afterRender(): anything to do once the page is drawn;
//   op: the solid operation being set up (`st.op`), on the model page:
//     label(), info(), pickHint() for the toolbar and the status bar,
//     preview(bodies, active) and panel() to draw it, pick(el) and
//     pickObject(id) for clicks in the view and the objects list.
// Pages link to each other with `go`, which keeps the theme.

// ---------------------------------------------------------------- icons
const I = {
  // tool sets (the rail): what each set leaves behind, in line style
  'cat-create': '<path class="r" d="M5 17.5a7 3 0 0 1 14 0" stroke-dasharray="1.6 1.8"/><path d="M5 6.5v11a7 3 0 0 0 14 0v-11"/><ellipse class="a" cx="12" cy="6.5" rx="7" ry="3"/>',
  'cat-modify': '<path d="M10.5 9H3.5v11h12v-6M3.5 9l5-5h7M20.5 9v6l-5 5M10.5 9l5-5M15.5 14l5-5"/><path class="a" d="M10.5 9a5 5 0 0 1 5 5M15.5 4a5 5 0 0 1 5 5"/>',
  'cat-transform': '<path class="r" stroke-dasharray="1.6 1.8" d="M7.5 10l4.76 2.75v5.5l-4.76 2.75-4.76-2.75v-5.5z"/><path class="r" stroke-dasharray="1.6 1.8" d="M2.74 12.75l4.76 2.75 4.76-2.75M7.5 15.5v5.5"/><path d="M16.5 3l4.76 2.75v5.5l-4.76 2.75-4.76-2.75v-5.5z"/><path d="M11.74 5.75l4.76 2.75 4.76-2.75M16.5 8.5v5.5"/>',
  'cat-construct': '<path d="M5.5 3.5l10 3.5v13.5l-10-3.5z"/><path class="a" d="M2.5 14l18-5"/><circle class="af" cx="10.5" cy="11.8" r="1.6"/>',
  'cat-inspect': '<path d="M12 3l7.8 4.5v9L12 21l-7.8-4.5v-9zM4.2 7.5L12 12l7.8-4.5M12 12v9"/><path class="a" d="M15.1 10.2l-2.7 10.4M18 8.5l-2.8 10.5M19.8 11l-1.4 5.3"/>',
  'cat-draw': '<path d="M5 20V10a7 7 0 0 1 14 0v10z"/><circle class="af" cx="5" cy="20" r="1.5"/><circle class="af" cx="19" cy="20" r="1.5"/><circle class="af" cx="12" cy="3" r="1.5"/>',
  'cat-smodify': '<path d="M2.5 9H15v12.5"/><path class="r" d="M19.5 9h2"/><path class="a" d="M17.25 5.5v7"/>',
  'cat-constrain': '<g transform="rotate(-18 12 12)"><rect x="5" y="6" width="14" height="12" rx=".5"/><path class="a" d="M5 13.5h4.5V18"/><circle class="af" cx="19" cy="6" r="1.5"/></g>',
  'cat-dim': '<rect x="3.5" y="13" width="17" height="7.5" rx="1"/><path class="r" d="M3.5 4.5v6.5M20.5 4.5v6.5"/><path class="a" d="M4.5 7.5h15M7 5.5l-2.5 2 2.5 2M17 5.5l2.5 2-2.5 2"/>',
  sketch: '<path class="r" d="M4 20h16"/><path class="t" d="M14.5 4.5l5 5L9 20H4v-5z"/><path class="a" d="M6 13l5 5"/>',
  extrude: '<path class="t" d="M4 15l8 4 8-4-8-4z"/><path class="a" d="M12 11V3M9 6l3-3 3 3"/>',
  revolve: '<path d="M20 12a8 8 0 1 1-2.6-5.9"/><path class="a" d="M17.5 2.5v4h4"/><path class="r" d="M12 7v10" stroke-dasharray="2 2"/>',
  hole: '<circle class="r" cx="12" cy="12" r="8"/><circle class="t" cx="12" cy="12" r="3.5"/><circle class="af" cx="12" cy="12" r="1.2"/>',
  // Fillet and chamfer: the model's are a block with its edge rounded (lines
  // along the round) or cut, the sketch's a corner with the fillet's centre
  // and radii or the chamfer's cut-off corner hatched (notes/ui-mock-fillet-chamfer.html, B).
  fillet: '<path d="M7.67 10L3.34 7.5v9L12 21.5v-4M3.34 7.5L12 2.5l4.33 2.5M12 21.5l8.66-5v-4"/><path class="a" d="M7.67 10C10.06 11.38 12 14.74 12 17.5M16.33 5C18.72 6.38 20.66 9.74 20.66 12.5M16.33 5L7.67 10M20.66 12.5L12 17.5"/><path class="a" stroke-width="1" opacity=".75" d="M18.49 6.92L9.84 11.92M20.08 9.67L11.42 14.67"/>',
  chamfer: '<path d="M7.67 10L3.34 7.5v9L12 21.5v-4M3.34 7.5L12 2.5l4.33 2.5M12 21.5l8.66-5v-4"/><path class="a" d="M7.67 10L12 17.5M16.33 5l4.33 7.5M16.33 5L7.67 10M20.66 12.5L12 17.5"/>',
  sfillet: '<path d="M4 20v-6M14 4h6"/><path class="r" d="M14 14H4M14 14V4" stroke-dasharray="1.6 2"/><path class="a" d="M4 14A10 10 0 0 1 14 4"/><circle class="af" cx="4" cy="14" r="1.5"/><circle class="af" cx="14" cy="4" r="1.5"/><circle class="af" cx="14" cy="14" r="1.3"/>',
  schamfer: '<path d="M4 20v-6M14 4h6"/><path class="r" d="M4 14V4h10" stroke-dasharray="1.6 2"/><path class="r" stroke-width=".9" d="M4 10l6-6M4 6l2-2"/><path class="a" d="M4 14L14 4"/><circle class="af" cx="4" cy="14" r="1.5"/><circle class="af" cx="14" cy="4" r="1.5"/>',
  combine: '<rect x="3" y="3" width="11" height="11" rx="1.5"/><rect x="10" y="10" width="11" height="11" rx="1.5"/><path class="ta" stroke="none" d="M10 10h4v4h-4z"/>',
  measure: '<path class="t" d="M3 17L17 3l4 4L7 21z"/><path class="a" d="M7 13l2 2M10 10l2 2M13 7l2 2"/>',
  pushpull: '<rect class="t" x="4" y="13" width="16" height="7" rx="1"/><path class="a" d="M12 10V3M9 6l3-3 3 3"/>',
  offset: '<rect class="t" x="3" y="3" width="18" height="18" rx="2"/><rect class="r" x="8" y="8" width="8" height="8" rx="1"/><path class="a" d="M16.5 12h2.5M17.5 10.5L19 12l-1.5 1.5"/>',
  line: '<path d="M6 18L18 6"/><circle class="a" cx="5" cy="19" r="1.6"/><circle class="a" cx="19" cy="5" r="1.6"/>',
  rect: '<rect class="t" x="4" y="6" width="16" height="12" rx="1"/><circle class="af" cx="4" cy="6" r="1.5"/><circle class="af" cx="20" cy="18" r="1.5"/>',
  circle: '<circle class="t" cx="12" cy="12" r="8"/><path class="r" d="M12 12l5.66-5.66" stroke-dasharray="2 2"/><circle class="af" cx="12" cy="12" r="1.5"/><circle class="af" cx="17.66" cy="6.34" r="1.5"/>',
  arc: '<path d="M4 18A9 9 0 0 1 20 18"/><circle class="af" cx="4" cy="18" r="1.5"/><circle class="af" cx="12" cy="10" r="1.5"/><circle class="af" cx="20" cy="18" r="1.5"/>',
  trim: '<circle cx="6" cy="7" r="2.5"/><circle cx="6" cy="17" r="2.5"/><path class="a" d="M8.2 8.4L20 17M8.2 15.6L20 7"/>',
  dim: '<path class="r" d="M4 7v10M20 7v10"/><path d="M5 12h14"/><path class="a" d="M7.5 9.5L5 12l2.5 2.5M16.5 9.5L19 12l-2.5 2.5"/>',
  constrain: '<rect class="t" x="5" y="11" width="14" height="9" rx="1.5"/><path class="a" d="M8 11V8a4 4 0 0 1 8 0v3"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7"/>',
  edit: '<path d="M14.5 4.5l5 5L9 20H4v-5z"/>',
  eye: '<path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6z"/><circle cx="12" cy="12" r="2.5"/>',
  eyeoff: '<path d="M3 3l18 18"/><path d="M10.6 6.1A9 9 0 0 1 12 6c6 0 9.5 6 9.5 6a16 16 0 0 1-2.7 3.3M6.6 7.6C4 9.3 2.5 12 2.5 12s3.5 6 9.5 6a9 9 0 0 0 3.4-.7"/>',
  rollback: '<path d="M4 4v16"/><path d="M20 12H8M12 8l-4 4 4 4"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
  undo: '<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>',
  redo: '<path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  // A new design: the plus, coloured as the file's icons are.
  new: '<path d="M12 5v14M5 12h14"/>',
  folder: '<path d="M3 7a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"/>',
  export: '<path class="a" d="M12 15V3M8 7l4-4 4 4"/><path d="M4 16v4h16v-4"/>',
  chev: '<path d="M7 10l5 5 5-5"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  moon: '<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/>',
  body: '<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/>',
  origin: '<path d="M5 19V5M5 19h14M5 19l8-8"/>',
  skoffset: '<path class="r" d="M4 20c2-7 7-12 16-13"/><path d="M3 13c2-4 6-7 11-7.8"/><path class="a" d="M10.5 14.5L8 11M7.5 13.3L8 11l2.4.2"/>',
  plane: '<path class="r" d="M3 20l4-5h14l-4 5z"/><path class="t" d="M3 11l4-5h14l-4 5z"/><path class="a" d="M12 17.5V9.5M10 11.5l2-2 2 2"/>',
  search: '<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>',
  help: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5V14"/><circle cx="12" cy="17" r=".6" fill="currentColor"/>',
  home: '<path d="M4 11l8-7 8 7"/><path d="M6 9.5V20h12V9.5"/>',
  more: '<circle cx="5.5" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="1.4" fill="currentColor" stroke="none"/>',
  ortho: '<path d="M3 9h12v12H3z"/><path d="M8 4h12v12h-5M3 9l5-5M15 9l5-5M15 21l5-5"/>',
  persp: '<path d="M3 9h12v12H3z"/><path d="M10 4h8v8h-3M3 9l7-5M15 9l3-5M15 21l3-9"/>',
  save: '<path d="M5 4h11l3 3v13H5z"/><path class="a" d="M8 4v5h7V4M8 20v-6h8v6"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
  min: '<path d="M6 12h12"/>',
  max: '<rect x="6" y="6" width="12" height="12" rx="1.5"/>',
  sweep: '<path class="r" d="M4 18c4 0 5-12 12-12"/><circle class="t" cx="17" cy="6" r="3"/><circle class="af" cx="4.5" cy="18" r="1.8"/>',
  loft: '<rect class="t" x="3" y="15" width="10" height="5" rx="1"/><circle class="t" cx="16" cy="6" r="3.5"/><path class="a" d="M3.5 14.5l9-8.5M13 15l6.3-6.8"/>',
  box: '<path class="t" d="M4 8l8-4 8 4v8l-8 4-8-4z"/><path d="M4 8l8 4 8-4M12 12v8"/><circle class="af" cx="12" cy="12" r="1.6"/>',
  shell: '<path class="t" d="M4 8l8-4 8 4v8l-8 4-8-4z"/><path class="a" d="M8 9.5l4-2 4 2v5l-4 2-4-2z"/>',
  move: '<rect class="t" x="9" y="9" width="6" height="6" rx="1"/><path class="a" d="M12 3v3.5M12 17.5V21M3 12h3.5M17.5 12H21M10 5l2-2 2 2M10 19l2 2 2-2M5 10l-2 2 2 2M19 10l2 2-2 2"/>',
  axis: '<path d="M4 20L20 4"/><circle class="af" cx="7.5" cy="16.5" r="1.7"/><circle class="af" cx="16.5" cy="7.5" r="1.7"/>',
  point: '<path d="M12 3v4M12 17v4M3 12h4M17 12h4"/><circle class="af" cx="12" cy="12" r="2.6"/>',
  section: '<path class="t" d="M4 8l8-4 8 4v8l-8 4-8-4z"/><path class="a" d="M3 13l18-4" stroke-dasharray="2 2"/>',
  polygon: '<path class="t" d="M12 3.5l8 5.8-3 9.2H7l-3-9.2z"/><path class="r" d="M12 12.2V5.5" stroke-dasharray="2 2"/><circle class="af" cx="12" cy="12.2" r="1.4"/><circle class="af" cx="12" cy="3.5" r="1.4"/>',
  spline: '<path d="M3 17c3-9 6-9 9-5s6 4 9-5"/><circle class="af" cx="3" cy="17" r="1.4"/><circle class="af" cx="12" cy="12" r="1.4"/><circle class="af" cx="21" cy="7" r="1.4"/>',
  extend: '<path d="M3 19l7-7"/><path class="a" d="M10 12l6.5-6.5" stroke-dasharray="2 2.2"/><path class="r" d="M13 2l8 8"/><circle class="af" cx="16.5" cy="5.5" r="1.4"/>',
  mirror: '<path class="r" d="M12 3v18" stroke-dasharray="2 2"/><path class="t" d="M9 9L3 19h6z"/><path d="M15 9l6 10h-6z"/><path class="a" d="M7 6.5C9 3 15 3 17 6.5M17.3 3.8L17 6.5l-2.6-.6"/>',
  coincident: '<path d="M4 20L20 4M4 4l16 16"/><circle class="af" cx="12" cy="12" r="2.6"/>',
  parallel: '<path d="M6 20L12 4M13 20l6-16"/><path class="a" d="M9.5 7l2.5-3 1 3.7M16.5 7l2.5-3 1 3.7"/>',
  perpendicular: '<path d="M4 20h16M12 20V5"/><path class="a" d="M12 15.5h4.5V20"/>',
  tangent: '<circle cx="10" cy="13" r="6"/><path d="M3 7h18"/><circle class="af" cx="10" cy="7" r="2"/>',
  equal: '<path d="M5 9h14M5 15h14"/><path class="a" d="M11 6.5l2 5M11 12.5l2 5"/>',
  paint: '<circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 0 0 16z" fill="currentColor"/>',
  bmirror: '<path class="r" d="M12 3v18" stroke-dasharray="2 2"/><path class="t" d="M9 8L3.5 10.5v7L9 20z"/><path d="M15 8l5.5 2.5v7L15 20z"/><path class="a" d="M7 5.5C9 2.5 15 2.5 17 5.5M17.4 2.9L17 5.5l-2.6-.5"/>',
  lpattern: '<rect class="t" x="2.5" y="7" width="5" height="6" rx="1"/><rect x="9.5" y="7" width="5" height="6" rx="1"/><rect x="16.5" y="7" width="5" height="6" rx="1" stroke-dasharray="2 1.6"/><path class="a" d="M4 18h15M17 16l2 2-2 2"/>',
  cpattern: '<circle class="a" cx="12" cy="12" r="7.5" stroke-dasharray="2 2.2"/><rect class="t" x="10" y="2.5" width="4" height="4" rx="1"/><rect x="17.5" y="10" width="4" height="4" rx="1"/><rect x="10" y="17.5" width="4" height="4" rx="1"/><rect x="2.5" y="10" width="4" height="4" rx="1" stroke-dasharray="1.6 1.4"/>',
};
// Icon colour by category: sketch drawing, modify, constraints, dimensions,
// solid/body, construction, inspect, the design's file.
const CAT = {};
for (const [cat, names] of Object.entries({
  sketch: 'sketch line rect circle arc polygon spline cat-draw',
  mod: 'trim extend mirror skoffset sfillet schamfer pushpull fillet chamfer shell combine offset cat-smodify cat-modify',
  cstr: 'constrain coincident parallel perpendicular tangent equal cat-constrain',
  dim: 'dim cat-dim',
  solid: 'extrude revolve hole body paint sweep loft box move bmirror lpattern cpattern cat-create cat-transform',
  cons: 'plane origin axis point cat-construct',
  insp: 'measure section cat-inspect',
  file: 'new folder save export',
})) for (const n of names.split(' ')) CAT[n] = cat;
const icon = (n, cls = 'i') => `<svg class="${cls}${CAT[n] ? ' c-' + CAT[n] : ''}" viewBox="0 0 24 24">${I[n] || ''}</svg>`;
// A folded "V": two slabs meeting at a crease, with a sketch point above.
const LOGO = `<svg class="logo" viewBox="0 0 24 24">
  <path class="la" d="M2.6 6.2h5.2l4.2 8.6v6z"/>
  <path class="lb" d="M21.4 6.2h-5.2L12 14.8v6z"/>
  <circle class="ld" cx="12" cy="4.6" r="2"/>
</svg>`;

function mouse(btn) {
  const wheel = '<rect class="b" x="4.1" y="2.2" width="1.8" height="3" rx=".9"/>';
  const b = {
    l: '<path class="b" d="M5 .5A4.5 4.5 0 0 0 .5 5v1H5z"/>',
    r: '<path class="b" d="M5 .5A4.5 4.5 0 0 1 9.5 5v1H5z"/>',
    m: wheel, w: wheel,
  }[btn];
  return `<svg class="mouse" viewBox="0 0 10 14"><rect x=".5" y=".5" width="9" height="13" rx="4.5"/>${b}</svg>`;
}

// ---------------------------------------------------------------- vectors and projection
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a, k) => [a[0] * k, a[1] * k, a[2] * k];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const norm = a => { const l = Math.hypot(...a); return a.map(x => x / l); };
const rad = d => d * Math.PI / 180;
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

// Camera orbiting target T. yaw around +Z, pitch above the XY plane. Orthographic,
// or in perspective with the eye D from T: the target's depth keeps its scale,
// nearer is larger.
function makeProj(yaw, pitch, T, D = Infinity) {
  const cy = Math.cos(rad(yaw)), sy = Math.sin(rad(yaw)), cp = Math.cos(rad(pitch)), sp = Math.sin(rad(pitch));
  const d = [cp * cy, cp * sy, sp], r = [-sy, cy, 0], u = [-sp * cy, -sp * sy, cp];
  const light = norm([0, 1, 2].map(i => -0.45 * r[i] + 0.7 * u[i] + 0.6 * d[i]));
  const P = (x, y, z) => {
    const q = [x - T[0], y - T[1], z - T[2]], k = D === Infinity ? 1 : D / Math.max(D - dot(q, d), 1e-3);
    return [dot(q, r) * k, -dot(q, u) * k];
  };
  // Whether a face with normal n through point p is turned towards the eye.
  const facing = (n, p, eps = 1e-4) => D === Infinity ? dot(n, d) > eps : dot(n, sub(add(T, mul(d, D)), p)) > eps;
  return { P, d, r, u, T, D, light, facing, depth: p => dot(sub(p, T), d) };
}

let proj;
const pts = (a, pr = proj) => a.map(p => pr.P(...p).map(n => n.toFixed(2)).join(',')).join(' ');
// Mean depth of a set of points, for painter's-order sorting.
const depthOf = (ps, pr = proj) => ps.reduce((s, p) => s + pr.depth(p), 0) / ps.length;
// Per-axis [min, max] of a list of points.
const bounds = ps => ps[0].map((_, i) => [Math.min(...ps.map(p => p[i])), Math.max(...ps.map(p => p[i]))]);
const lineP = (a, b, cls, pr = proj) => {
  const [x1, y1] = pr.P(...a), [x2, y2] = pr.P(...b);
  return `<line class="${cls}" x1="${x1.toFixed(2)}" y1="${y1.toFixed(2)}" x2="${x2.toFixed(2)}" y2="${y2.toFixed(2)}"/>`;
};
const circ = (f, cu, cv, r, n = 48, a0 = 0, a1 = Math.PI * 2) => {
  const o = [];
  for (let i = 0; i < n; i++) { const t = a0 + (a1 - a0) * i / n; o.push(f(cu + r * Math.cos(t), cv + r * Math.sin(t))); }
  return o;
};
const zc = z => (u, v) => [u, v, z];
const xc = x => (u, v) => [x, u, v];

// ---------------------------------------------------------------- solids
// Lit face colour, plus the selected and hovered variants, which keep the
// shading and only swap the hue so a selected face still reads as 3D.
function shade(n, pr, dark) {
  const i = Math.max(0, dot(n, pr.light));
  const L = dark ? 30 + 32 * i : 54 + 33 * i;
  const l = (h, s, dl = 0) => `hsl(${h} ${s}% ${(L + dl).toFixed(1)}%)`;
  // x and a tint what a cut or an intersect being set up takes away or keeps.
  return dark
    ? { f: l(258, 9), s: l(188, 42, 2), h: l(110, 26, 2), x: l(6, 46, 2), a: l(36, 52, 2) }
    : { f: l(258, 11), s: l(188, 52, -2), h: l(110, 34, 0), x: l(6, 62, -4), a: l(36, 72, -4) };
}

let uid = 0;
// Faces are planar polygons with outward normals; back faces are culled and
// the rest painted far-to-near. Holes are drawn on their face: the near rim is
// the wall, the far rim clipped to it is where you see through. A face can
// carry its body (`body`), a colour variant (`tint`), `draft` (a preview,
// never picked), `fade` and a shell's `opening`. With `marks`, the faces can be
// picked and show the selection, and the faces ('base-top') and bodies
// ('body:body') in `marks` as if selected; without, they're a picture.
function solidSvg(faces, pr, marks = null) {
  const interactive = !!marks;
  const vis = faces.filter(f => pr.facing(f.n, f.p[0]))
    .map(f => ({ f, z: depthOf(f.p, pr) }))
    .sort((a, b) => a.z - b.z);
  const bodySel = interactive && st.sel?.kind === 'body';
  const dark = isDark();
  let out = '';
  for (const { f } of vis) {
    const c = shade(f.n, pr, dark), p = pts(f.p, pr), fill = c[f.tint || 'f'];
    const live = interactive && !f.draft, body = f.body || 'body';
    const inBody = live && ((bodySel && st.sel.id === body) || marks.has('body:' + body));
    if (live) {
      const sel = inBody || (f.id && (st.sel?.id === f.id || marks.has(f.id)));
      const cls = 'face' + (sel ? ' sel' : '') + (f.fade ? ' fade' : '');
      out += `<polygon class="${cls}"${f.id ? ` data-face="${f.id}"` : ''} data-body="${body}" style="--f:${fill};--fs:${c.s};--fh:${c.h}" points="${p}"/>`;
    } else {
      out += `<polygon class="tf${f.draft ? ' draft' : ''}" style="fill:${fill}${f.smooth ? ';stroke:' + fill : ''}" points="${p}"/>`;
    }
    if (f.opening) out += `<polygon class="wall" points="${pts(f.opening, pr)}"/>`;
    for (const [a, b] of f.tangents || []) out += lineP(a, b, 'tangent', pr);
    for (const h of f.holes || []) {
      const id = 'c' + (uid++);
      const sel = inBody || st.sel?.id === h.id;
      const attrs = live && h.id ? ` class="hole${sel ? ' sel' : ''}" data-face="${h.id}" data-body="${body}"` : '';
      const near = pts(h.near, pr);
      out += `<g${attrs}><clipPath id="${id}"><polygon points="${near}"/></clipPath>
        <polygon class="wall" points="${near}"/>
        <polygon class="thru" clip-path="url(#${id})" points="${pts(h.far, pr)}"/></g>`;
    }
  }
  return out;
}

// ---------------------------------------------------------------- camera
// The camera is fixed: each page is a screenshot, not a model to turn.
const VIEWS = {
  home: { yaw: -58, pitch: 24 },
  top: { yaw: -90, pitch: 90 }, bottom: { yaw: -90, pitch: -90 },
  front: { yaw: -90, pitch: 0 }, back: { yaw: 90, pitch: 0 },
  right: { yaw: 0, pitch: 0 }, left: { yaw: 180, pitch: 0 },
};
const HOME = { ...VIEWS.home, zoom: 0.8, px: 0, py: 0 };
const cam = { ...HOME };
// The app's 45° vertical field of view: the eye's distance for the view's height.
const eyeDistance = () => 250 * cam.zoom / (2 * Math.tan(rad(22.5)));
const viewBox = () => { const w = 380 * cam.zoom, h = 250 * cam.zoom; return `${cam.px - w / 2} ${cam.py - h / 2} ${w} ${h}`; };

// Scene units per screen pixel, for things sized in pixels.
function unitsPerPixel() {
  const s = $('#scene'), w = 380 * cam.zoom, h = 250 * cam.zoom;
  const r = s?.getBoundingClientRect();
  return r?.width && r.height ? Math.max(w / r.width, h / r.height) : h / 600;
}

// ---------------------------------------------------------------- the bracket
// The design the mock shows: its faces, the planes and sketches on it, its
// timeline.
const HA0 = circ(zc(0), 50, 25, 8), HA1 = circ(zc(10), 50, 25, 8);
const HB0 = circ(xc(0), 25, 45, 7), HB1 = circ(xc(10), 25, 45, 7);
const Lprof = y => [[0, y, 0], [80, y, 0], [80, y, 10], [10, y, 10], [10, y, 70], [0, y, 70]];
// `feat` ties holes and fillet edges to the timeline feature that makes them.
const BRACKET = [
  { id: 'side', n: [0, -1, 0], p: Lprof(0) },
  { id: 'side-b', n: [0, 1, 0], p: Lprof(50).reverse() },
  { id: 'bottom', n: [0, 0, -1], p: [[0, 0, 0], [80, 0, 0], [80, 50, 0], [0, 50, 0]], holes: [{ id: 'hole-a', feat: 'h1', near: HA0, far: HA1 }] },
  { id: 'back', n: [-1, 0, 0], p: [[0, 0, 0], [0, 50, 0], [0, 50, 70], [0, 0, 70]], holes: [{ id: 'hole-b', feat: 'h2', near: HB0, far: HB1 }] },
  { id: 'base-x', n: [1, 0, 0], p: [[80, 0, 0], [80, 50, 0], [80, 50, 10], [80, 0, 10]] },
  { id: 'base-top', n: [0, 0, 1], p: [[10, 0, 10], [80, 0, 10], [80, 50, 10], [10, 50, 10]], holes: [{ id: 'hole-a', feat: 'h1', near: HA1, far: HA0 }], tangents: [[[12, 0, 10], [12, 50, 10]]] },
  { id: 'up-x', n: [1, 0, 0], p: [[10, 0, 10], [10, 50, 10], [10, 50, 70], [10, 0, 70]], holes: [{ id: 'hole-b', feat: 'h2', near: HB1, far: HB0 }], tangents: [[[10, 0, 12], [10, 50, 12]]] },
  { id: 'up-top', n: [0, 0, 1], p: [[0, 0, 70], [10, 0, 70], [10, 50, 70], [0, 50, 70]] },
];
// The bracket as the features in `active` leave it: its holes, and Fillet 1's
// tangent lines.
const bracketAt = active => BRACKET.map(f => ({
  ...f,
  holes: (f.holes || []).filter(h => active.has(h.feat)),
  tangents: active.has('f1') ? f.tangents || [] : [],
}));

// Planes that can hold a sketch: map (u, v) in the plane to model space.
const PLANES = {
  'side':     { f: (u, v) => [u, 0, v], b: [0, 80, 0, 70], view: VIEWS.front },
  'side-b':   { f: (u, v) => [u, 50, v], b: [0, 80, 0, 70], view: VIEWS.back },
  'bottom':   { f: (u, v) => [u, v, 0], b: [0, 80, 0, 50], view: VIEWS.bottom },
  'back':     { f: (u, v) => [0, u, v], b: [0, 50, 0, 70], view: VIEWS.left },
  'base-top': { f: (u, v) => [u, v, 10], b: [0, 80, 0, 50], view: VIEWS.top },
  'base-x':   { f: (u, v) => [80, u, v], b: [0, 50, 0, 10], view: VIEWS.right },
  'up-x':     { f: (u, v) => [10, u, v], b: [0, 50, 0, 70], view: VIEWS.right },
  'up-top':   { f: (u, v) => [u, v, 70], b: [0, 10, 0, 50], view: VIEWS.top },
  'plane-xy': { f: (u, v) => [u, v, 0], b: [0, 40, 0, 40], view: VIEWS.top },
  'plane-xz': { f: (u, v) => [u, 0, v], b: [0, 40, 0, 40], view: VIEWS.front },
  'plane-yz': { f: (u, v) => [0, u, v], b: [0, 40, 0, 40], view: VIEWS.right },
};

const SKETCHES = {
  s1: {
    plane: 'side', closed: [[0, 0], [80, 0], [80, 10], [10, 10], [10, 70], [0, 70]], circles: [],
    dims: [{ a: [0, -9], b: [80, -9], t: '80' }, { a: [-9, 0], b: [-9, 70], t: '70' }, { a: [89, 0], b: [89, 10], t: '10' }, { a: [0, 79], b: [10, 79], t: '10' }],
    summary: '6 lines · 4 dimensions',
  },
  s2: {
    plane: 'base-top', circles: [{ c: [50, 25], r: 8 }],
    dims: [{ a: [50, -8], b: [80, -8], t: '30' }, { a: [88, 0], b: [88, 25], t: '25' }], labels: [{ p: [64, 37], t: 'Ø16' }],
    summary: '1 circle · 3 dimensions',
  },
  s3: {
    plane: 'up-x', circles: [{ c: [25, 45], r: 7 }],
    dims: [{ a: [25, 79], b: [50, 79], t: '25' }, { a: [58, 10], b: [58, 45], t: '35' }], labels: [{ p: [37, 57], t: 'Ø14' }],
    summary: '1 circle · 3 dimensions',
  },
};

const FACE_INFO = {
  'side': ['Planar face', '1 400 mm²', 'Extrude 1'],
  'side-b': ['Planar face', '1 400 mm²', 'Extrude 1'],
  'bottom': ['Planar face', '3 799 mm²', 'Extrude 1'],
  'back': ['Planar face', '3 346 mm²', 'Extrude 1'],
  'base-top': ['Planar face', '3 299 mm²', 'Extrude 1'],
  'base-x': ['Planar face', '500 mm²', 'Extrude 1'],
  'up-x': ['Planar face', '2 846 mm²', 'Extrude 1'],
  'up-top': ['Planar face', '500 mm²', 'Extrude 1'],
  'hole-a': ['Cylindrical face', 'Ø16 × 10 mm', 'Hole 1'],
  'hole-b': ['Cylindrical face', 'Ø14 × 10 mm', 'Hole 2'],
  'plane-xy': ['Origin plane', 'XY', 'Origin'],
  'plane-xz': ['Origin plane', 'XZ', 'Origin'],
  'plane-yz': ['Origin plane', 'YZ', 'Origin'],
};

const BRACKET_TIMELINE = [
  { id: 's1', type: 'sketch', name: 'Sketch 1', meta: 'Front' },
  { id: 'e1', type: 'extrude', name: 'Extrude 1', meta: '50 mm', info: 'Distance 50 mm · New body' },
  { id: 's2', type: 'sketch', name: 'Sketch 2', meta: 'Face' },
  { id: 'h1', type: 'hole', name: 'Hole 1', meta: 'Ø16 thru', info: 'Simple · Ø16 mm · Through all' },
  { id: 's3', type: 'sketch', name: 'Sketch 3', meta: 'Face' },
  { id: 'h2', type: 'hole', name: 'Hole 2', meta: 'Ø14 thru', info: 'Simple · Ø14 mm · Through all' },
  { id: 'f1', type: 'fillet', name: 'Fillet 1', meta: 'R2', info: '1 edge · R2 mm · Tangent chain', params: { edges: ['base-top|up-x'], radius: '2 mm', chain: true } },
];

// `extra` holds bodies beyond the bracket's.
function makeDoc(kind, name) {
  return kind === 'bracket'
    ? { kind, name, dirty: true, T: [40, 25, 32], timeline: BRACKET_TIMELINE.map(t => ({ ...t })), extra: [], hidden: new Set(['origin', 's1', 's2', 's3']) }
    : { kind, name, dirty: false, T: [0, 0, 0], timeline: [], extra: [], hidden: new Set() };
}

// ---------------------------------------------------------------- state
const st = {
  doc: null,       // the design open, none on the welcome screen
  mode: 'model',   // 'model' | 'sketch'
  sketch: null,    // { id, plane }
  tool: null,      // active sketch tool
  pick: false,     // waiting for a plane to sketch on
  sel: null,       // { kind: 'face'|'hole'|'plane'|'feature'|'body', id }
  panel: 'timeline', alt: false, menu: false, help: false,
  gbarPinned: false,
  railOpen: null,  // index of the tool set whose list is open from the rail
  persp: false,    // perspective, else orthographic
  projMenu: false, // the status bar's menu open: the projection, mouse hints
  mouseHints: true, // the status bar shows the mouse's actions
  op: null,        // solid operation being set up: { kind, pick, editing?, ...its parameters }
};
const root = document.documentElement;
const $ = s => document.querySelector(s);

// Features applied to the model: all of them, or those before the sketch
// being edited, since editing rolls the model back to it.
function activeFeatures() {
  const tl = st.doc.timeline;
  const n = st.mode === 'sketch' ? tl.findIndex(t => t.id === st.sketch.id) : tl.length;
  return new Set(tl.slice(0, n).map(t => t.id));
}

// The bodies shown: the bracket as the applied features make it, and the
// document's extra bodies. Operations added in the mock don't change them.
function bodiesAt(active) {
  const d = st.doc, bodies = [];
  if (d.kind === 'bracket' && active.has('e1')) bodies.push({ id: 'body', name: 'Bracket', faces: bracketAt(active) });
  return [...bodies, ...d.extra];
}

// The page's hooks (see the top of this file); each page fills them in.
const page = {};

// ---------------------------------------------------------------- operations per context
function context() {
  if (st.mode === 'sketch') return st.tool ? 'tool' : 'sketch';
  if (st.op) return 'op';
  if (st.pick) return 'pick';
  return st.sel ? st.sel.kind : 'none';
}

function ops() {
  const c = context();
  const measure = [{ sep: 1 }, { id: 'measure', label: 'Measure', icon: 'measure', key: 'I' }];
  switch (c) {
    case 'none':
    case 'pick':
      return [
        { id: 'sketch', label: 'Sketch', icon: 'sketch', key: 'S', on: c === 'pick' },
        { id: 'extrude', label: 'Extrude', icon: 'extrude', key: 'X' },
        { id: 'revolve', label: 'Revolve', icon: 'revolve', key: 'O' },
        { id: 'hole', label: 'Hole', icon: 'hole', key: 'H' },
        { id: 'fillet', label: 'Fillet', icon: 'fillet', key: 'F' },
        { id: 'chamfer', label: 'Chamfer', icon: 'chamfer', key: 'C' },
        { id: 'shell', label: 'Shell', icon: 'shell' },
        { id: 'combine', label: 'Combine', icon: 'combine', key: 'B' },
        { id: 'move', label: 'Move', icon: 'move', key: 'M' },
        { id: 'lpattern', label: 'Pattern', icon: 'lpattern', key: 'P' },
        ...measure,
      ];
    case 'face':
      return [
        { id: 'sketch', label: 'Sketch on face', icon: 'sketch', key: 'S' },
        { id: 'pushpull', label: 'Press pull', icon: 'pushpull', key: 'U' },
        { id: 'extrude', label: 'Extrude', icon: 'extrude', key: 'X' },
        { id: 'hole', label: 'Hole', icon: 'hole', key: 'H' },
        { id: 'offset', label: 'Offset face', icon: 'offset' },
        { id: 'shell', label: 'Shell from here', icon: 'shell' },
        ...measure,
      ];
    case 'hole':
      return [
        { id: 'pushpull', label: 'Press pull', icon: 'pushpull', key: 'U' },
        { id: 'fillet', label: 'Fillet edges', icon: 'fillet', key: 'F' },
        { id: 'chamfer', label: 'Chamfer edges', icon: 'chamfer', key: 'C' },
        { id: 'edit-feature', label: 'Edit ' + FACE_INFO[st.sel.id][2], icon: 'edit', key: 'Enter' },
        ...measure,
      ];
    case 'plane':
      return [
        { id: 'sketch', label: 'Sketch on plane', icon: 'sketch', key: 'S' },
        { id: 'offset-plane', label: 'Offset plane', icon: 'plane' },
        ...measure,
      ];
    case 'body':
      return [
        { id: 'move', label: 'Move', icon: 'move', key: 'M' },
        { id: 'bmirror', label: 'Mirror', icon: 'bmirror' },
        { id: 'lpattern', label: 'Pattern', icon: 'lpattern', key: 'P' },
        { id: 'cpattern', label: 'Circular pattern', icon: 'cpattern' },
        { id: 'combine', label: 'Combine', icon: 'combine', key: 'B' },
        { sep: 1 },
        { id: 'appearance', label: 'Appearance', icon: 'paint', key: 'A' },
        { id: 'material', label: 'Material', icon: 'body' },
        { id: 'hide', label: 'Hide', icon: 'eyeoff', key: 'V' },
        { id: 'export', label: 'Export…', icon: 'export' },
        ...measure,
      ];
    case 'feature': {
      const f = st.doc.timeline.find(t => t.id === st.sel.id);
      return [
        f.type === 'sketch'
          ? { id: 'edit-sketch', label: 'Edit sketch', icon: 'sketch', key: 'Enter' }
          : { id: 'edit-feature', label: 'Edit ' + f.name, icon: 'edit', key: 'Enter' },
        { id: 'rollback', label: 'Roll back here', icon: 'rollback' },
        { id: 'suppress', label: 'Suppress', icon: 'eyeoff' },
        { id: 'rename', label: 'Rename', icon: 'edit', key: 'F2' },
        { sep: 1 },
        { id: 'delete', label: 'Delete', icon: 'trash', key: 'Del' },
      ];
    }
    case 'op':
      return [
        { id: 'op-cancel', label: 'Cancel', icon: 'close', key: 'Esc' },
        { id: 'op-ok', label: 'OK', icon: 'check', key: 'Enter', primary: true },
      ];
    case 'sketch':
    case 'tool':
      return [
        ...Object.entries(TOOLS).filter(([, t]) => t.key)
          .map(([id, { label, key }]) => ({ id: 'tool:' + id, label, icon: id === 'offset' ? 'skoffset' : id, key, on: st.tool === id })),
      ];
  }
  return [];
}

// What the mock leaves out, said for an operation that does nothing here.
function notWired(id) {
  const o = ops().find(o => o.id === id) || GLOBAL_FLAT.find(o => o.op === id);
  toast((o ? o.label : id) + ' — not wired up in the mock');
}

// Run an operation from the Alt bar. Sketch tools need a sketch first.
function globalOp(op) {
  if (op.startsWith('tool:') && st.mode !== 'sketch') { st.pick = true; st.sel = null; toast('Pick a plane to sketch on first'); return; }
  if (op === 'sketch' && st.mode === 'sketch') { toast('Already in a sketch'); return; }
  page.doOp(op);
}

// Sketch tools. The ones with a key are listed in the sketch toolbar, in this order.
const Tool = (label, key, hint) => ({ label, key, hint });
const TOOLS = {
  line: Tool('Line', 'L', 'Click start point'), rect: Tool('Rectangle', 'B', 'Click first corner'),
  circle: Tool('Circle', 'C', 'Click center point'), arc: Tool('Arc', 'A', 'Click start point'),
  trim: Tool('Trim', 'T', 'Click a segment to trim'), offset: Tool('Offset', 'O', 'Select a loop to offset'),
  dim: Tool('Dimension', 'D', 'Select geometry to dimension'), constrain: Tool('Constrain', 'K', 'Select entities to constrain'),
  polygon: Tool('Polygon', '', 'Click center point'), spline: Tool('Spline', '', 'Click first fit point'),
  extend: Tool('Extend', '', 'Click a segment to extend'), mirror: Tool('Mirror', '', 'Select entities to mirror'),
  fillet: Tool('Fillet', '', 'Click a corner to fillet'), chamfer: Tool('Chamfer', '', 'Click a corner to chamfer'),
};

// ---------------------------------------------------------------- Alt bar and tool rail
// The Alt bar: the full set of operations, grouped by kind.
// It follows the mode (model or sketch) but not the selection.
// Items are [icon, label, op]; op defaults to the icon name.
const G = (name, items) => [name, items.map(([icon, label, op = icon]) => ({ icon, label, op }))];
const MODEL_GROUPS = [
  G('Create', [['sketch', 'Sketch'], ['extrude', 'Extrude'], ['revolve', 'Revolve'], ['sweep', 'Sweep'], ['loft', 'Loft'], ['hole', 'Hole'], ['box', 'Box']]),
  G('Modify', [['pushpull', 'Press pull'], ['fillet', 'Fillet'], ['chamfer', 'Chamfer'], ['shell', 'Shell'], ['combine', 'Combine']]),
  G('Transform', [['move', 'Move'], ['bmirror', 'Mirror'], ['lpattern', 'Linear pattern'], ['cpattern', 'Circular pattern']]),
  G('Construct', [['plane', 'Plane', 'offset-plane'], ['axis', 'Axis'], ['point', 'Point']]),
  G('Inspect', [['measure', 'Measure'], ['section', 'Section']]),
];
const SKETCH_GROUPS = [
  G('Create', [['line', 'Line', 'tool:line'], ['rect', 'Rectangle', 'tool:rect'], ['circle', 'Circle', 'tool:circle'], ['arc', 'Arc', 'tool:arc'],
               ['polygon', 'Polygon', 'tool:polygon'], ['spline', 'Spline', 'tool:spline']]),
  G('Modify', [['trim', 'Trim', 'tool:trim'], ['extend', 'Extend', 'tool:extend'], ['skoffset', 'Offset', 'tool:offset'], ['mirror', 'Mirror', 'tool:mirror'],
               ['sfillet', 'Fillet', 'tool:fillet'], ['schamfer', 'Chamfer', 'tool:chamfer']]),
  G('Constraints', [['coincident', 'Coincident', 'tool:constrain'], ['parallel', 'Parallel', 'tool:constrain'], ['perpendicular', 'Perpendicular', 'tool:constrain'],
                    ['tangent', 'Tangent', 'tool:constrain'], ['equal', 'Equal', 'tool:constrain']]),
  G('Dimension', [['dim', 'Dimension', 'tool:dim']]),
];
const GLOBAL_FLAT = [...MODEL_GROUPS, ...SKETCH_GROUPS].flatMap(([, items]) => items);

function gbar() {
  return `<div class="gbar">
    ${(st.mode === 'sketch' ? SKETCH_GROUPS : MODEL_GROUPS).map(([name, items]) => `<div class="grp">
      <div class="gl">${name}</div>
      <div class="btns">${items.map(o => `<button class="gbtn" data-act="gop:${o.op}" title="${o.label}">${icon(o.icon)}${o.label}</button>`).join('')}</div>
    </div>`).join('')}
  </div>`;
}

// The tool rail: the same sets as the Alt bar, one card each with the set's
// icon, its number key and its first tools on a recessed strip. Pointing at a
// set (or clicking it, or its number key) opens its full list beside the card;
// letters then pick from it, Esc closes it.
const SET_STYLE = {
  model: { Create: ['cat-create', 'solid'], Modify: ['cat-modify', 'mod'], Transform: ['cat-transform', 'solid'], Construct: ['cat-construct', 'cons'], Inspect: ['cat-inspect', 'insp'] },
  sketch: { Create: ['cat-draw', 'sketch'], Modify: ['cat-smodify', 'mod'], Constraints: ['cat-constrain', 'cstr'], Dimension: ['cat-dim', 'dim'] },
};
const RAIL_TOOLS = 3;
const railSets = () => st.mode === 'sketch' ? SKETCH_GROUPS : MODEL_GROUPS;
const railStyle = name => SET_STYLE[st.mode === 'sketch' ? 'sketch' : 'model'][name];
// The keys opening the rail's sets, in order: the top row from the left.
// No tool's key is one of them.
const RAIL_SET_KEYS = 'QWERTYUIO';
// Shortcut letters inside an open set: the first free letter of each label,
// never a set's key, so another set opens from an open list.
function railLetters(items) {
  const used = [...RAIL_SET_KEYS.slice(0, railSets().length)];
  return items.map(o => { const k = [...o.label.toUpperCase()].find(c => /[A-Z]/.test(c) && !used.includes(c)) || ''; used.push(k); return k; });
}
// A rail tool's shortcut: the toolbar's key for it in model mode, the sketch tool's key in a sketch.
const toolKey = o => o.op.startsWith('tool:') ? TOOLS[o.op.slice(5)]?.key || '' : ops().find(x => x.id === o.op)?.key || '';
function rail() {
  return `<div class="rail">${railSets().map(([name, items], i) => {
    const [ic, cat] = railStyle(name);
    const tools = items.slice(0, RAIL_TOOLS).map(o => `<button class="rtool" data-act="rgop:${o.op}" data-tip="${o.label}" data-key="${toolKey(o)}">${icon(o.icon)}</button>`).join('');
    return `<div class="rcard"><button class="rset${st.railOpen === i ? ' on' : ''}" data-act="rail:${i}" data-rail="${i}" title="${name} (${RAIL_SET_KEYS[i]})">${icon(ic, 'i c-' + cat)}</button>${tools ? `<div class="rtools">${tools}</div>` : ''}</div>`;
  }).join('')}</div>`;
}
function railPop() {
  const set = railSets()[st.railOpen];
  if (!set) return '';
  const [name, items] = set, [ic, cat] = railStyle(name), keys = railLetters(items);
  return `<div class="rpop" id="rpop" style="--rc:var(--i-${cat})">
    <div class="hd">${icon(ic, 'i c-' + cat)}${name}<kbd class="own">${RAIL_SET_KEYS[st.railOpen]}</kbd><kbd>Esc</kbd></div>
    ${items.map((o, i) => `<button data-act="rgop:${o.op}">${icon(o.icon)}${o.label}<span class="k">${keys[i]}</span></button>`).join('')}
  </div>`;
}
// After a render: line the open list up with its card, moved up as far as it
// must to stay in the window, and drop rail tools evenly if the rail is short
// (the set showing the most gives up its last one until everything fits).
function placeRail() {
  const r = $('.rail');
  if (!r) return;
  const strips = [...r.querySelectorAll('.rtools')].map(t => [...t.children]);
  strips.flat().forEach(b => b.hidden = false);
  r.querySelectorAll('.rtools').forEach(t => t.hidden = false);
  for (let guard = 100; r.scrollHeight > r.clientHeight + 1 && guard--;) {
    let most = null;
    for (const s of strips) { const v = s.filter(b => !b.hidden); if (v.length && (!most || v.length >= most.length)) most = v; }
    if (!most) break;
    most.at(-1).hidden = true;
    const t = most.at(-1).parentElement;
    if (![...t.children].some(b => !b.hidden)) t.hidden = true;
  }
  const pop = $('#rpop'), card = st.railOpen != null && $(`[data-rail="${st.railOpen}"]`);
  if (!pop || !card) return;
  const win = $('#win').getBoundingClientRect(), top = card.getBoundingClientRect().top - win.top - 4;
  const bottom = win.height - parseFloat(getComputedStyle(root).getPropertyValue('--status-h')) - 8;
  pop.style.top = Math.max(0, Math.min(top, bottom - pop.offsetHeight)) + 'px';
}

// ---------------------------------------------------------------- chrome
function titlebar() {
  const title = st.doc
    ? `<b>${st.doc.name}.vrdp</b>${st.doc.dirty ? ' — Edited' : ''}`
    : 'Welcome';
  return `<div class="titlebar">
    <span class="brand">${LOGO}varde</span>
    <span class="title">${title}</span>
    <div class="spacer"></div>
    <button class="op icon" data-act="theme" title="Toggle theme">${icon(isDark() ? 'sun' : 'moon')}</button>
    <button class="op icon" data-act="help" title="Shortcuts (?)">${icon('help')}</button>
    <div class="winctl">
      <button data-act="toast:Minimize" title="Minimize">${icon('min')}</button>
      <button data-act="toast:Maximize" title="Maximize">${icon('max')}</button>
      <button class="close" data-act="toast:Close" title="Close">${icon('close')}</button>
    </div>
  </div>`;
}

let lastOpsKey = '';
function toolbar() {
  const d = st.doc;
  let ctx;
  if (st.mode === 'sketch') {
    const name = d.timeline.find(t => t.id === st.sketch.id).name;
    ctx = `<span class="wash"><span>${icon('sketch')}${name}</span><button class="op primary finish" data-act="op:finish" title="Finish sketch (Esc)">${icon('check')}</button></span>${st.tool ? `<span class="tag">${TOOLS[st.tool].label}</span>` : ''}`;
  } else {
    const tag = st.op ? (st.op.editing ? 'Editing ' + st.op.name : page.op.label()) : st.pick ? 'New sketch' : '';
    ctx = `Model${tag ? `<span class="tag">${tag}</span>` : ''}`;
  }

  const list = ops();
  const key = list.map(o => o.id).join();
  const enter = key !== lastOpsKey;
  lastOpsKey = key;
  let i = 0;
  const opsHtml = list.map(o => o.sep ? '<span class="sep"></span>' : `
    <button class="op${o.on ? ' on' : ''}${o.primary ? ' primary' : ''}" style="--i:${i++}" data-act="op:${o.id}" title="${o.label}${o.key ? ` (${o.key})` : ''}">
      ${icon(o.icon)}<span class="lbl">${o.label}</span>${o.key && !o.primary ? `<span class="k">${o.key}</span>` : ''}
    </button>`).join('');

  return `<div class="toolbar">
    <div class="file" data-act="menu">
      <span class="name">${d.name}<span class="ext">.vrdp</span></span>
      ${d.dirty ? '<span class="dirty" title="Unsaved changes"></span>' : ''}
      ${icon('chev', 'i chev')}
      ${st.menu ? `
        <div class="menu">
          <button data-act="new">${icon('new')}New design<span class="k">Ctrl N</span></button>
          <button data-act="open:bracket">${icon('folder')}Open…<span class="k">Ctrl O</span></button>
          <button data-act="toast:Saved">${icon('save')}Save<span class="k">Ctrl S</span></button>
          <button data-act="toast:Export">${icon('export')}Export…<span class="k">Ctrl E</span></button>
          <hr>
          <button data-act="home">${icon('close')}Close document</button>
        </div>` : ''}
    </div>
    <div class="save-cell"><button class="op icon" data-act="toast:Saved" title="Save (Ctrl S)">${icon('save')}</button></div>
    <div class="ctx">${ctx}</div>
    <div class="ops${list.length > 8 ? ' many' : ''}${enter ? ' enter' : ''}">${opsHtml}</div>
    <div class="spacer"></div>
    <button class="op icon" data-act="toast:Undo" title="Undo (Ctrl Z)">${icon('undo')}</button>
    <button class="op icon" data-act="toast:Redo" title="Redo (Ctrl Shift Z)">${icon('redo')}</button>
    <span class="sep"></span>
    <button class="op icon" data-act="toast:Command palette" title="Commands (Ctrl K)">${icon('search')}</button>
  </div>`;
}

// ---------------------------------------------------------------- status bar
// Floating at the view's bottom right: the selection in a box of its own,
// with the key clearing it, then the status bar: what's going on, the mouse
// and key hints, and with a design open a menu of view options (the
// projection, whether to show the mouse hints).
const SELECTION = new Set(['face', 'hole', 'plane', 'body', 'feature']);
const statusbar = () => {
  const i = info(), h = hints(), sel = st.doc && SELECTION.has(context());
  const parts = [!sel && i && `<div class="info">${i}</div>`, h && `<div class="hints">${h}</div>`, st.doc && projDropdown()];
  const box = sel ? `<div class="selbox"><div class="info">${i}</div><span class="sep"></span><span class="h"><kbd>Space</kbd><span>Clear</span></span></div>` : '';
  return `<div class="sbar">${box}<div class="statusbar">${parts.filter(Boolean).join('<span class="sep"></span>')}</div></div>`;
};
const PROJECTIONS = [[false, 'ortho', 'Orthographic'], [true, 'persp', 'Perspective']];
const projDropdown = () => {
  return `<div class="projdd${st.projMenu ? ' open' : ''}">
    <button class="sb-btn" data-act="projmenu" title="View options">${icon('more')}</button>
    ${st.projMenu ? `<div class="menu">${PROJECTIONS.map(([p, ic, label]) =>
      `<button class="${st.persp === p ? 'on' : ''}" data-act="proj:${ic}">${icon(ic)}${label}${icon('check', 'i tick')}</button>`).join('')}
      <hr><button class="${st.mouseHints ? 'on' : ''}" data-act="mousehints"><span class="mi">${mouse('l')}</span>Mouse hints${icon('check', 'i tick')}</button></div>` : ''}
  </div>`;
};

function hints() {
  const k = s => s.split(' ').map(x => x.startsWith('mouse:') ? mouse(x.slice(6)) : `<kbd>${x}</kbd>`).join('');
  // Without mouse hints, only those for keys.
  const h = list => list.filter(([keys]) => st.mouseHints || !keys.includes('mouse:'))
    .map(([keys, label]) => `<span class="h">${k(keys)}<span>${label}</span></span>`).join('');
  if (!st.doc) return h([['N', 'New design'], ['O', 'Open'], ['?', 'Shortcuts']]);
  const nav = [['mouse:l', 'Drag to orbit'], ['mouse:r', 'Pan'], ['mouse:w', 'Zoom'], ['mouse:m', 'Click to set pivot']];
  switch (context()) {
    case 'none': return h([['mouse:l', 'Select'], ['mouse:l mouse:l', 'Body'], ...nav]);
    case 'pick': return h([['mouse:l', 'Pick plane'], ...nav, ['Esc', 'Cancel']]);
    case 'face': case 'plane': return h([['S', 'Sketch'], ['U', 'Press pull'], ['mouse:l mouse:l', 'Body']]);
    case 'hole': return h([['F', 'Fillet'], ['Enter', 'Edit hole'], ['mouse:l mouse:l', 'Body']]);
    case 'feature': return h([['Enter', 'Edit'], ['Del', 'Delete'], ['mouse:l', 'Drag to reorder']]);
    case 'body': return h([['M', 'Move'], ['P', 'Pattern'], ['V', 'Hide']]);
    case 'op': return h([['mouse:l', page.op.pickHint()], ...nav, ['Enter', 'OK'], ['Esc', 'Cancel']]);
    case 'sketch': return h([['L', 'Line'], ['B', 'Rect'], ['C', 'Circle'], ['D', 'Dimension'], ['Esc', 'Finish']]);
    case 'tool': return h([['mouse:l', TOOLS[st.tool].hint], ['Shift', 'No snap'], ['Tab', 'Type value'], ['Esc', 'Stop tool']]);
  }
  return '';
}

function info() {
  if (!st.doc) return '';
  const c = context();
  if (c === 'pick') return `<span class="accent">Select a plane or planar face for the new sketch</span>`;
  if (c === 'op') return page.op.info();
  if (c === 'sketch' || c === 'tool') {
    const s = SKETCHES[st.sketch.id];
    const name = st.doc.timeline.find(t => t.id === st.sketch.id).name;
    return s
      ? `<span class="dotok"></span><b>${name}</b><span class="muted">Fully constrained · ${s.summary}</span>`
      : `<b>${name}</b><span class="muted">Empty · on ${FACE_INFO[st.sketch.plane]?.[1] || 'face'}</span>`;
  }
  if (c === 'none') return '';
  if (c === 'face' || c === 'hole' || c === 'plane') {
    const [t, d, f] = FACE_INFO[st.sel.id];
    return `${icon(c === 'plane' ? 'plane' : 'body')}<b>${t}</b><span>${d}</span><span class="muted">${f}</span>`;
  }
  if (c === 'body') {
    if (st.sel.id === 'body') return `${icon('body')}<b>Bracket</b><span>84.3 cm³</span><span class="muted">228 g · Aluminium 6061</span>`;
    const b = bodiesAt(activeFeatures()).find(b => b.id === st.sel.id);
    const by = st.doc.timeline.find(t => t.id === st.sel.id.split(':')[0]);
    return `${icon('body')}<b>${b?.name || 'Body'}</b><span class="muted">made by ${by?.name || 'a feature'}</span>`;
  }
  if (c === 'feature') {
    const f = st.doc.timeline.find(t => t.id === st.sel.id);
    const extra = f.type === 'sketch' ? (SKETCHES[f.id]?.summary || 'Empty') : f.info;
    return `${icon(f.type)}<b>${f.name}</b><span class="muted">${extra}</span>`;
  }
  return '';
}

// ---------------------------------------------------------------- side panel
function side() {
  const shown = st.alt ? (st.panel === 'timeline' ? 'objects' : 'timeline') : st.panel;
  const tab = (id, label, ic) => `
    <button class="tab${shown === id ? (st.alt ? ' peek' : ' on') : ''}" data-act="tab:${id}">
      ${icon(ic)}${label}${id !== st.panel && !st.alt ? '<kbd>Alt</kbd>' : ''}
    </button>`;
  return `
    <div class="tabs">${tab('timeline', 'Timeline', 'rollback')}${tab('objects', 'Objects', 'body')}</div>
    <div class="list">${shown === 'timeline' ? timeline() : objects()}</div>`;
}

// The timeline with the rollback marker in it: after the last feature, or
// after the sketch being edited, which rolls the model back to it.
function timeline() {
  const d = st.doc, tl = d.timeline;
  if (!tl.length) return `<div class="empty-note">No features yet.<br>Pick an origin plane and press <kbd>S</kbd> to start a sketch.</div>`;
  const editing = st.mode === 'sketch';
  const cut = editing ? tl.findIndex(t => t.id === st.sketch.id) + 1 : tl.length;
  const seq = [...tl.slice(0, cut), { marker: true }, ...tl.slice(cut)];
  return seq.map((t, i) => {
    if (t.marker) return `<div class="marker">${editing ? 'Rolled back' : ''}</div>`;
    const cls = ['row'];
    if ((editing && t.id === st.sketch.id) || st.op?.editing === t.id) cls.push('editing');
    else if (i > cut) cls.push('rolled');
    if (st.sel?.kind === 'feature' && st.sel.id === t.id) cls.push('sel');
    return `<div class="${cls.join(' ')}" data-act="tl:${t.id}">
      <span class="ic">${icon(t.type)}</span><span class="name">${t.name}</span><span class="meta">${t.meta || ''}</span></div>`;
  }).join('');
}

function objects() {
  const d = st.doc, hid = d.hidden;
  const row = (id, name, ic, depth, { group, sel, eye = true } = {}) => {
    const off = hid.has(id);
    return `<div class="row${group ? ' group' : ''}${sel ? ' sel' : ''}${off ? ' dim' : ''}" data-act="obj:${id}" style="padding-left:${8 + depth * 16}px">
      <span class="ic">${icon(group ? 'chev' : ic)}</span><span class="name">${name}</span>
      ${eye ? `<button class="eye${off ? ' off' : ''}" data-act="eye:${id}" title="${off ? 'Show' : 'Hide'}">${icon(off ? 'eyeoff' : 'eye')}</button>` : ''}
    </div>`;
  };
  const sketches = d.timeline.filter(t => t.type === 'sketch');
  let out = row('origin', 'Origin', 'origin', 0, { group: true });
  if (!hid.has('origin')) {
    for (const p of ['xy', 'xz', 'yz']) out += row('plane-' + p, p.toUpperCase() + ' plane', 'plane', 1, { sel: st.sel?.id === 'plane-' + p, eye: false });
  }
  const bodies = bodiesAt(activeFeatures());
  out += row('bodies', `Bodies <span class="meta">${bodies.length}</span>`, '', 0, { group: true, eye: false });
  for (const b of bodies) out += row(b.id, b.name, 'body', 1, { sel: st.sel?.kind === 'body' && st.sel.id === b.id });
  out += row('sketches', `Sketches <span class="meta">${sketches.length}</span>`, '', 0, { group: true, eye: false });
  for (const s of sketches) out += row(s.id, s.name, 'sketch', 1, { sel: st.sel?.id === s.id });
  return out;
}

// ---------------------------------------------------------------- viewport
// The grid's plane: the XY plane, or the sketch's being edited, as an origin
// and unit x and y axes.
function gridPlane() {
  if (st.mode !== 'sketch') return { o: [0, 0, 0], x: [1, 0, 0], y: [0, 1, 0] };
  const f = PLANES[st.sketch.plane].f, o = f(0, 0);
  return { o, x: norm(sub(f(1, 0), o)), y: norm(sub(f(0, 1), o)) };
}

// The app's grid (`fs_grid` in crates/render/src/shaders/scene.wgsl), ported
// per pixel onto a canvas under the scene: unbounded on its plane, its spacing
// a power of ten keeping the finest lines at least 16 px apart where each
// pixel looks, each level fading into the next as it gets denser, faded out
// from 1.5 to 6 view heights of the target and at grazing angles.
const GRID_COLOR = '#737d8a';
function drawGrid() {
  const svg = $('#scene'), cv = $('#grid');
  if (!svg || !cv) return;
  const W = svg.clientWidth, H = svg.clientHeight;
  if (!W || !H) return;
  if (cv.width !== W || cv.height !== H) Object.assign(cv, { width: W, height: H });
  // Canvas pixel to scene units: the scene's viewBox, fitted as "meet".
  const vb = svg.viewBox.baseVal, k = Math.max(vb.width / W, vb.height / H);
  const x0 = vb.x + vb.width / 2 - W * k / 2, y0 = vb.y + vb.height / 2 - H * k / 2;
  const g = gridPlane(), n = cross(g.x, g.y), { T, d, r, u, D } = proj, persp = D !== Infinity;
  const to = sub(T, g.o), D_ = persp ? D : 0;
  // Linear forms in the screen point (sx, sy) of the point q on the view
  // plane at the target, and of the ray's direction, along n, x and y.
  const lin = v => [dot(to, v), dot(r, v), -dot(u, v), dot(d, v)];
  const [N, X, Y] = [lin(n), lin(g.x), lin(g.y)];
  // Plane coordinates of the pixel's ray, with how squarely it meets the
  // plane, or null if it doesn't ahead of the eye.
  const px = new Float32Array((W + 1) * (H + 1)), py = new Float32Array(px.length), ok = new Uint8Array(px.length);
  const cosA = new Float32Array(px.length);
  for (let j = 0; j <= H; j++) {
    const sy = y0 + (j + 0.5) * k;
    for (let i = 0; i <= W; i++) {
      const sx = x0 + (i + 0.5) * k, at = j * (W + 1) + i;
      const qn = N[0] + N[1] * sx + N[2] * sy, qx = X[0] + X[1] * sx + X[2] * sy, qy = Y[0] + Y[1] * sx + Y[2] * sy;
      const dn = persp ? -D_ * N[3] + N[1] * sx + N[2] * sy : -N[3];
      const dx = persp ? -D_ * X[3] + X[1] * sx + X[2] * sy : -X[3];
      const dy = persp ? -D_ * Y[3] + Y[1] * sx + Y[2] * sy : -Y[3];
      if (Math.abs(dn) < 1e-12) continue;
      const t = -qn / dn;
      if (persp && t <= -1) continue;
      px[at] = qx + t * dx; py[at] = qy + t * dy; ok[at] = 1;
      cosA[at] = Math.abs(dn) / (persp ? Math.hypot(D_, sx, sy) : 1);
    }
  }
  const fc = add(T, sub(mul(r, cam.px), mul(u, cam.py))), fx = dot(sub(fc, g.o), g.x), fy = dot(sub(fc, g.o), g.y);
  const extent = H * k;
  const smooth = (a, b, x) => { const t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); };
  const weight = x => clamp((x + 1) * 0.25, 0, 0.6);
  const lines = (cx, cy, wx, wy, s) => {
    const lx = Math.abs(((cx / s - 0.5) % 1 + 1) % 1 - 0.5) / Math.max(wx / s, 1e-6);
    const ly = Math.abs(((cy / s - 0.5) % 1 + 1) % 1 - 0.5) / Math.max(wy / s, 1e-6);
    return 1 - Math.min(lx, ly, 1);
  };
  const ctx = cv.getContext('2d'), img = ctx.createImageData(W, H), out = img.data;
  const [cr, cg, cb] = [1, 3, 5].map(i => parseInt(GRID_COLOR.slice(i, i + 2), 16));
  for (let j = 0; j < H; j++) {
    for (let i = 0; i < W; i++) {
      const at = j * (W + 1) + i, ar = at + 1, dn = at + W + 1;
      if (!ok[at] || !ok[ar] || !ok[dn]) continue;
      const cx = px[at], cy = py[at];
      // fwidth: the change over a pixel across and down.
      const wx = Math.abs(px[ar] - cx) + Math.abs(px[dn] - cx), wy = Math.abs(py[ar] - cy) + Math.abs(py[dn] - cy);
      const level = Math.log10(Math.max(Math.hypot(wx, wy), 1e-6) * 16), s = 10 ** Math.floor(level), f = level - Math.floor(level);
      const l = Math.max(lines(cx, cy, wx, wy, s) * weight(-f), lines(cx, cy, wx, wy, s * 10) * weight(1 - f), lines(cx, cy, wx, wy, s * 100) * weight(2 - f));
      const fade = (1 - smooth(extent * 1.5, extent * 6, Math.hypot(cx - fx, cy - fy))) * smooth(0.02, 0.15, cosA[at]);
      const a = l * fade;
      if (a <= 0) continue;
      const o = (j * W + i) * 4;
      out[o] = cr; out[o + 1] = cg; out[o + 2] = cb; out[o + 3] = Math.round(a * 255);
    }
  }
  ctx.putImageData(img, 0, 0);
}

function originSvg() {
  const planes = [
    { id: 'plane-yz', p: [[0, 0, 0], [0, 40, 0], [0, 40, 40], [0, 0, 40]], lab: 'YZ', lp: [0, 34, 36] },
    { id: 'plane-xz', p: [[0, 0, 0], [40, 0, 0], [40, 0, 40], [0, 0, 40]], lab: 'XZ', lp: [34, 0, 36] },
    { id: 'plane-xy', p: [[0, 0, 0], [40, 0, 0], [40, 40, 0], [0, 40, 0]], lab: 'XY', lp: [34, 34, 0] },
  ].map(p => ({ ...p, z: depthOf(p.p) })).sort((a, b) => a.z - b.z);
  let out = '';
  for (const p of planes) {
    const [lx, ly] = proj.P(...p.lp);
    out += `<polygon class="oplane${st.sel?.id === p.id ? ' sel' : ''}" data-face="${p.id}" points="${pts(p.p)}"/><text class="oplane-label" x="${lx}" y="${ly}">${p.lab}</text>`;
  }
  return out;
}

// The X, Y and Z axes' colours, the app's (`AXES` in crates/view/src/theme.rs).
const AXIS_COLORS = ['#c24740', '#59a14f', '#4073c4'];

// The grid's x and y axis lines, as the app's: through its origin, on to
// the horizon, not faded, over the grid but under the model, each in the
// colour of the world axis it lies along, or the grid's.
function gridAxesSvg(g) {
  let out = '';
  for (const a of [g.x, g.y]) {
    // Each way from the origin, short of the eye in perspective.
    const reach = s => {
      const dd = s * dot(a, proj.d), d0 = proj.depth(g.o);
      return proj.D === Infinity || dd <= 0 ? 1e5 : Math.min(1e5, (0.98 * proj.D - d0) / dd);
    };
    const world = [0, 1, 2].find(i => Math.abs(a[i]) > 1 - 1e-6);
    const line = lineP(add(g.o, mul(a, -reach(-1))), add(g.o, mul(a, reach(1))), 'axis');
    out += line.replace('class="axis"', `class="axis" stroke="${world === undefined ? GRID_COLOR : AXIS_COLORS[world]}"`);
  }
  return `<g style="pointer-events:none">${out}</g>`;
}

// The app's origin marker: a ring lying in the XY plane, its widest 10 px
// on screen at any zoom, and a dot at the origin, each white with a dark rim.
function originMarkerSvg() {
  const k = unitsPerPixel(), h = 1e-3, [ox, oy] = proj.P(0, 0, 0);
  const img = v => { const [x, y] = proj.P(...mul(v, h)); return [(x - ox) / h, (y - oy) / h]; };
  const ex = img([1, 0, 0]), ey = img([0, 1, 0]);
  const at = t => [Math.cos(t) * ex[0] + Math.sin(t) * ey[0], Math.cos(t) * ex[1] + Math.sin(t) * ey[1]];
  const ts = Array.from({ length: 48 }, (_, i) => i * Math.PI / 24);
  const widest = Math.max(...ts.map(t => Math.hypot(...at(t))), 1e-9), sc = 10 * k / widest;
  const ring = ts.map(t => at(t).map((c, j) => ((j ? oy : ox) + c * sc).toFixed(3)).join(',')).join(' ');
  return `<g style="pointer-events:none">
    <polygon class="origin-ring rim" points="${ring}"/><polygon class="origin-ring core" points="${ring}"/>
    <circle cx="${ox}" cy="${oy}" r="${3.5 * k}" fill="#333840"/><circle cx="${ox}" cy="${oy}" r="${2.5 * k}" fill="#fff"/>
  </g>`;
}

function sketchGeomSvg(id, planeKey, withDims) {
  const s = SKETCHES[id];
  if (!s) return '';
  const f = PLANES[planeKey].f, at = uv => proj.P(...f(...uv));
  const extra = withDims ? '' : ' sk-visible';
  let out = '';
  const pt = p => { const [x, y] = proj.P(...p); return `<circle class="sk-pt" cx="${x}" cy="${y}" r="1"/>`; };
  if (s.closed) {
    const ps = s.closed.map(([u, v]) => f(u, v));
    out += `<polygon class="sk${extra}" points="${pts(ps)}"/>`;
    if (withDims) out += ps.map(pt).join('');
  }
  for (const c of s.circles || []) {
    out += `<polygon class="sk${extra}" points="${pts(circ(f, c.c[0], c.c[1], c.r))}"/>`;
    if (withDims) out += pt(f(...c.c));
  }
  if (withDims) {
    for (const d of s.dims || []) {
      const a = at(d.a), b = at(d.b);
      out += `<line class="sk-dim" x1="${a[0]}" y1="${a[1]}" x2="${b[0]}" y2="${b[1]}"/>`;
      out += `<circle class="sk-dim-end" cx="${a[0]}" cy="${a[1]}" r=".7"/><circle class="sk-dim-end" cx="${b[0]}" cy="${b[1]}" r=".7"/>`;
      out += `<text class="sk-label" x="${(a[0] + b[0]) / 2}" y="${(a[1] + b[1]) / 2}">${d.t}</text>`;
    }
    for (const l of s.labels || []) { const [x, y] = at(l.p); out += `<text class="sk-label" x="${x}" y="${y}">${l.t}</text>`; }
  }
  return out;
}

function sceneInner() {
  const d = st.doc;
  proj = makeProj(cam.yaw, cam.pitch, d.T, st.persp ? eyeDistance() : Infinity);
  const inSketch = st.mode === 'sketch';
  const active = activeFeatures();
  const plane = gridPlane();
  const bodies = bodiesAt(active);
  // An operation being set up adds its preview, highlights and pick targets.
  const pv = st.op ? page.op.preview(bodies, active) : null;

  let model = '';
  if ((!d.hidden.has('origin') || pv?.planes) && !(inSketch && d.kind === 'bracket')) model += originSvg();
  const faces = bodies.filter(b => !d.hidden.has(b.id))
    .flatMap(b => b.faces.map(f => ({ ...f, body: b.id, fade: pv?.fade?.has(b.id), tint: pv?.tint?.get(b.id) })));
  faces.push(...(pv?.faces || []));
  if (faces.length) model += solidSvg(faces, proj, pv?.marks || new Set());

  let overlay = '';
  if (inSketch) {
    overlay += sketchGeomSvg(st.sketch.id, st.sketch.plane, true);
  } else if (d.kind === 'bracket') {
    // Sketches show when made visible.
    for (const id of Object.keys(SKETCHES)) if (!d.hidden.has(id)) overlay += sketchGeomSvg(id, SKETCHES[id].plane, false);
  }
  const marker = !d.hidden.has('origin') && !(inSketch && d.kind === 'bracket') ? originMarkerSvg() : '';
  // The marker is over the model and the finished sketches, under the sketch being edited.
  overlay = inSketch ? marker + overlay : overlay + marker;
  return `${gridAxesSvg(plane)}<g class="${inSketch ? 'ghost' : ''}">${model}</g><g style="pointer-events:none">${overlay}</g><g class="opl">${pv?.svg || ''}</g>`;
}

// The view cube, as the app's (crates/view/src/view_cube.rs): 116 px, room
// around the cube for the X, Y and Z axes, each along an edge of the cube in
// sight and on past it to an arrowhead and its letter.
const CUBE = 116, CUBE_HALF = 25;
function cubeInner() {
  const pr = makeProj(cam.yaw, cam.pitch, [0, 0, 0]);
  const s = CUBE_HALF, m = CUBE / 2, dark = isDark();
  const L = v => [dot(v, pr.r), -dot(v, pr.u)];
  const W = v => L(v).map(x => x + m);
  const faces = [
    { n: [0, 0, 1], k: 'T', name: 'Top', u: [1, 0, 0], v: [0, -1, 0] },
    { n: [0, 0, -1], k: 'U', name: 'Under', u: [1, 0, 0], v: [0, 1, 0] },
    { n: [0, -1, 0], k: 'F', name: 'Front', u: [1, 0, 0], v: [0, 0, -1] },
    { n: [0, 1, 0], k: 'B', name: 'Back', u: [-1, 0, 0], v: [0, 0, -1] },
    { n: [1, 0, 0], k: 'R', name: 'Right', u: [0, 1, 0], v: [0, 0, -1] },
    { n: [-1, 0, 0], k: 'L', name: 'Left', u: [0, -1, 0], v: [0, 0, -1] },
  ].filter(f => dot(f.n, pr.d) > 1e-3);
  const axes = cubeAxes(pr, W);
  let out = cubeAxesSvg(axes, false);
  for (const f of faces) {
    const c = f.n.map(x => x * s);
    const corner = (a, b) => W([0, 1, 2].map(i => c[i] + (a * f.u[i] + b * f.v[i]) * s)).map(x => x.toFixed(2)).join(',');
    const i = Math.max(0, dot(f.n, pr.light));
    const fill = dark ? `hsl(258 8% ${(30 + 20 * i).toFixed(1)}%)` : `hsl(258 10% ${(80 + 16 * i).toFixed(1)}%)`;
    const [cx, cy] = W(c), [a, b] = L(f.u), [cc, dd] = L(f.v);
    out += `<polygon style="--f:${fill}" points="${[corner(-1, -1), corner(1, -1), corner(1, 1), corner(-1, 1)].join(' ')}"><title>${f.name}</title></polygon>
      <text transform="matrix(${a} ${b} ${cc} ${dd} ${cx} ${cy})">${f.k}</text>`;
  }
  return out + cubeAxesSvg(axes, true);
}

// The axes on the cube: of the four edges along each, those on a face turned
// to the camera; of those, one whose part past the cube nothing of the cube
// hides if any, then the lowest and furthest left on screen, so they gather
// at the cube's bottom left like a triad. None for an axis seen end on.
function cubeAxes(pr, W) {
  const s = CUBE_HALF, facing = n => dot(n, pr.d) > 1e-3;
  // Whether the cube hides p, a point on or outside it.
  const hidden = p => {
    const inside = s * (1 - 1e-3);
    let near = 1e-3, far = Infinity;
    for (let i = 0; i < 3; i++) {
      const o = p[i], d = pr.d[i];
      if (Math.abs(d) < 1e-9) { if (Math.abs(o) >= inside) return false; continue; }
      const a = (-inside - o) / d, b = (inside - o) / d;
      near = Math.max(near, Math.min(a, b));
      far = Math.min(far, Math.max(a, b));
    }
    return near < far;
  };
  const E = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
  return E.map((along, index) => {
    const [px, py] = [dot(along, pr.r), -dot(along, pr.u)], len = Math.hypot(px, py);
    if (len < 0.05) return null;
    const dir = [px / len, py / len], u = E[(index + 1) % 3], v = E[(index + 2) % 3];
    let best = null, bestScore = null;
    for (const [a, b] of [[-1, -1], [1, -1], [1, 1], [-1, 1]]) {
      if (!facing(mul(u, a)) && !facing(mul(v, b))) continue;
      const start = mul(sub(add(mul(u, a), mul(v, b)), along), s), end = add(start, mul(along, 2 * s)), tip = add(end, mul(along, 12));
      let tipShown = true;
      for (let i = 1; i <= 8; i++) if (hidden(add(end, mul(sub(tip, end), i / 8)))) tipShown = false;
      const [S, Ee, T] = [W(start), W(end), W(tip)];
      const letter = [0, 1].map(j => clamp(T[j] + dir[j] * 8, 6, CUBE - 6));
      const score = [tipShown ? 1 : 0, (S[1] + Ee[1]) / 2 - (S[0] + Ee[0]) / 2];
      if (!best || score[0] > bestScore[0] || (score[0] === bestScore[0] && score[1] > bestScore[1])) {
        best = { index, start: S, end: Ee, tip: T, dir, letter, tipShown };
        bestScore = score;
      }
    }
    return best;
  }).filter(Boolean);
}

// The axes' parts drawn over the faces if `over`, else those under them.
// Their edges are always over: each runs along a face turned to the camera.
function cubeAxesSvg(axes, over) {
  const f = p => p.map(x => x.toFixed(2)).join(',');
  const LETTERS = [
    [[[-0.38, -0.5], [0.38, 0.5]], [[0.38, -0.5], [-0.38, 0.5]]],
    [[[-0.38, -0.5], [0, 0], [0.38, -0.5]], [[0, 0], [0, 0.5]]],
    [[[-0.38, -0.5], [0.38, -0.5], [-0.38, 0.5], [0.38, 0.5]]],
  ];
  let out = '';
  for (const a of axes) {
    let g = over ? `<polyline class="ax" points="${f(a.start)} ${f(a.end)}"/>` : '';
    if (a.tipShown === over) {
      const base = [0, 1].map(j => a.tip[j] - a.dir[j] * 6);
      const room = Math.hypot(a.tip[0] - a.end[0], a.tip[1] - a.end[1]) > 6;
      g += `<polyline class="ax" points="${f(a.end)} ${f(room ? base : a.tip)}"/>`;
      if (room) {
        const side = [-a.dir[1] * 3, a.dir[0] * 3];
        g += `<polygon class="ax head" points="${f(a.tip)} ${f([base[0] + side[0], base[1] + side[1]])} ${f([base[0] - side[0], base[1] - side[1]])}"/>`;
      }
      for (const stroke of LETTERS[a.index]) {
        g += `<polyline class="ax letter" points="${stroke.map(([x, y]) => f([a.letter[0] + x * 8, a.letter[1] + y * 8])).join(' ')}"/>`;
      }
    }
    if (g) out += `<g style="--ax:${AXIS_COLORS[a.index]}">${g}</g>`;
  }
  return out;
}

// ---------------------------------------------------------------- render
// Everything is rebuilt on each change, so the focused field ([data-field]),
// its caret and the operation panel's scroll carry over.
function render() {
  const a = document.activeElement, field = a?.dataset?.field, caret = field && [a.selectionStart, a.selectionEnd];
  const scroll = $('.opp-body')?.scrollTop;
  renderApp();
  drawGrid();
  const pane = $('.opp-body');
  if (pane && scroll) pane.scrollTop = scroll;
  const el = field && $(`[data-field="${field}"]`);
  if (el) { el.focus(); el.setSelectionRange(...caret); }
  page.afterRender?.();
}

function renderApp() {
  const app = $('#app');
  const help = st.help ? helpModal() : '';
  if (!st.doc) {
    app.innerHTML = titlebar() + page.welcome() + statusbar() + help;
    return;
  }
  const hint = st.doc.kind === 'empty' && !st.doc.timeline.length && st.mode === 'model' && !st.pick && !st.sel
    ? `<div class="hint-center">Select an origin plane, then <kbd>S</kbd> to sketch</div>` : '';
  app.innerHTML = titlebar() + toolbar() + `
    <div class="side">${side()}</div>
    ${rail()}${railPop()}
    <div class="vp">
      <canvas id="grid"></canvas>
      <svg id="scene" viewBox="${viewBox()}" preserveAspectRatio="xMidYMid meet">${sceneInner()}</svg>
      <div class="cube-wrap">
        <svg id="cube" class="cube" viewBox="0 0 116 116">${cubeInner()}</svg>
        <button class="home-btn" title="Home view">${icon('home')}</button>
      </div>
      ${st.op ? page.op.panel() : ''}
      ${hint}
    </div>` + (st.alt || st.gbarPinned ? gbar() : '') + statusbar() + help;
  placeRail();
}

function helpModal() {
  const kv = (k, v) => `<div class="kv"><span>${v}</span><span>${k.split(' ').map(x => `<kbd>${x}</kbd>`).join(' ')}</span></div>`;
  return `<div class="modal-bg" data-act="help"><div class="modal" data-act="noop">
    <h2>Keyboard &amp; mouse</h2>
    <div class="cols">
      <h3>Everywhere</h3>
      ${kv('Ctrl K', 'Command palette')}${kv('?', 'This sheet')}
      ${kv('Esc', 'Back out one level')}${kv('Space', 'Clear the selection')}${kv('Q W E R T', 'Open a tool set from the rail')}${kv('Alt', 'Hold: other tab + tool bar')}${kv('Alt', 'Tap: keep the tool bar open')}
      <h3>Model</h3>
      ${kv('S', 'Sketch')}${kv('X', 'Extrude')}${kv('O', 'Revolve')}${kv('U', 'Press pull')}${kv('H', 'Hole')}${kv('F', 'Fillet')}
      ${kv('C', 'Chamfer')}${kv('B', 'Combine')}${kv('M', 'Move')}${kv('P', 'Linear pattern')}${kv('I', 'Measure')}
      <h3>Operation panel</h3>
      ${kv('Enter', 'OK')}${kv('Esc', 'Cancel')}
      <div class="kv"><span>Pick into a field</span><span>click the field, then ${mouse('l')} in the view</span></div>
      <h3>Sketch</h3>
      ${kv('L', 'Line')}${kv('B', 'Rectangle')}${kv('C', 'Circle')}${kv('A', 'Arc')}${kv('T', 'Trim')}${kv('D', 'Dimension')}
      <h3>Mouse</h3>
      <div class="kv"><span>Select face</span><span>${mouse('l')} click</span></div>
      <div class="kv"><span>Select body</span><span>${mouse('l')} double-click</span></div>
      <div class="kv"><span>Orbit</span><span>${mouse('l')} / ${mouse('m')} drag</span></div>
      <div class="kv"><span>Orbit about a point</span><span>${mouse('m')} click the model or grid</span></div>
      <div class="kv"><span>Pan</span><span>${mouse('r')} drag</span></div>
      <div class="kv"><span>Zoom</span><span>${mouse('w')} wheel</span></div>
      <div class="kv"><span>Reorder / roll back</span><span>drag in timeline</span></div>
    </div>
    <p>The toolbar only lists operations that apply to the current context and selection. Shortcut letters follow the same list, so what you see is what the keyboard does.</p>
  </div></div>`;
}

// ---------------------------------------------------------------- actions
let toastTimer;
function toast(msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 1400);
}

function isDark() {
  const t = root.dataset.theme;
  return t ? t === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches;
}

function act(a) {
  const [k, ...rest] = a.split(':');
  const v = rest.join(':');
  if (k !== 'menu') st.menu = false;
  if (k !== 'projmenu') st.projMenu = false;
  switch (k) {
    case 'new': go('model.html', 'empty'); return;
    case 'open': go('model.html', v); return;
    case 'home': go('welcome.html'); return;
    case 'menu': st.menu = !st.menu; break;
    case 'theme': root.dataset.theme = isDark() ? 'light' : 'dark'; break;
    case 'help': st.help = !st.help; break;
    case 'toast': toast(v); return;
    case 'projmenu': st.projMenu = !st.projMenu; break;
    case 'proj': st.persp = v === 'persp'; break;
    case 'mousehints': st.mouseHints = !st.mouseHints; break;
    case 'op': page.doOp(v); break;
    case 'gop': altUsed = true; globalOp(v); break;
    case 'rail': st.railOpen = +v; break;
    case 'rgop': st.railOpen = null; globalOp(v); break;
    case 'tab': st.panel = v; break;
    case 'tl':
      if (st.mode === 'sketch' || st.op) return;
      st.pick = false;
      st.sel = st.sel?.id === v ? null : { kind: 'feature', id: v };
      break;
    case 'eye': {
      const h = st.doc.hidden;
      h.has(v) ? h.delete(v) : h.add(v);
      break;
    }
    case 'obj':
      if (st.op) { page.op.pickObject(v); break; }
      if (bodiesAt(activeFeatures()).some(b => b.id === v)) st.sel = { kind: 'body', id: v };
      else if (v.startsWith('plane-')) st.sel = { kind: 'plane', id: v };
      else if (st.doc.timeline.some(t => t.id === v)) st.sel = { kind: 'feature', id: v };
      else return;
      break;
    default: if (!page.act?.(k, v)) return;
  }
  render();
}

// ---------------------------------------------------------------- input
let altUsed = false, altDownAt = 0;

// Instant tooltips: a [data-tip] under the pointer shows one beside it, no delay.
document.addEventListener('mousemove', e => {
  const t = e.target.closest?.('[data-tip]'), tip = $('#tip');
  if (!t) { tip.hidden = true; return; }
  const win = $('#win').getBoundingClientRect(), r = t.getBoundingClientRect();
  tip.innerHTML = t.dataset.tip + (t.dataset.key ? `<kbd>${t.dataset.key}</kbd>` : '');
  tip.style.left = (r.right - win.left + 6) + 'px';
  tip.style.top = (r.top + r.height / 2 - win.top) + 'px';
  tip.hidden = false;
});
document.addEventListener('mousedown', () => { $('#tip').hidden = true; });

// Pointing at a set opens its list. Pointing at a tool on a card closes it at
// once; anywhere else outside a set and its list closes it after a moment, so
// the pointer can cross the gap from a card to its list.
let railClose = 0;
const closeRail = () => { clearTimeout(railClose); railClose = 0; if (st.railOpen != null) { st.railOpen = null; render(); } };
document.addEventListener('mouseover', e => {
  const set = e.target.closest('[data-rail]');
  if (set || e.target.closest('.rpop')) { clearTimeout(railClose); railClose = 0; }
  if (set) { if (st.railOpen !== +set.dataset.rail) { st.railOpen = +set.dataset.rail; render(); } }
  else if (st.railOpen == null || e.target.closest('.rpop')) return;
  else if (e.target.closest('.rtool')) closeRail();
  else if (!railClose) railClose = setTimeout(closeRail, 250);
});

document.addEventListener('click', e => {
  if (st.railOpen != null && !e.target.closest('.rail, .rpop')) { st.railOpen = null; render(); }
  const el = e.target.closest('[data-act]');
  if (el) {
    if (el.dataset.act !== 'noop') act(el.dataset.act);
    return;
  }
  if (st.op && e.target.closest('#scene')) {
    // While an operation is set up, clicks in the view feed its panel.
    const p = e.target.closest('[data-pick], [data-face], [data-body]');
    st.menu = false;
    if (p) page.op.pick(p);
    render();
    return;
  }
  const f = e.target.closest('[data-face]');
  if (f) { st.menu = false; page.faceClick(f.dataset.face); render(); return; }
  const b = e.target.closest('[data-body]');
  if (b && st.mode === 'model' && !st.pick) { st.menu = false; st.sel = { kind: 'body', id: b.dataset.body }; render(); return; }
  if (e.target.closest('#scene')) {
    if (st.mode === 'sketch' && st.tool) toast(TOOLS[st.tool].hint + ' — drawing not in mock');
    else st.sel = null;
    st.menu = st.projMenu = false;
    render();
  } else if ((st.menu || st.projMenu) && !e.target.closest('.file, .projdd')) { st.menu = st.projMenu = false; render(); }
});
document.addEventListener('keydown', e => {
  if (e.key === 'Alt') {
    e.preventDefault();
    if (!e.repeat && st.doc) { st.alt = true; altUsed = false; altDownAt = performance.now(); render(); }
    return;
  }
  // In a text field keys are text, but Enter and Esc go to the toolbar's
  // operations with those keys: an operation panel's OK and Cancel.
  if (e.target.matches?.('input')) {
    const o = (e.key === 'Enter' || e.key === 'Escape') && ops().find(o => o.key === (e.key === 'Escape' ? 'Esc' : 'Enter'));
    if (o) { e.preventDefault(); page.doOp(o.id); render(); }
    return;
  }
  if (e.altKey) altUsed = true;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key === '?') { st.help = !st.help; render(); return; }
  if (st.help) { if (e.key === 'Escape') { st.help = false; render(); } return; }

  const key = e.key === 'Delete' ? 'Del' : e.key.length === 1 ? e.key.toUpperCase() : e.key;
  if (!st.doc) {
    if (key === 'N') go('model.html', 'empty');
    if (key === 'O') go('model.html', 'bracket');
    return;
  }
  if (st.railOpen != null) {
    const items = railSets()[st.railOpen][1], i = railLetters(items).indexOf(key);
    if (i >= 0) { e.preventDefault(); act('rgop:' + items[i].op); return; }
  }
  const set = key.length === 1 ? RAIL_SET_KEYS.indexOf(key) : -1;
  if (set >= 0 && railSets()[set]) { st.railOpen = st.railOpen === set ? null : set; render(); return; }
  if (e.key === 'Escape') {
    if (st.railOpen != null) st.railOpen = null;
    else if (st.menu) st.menu = false;
    else if (st.projMenu) st.projMenu = false;
    else if (st.gbarPinned) st.gbarPinned = false;
    else if (st.op) st.op = null;
    else if (st.tool) st.tool = null;
    else if (st.pick) st.pick = false;
    else if (st.sel) st.sel = null;
    else page.escape?.();
    render();
    return;
  }
  if (e.key === ' ' && st.sel && !st.op) { e.preventDefault(); st.sel = null; render(); return; }
  const o = ops().find(o => o.key === key);
  if (o) { e.preventDefault(); page.doOp(o.id); render(); }
});

document.addEventListener('keyup', e => {
  if (e.key !== 'Alt') return;
  e.preventDefault();
  if (!st.alt) return;
  st.alt = false;
  // A quick tap on its own pins the bar open (or closes it again).
  if (!altUsed && performance.now() - altDownAt < 300) st.gbarPinned = !st.gbarPinned;
  render();
});
window.addEventListener('blur', () => { if (st.alt) { st.alt = false; render(); } });
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', render);
addEventListener('resize', render);
// Following a link to this page with another hash starts it afresh.
addEventListener('hashchange', () => location.reload());

// ---------------------------------------------------------------- pages
// Opens another page of the mock, with its hash (`model.html`, 'bracket/face'),
// in the theme picked here.
function go(file, hash = '') {
  const theme = root.dataset.theme;
  const parts = [hash, theme].filter(Boolean).join('/');
  location.href = file + (parts ? '#' + parts : '');
}
// The hash's parts after the first, as flags ('face', 'dark', 'op:fillet').
const HASH = location.hash.slice(1).split('/');
const flag = f => HASH.slice(1).includes(f);

// The links between the pages, then the page itself. Before this, the page
// sets up the state; the shared flags in the hash apply here.
const PAGES = [['welcome', 'Welcome', 'welcome.html', ''], ['model', 'Model', 'model.html', 'bracket'], ['sketch', 'Sketch', 'sketch.html', 's1']];
function start(current) {
  if (flag('still')) root.classList.add('still');
  if (flag('dark')) root.dataset.theme = 'dark';
  if (flag('light')) root.dataset.theme = 'light';
  if (flag('persp')) st.persp = true;
  if (flag('projmenu')) st.projMenu = true;
  if (flag('nomouse')) st.mouseHints = false;
  if (flag('tools')) st.gbarPinned = true;
  const nav = document.createElement('nav');
  nav.className = 'pages';
  nav.innerHTML = PAGES.map(([id, label, file, hash]) => `<a class="${id === current ? 'on' : ''}" href="${file}${hash && '#' + hash}" data-go="${file}" data-hash="${hash}">${label}</a>`).join('');
  nav.addEventListener('click', e => {
    const a = e.target.closest('a');
    if (!a) return;
    e.preventDefault();
    go(a.dataset.go, a.dataset.hash);
  });
  document.body.prepend(nav);
  render();
}
