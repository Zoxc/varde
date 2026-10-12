// The varde UI mock's MCP server: the settings page's MCP tab and the
// status bar's chip while a client is connected (notes/AgentSupport.md). Loaded
// after mock.js on every page; mock.js calls `window.mcp` where these go:
// settingsTab() among the settings' tabs, chip() in the status bar,
// act(k, v) for its data-acts, start() for the hash's flags, button() in
// the toolbar and panel() on the right of the document screen.
// Desktop only, as the Files tab: a page can't listen. The server is off
// by default; one client at a time (Claude Code here), whose edits apply
// as the user's would, one undo step each. The panel (its toggle shown
// while the server is on) lists the client's calls, newest last: what
// each read or applied, and what regeneration made of an edit. A failing
// edit stays applied. Not a review surface: nothing waits on it. Each
// edit row keeps the design as it was before and after the call (in
// memory only, the oldest rows losing it first): Revert to here, or to
// before it, makes the design that again as one undoable change, asking
// first when changes after it (the user's or the client's) would go.
// The client is told in its next call's result.

(() => {
Object.assign(I, {
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
  // A speech bubble with a spark: the panel's toggle.
  agent: '<path d="M4 5.5h16v11H11l-4.5 3.5v-3.5H4z"/><path class="a" d="M12 7.6l.9 2.1 2.1.9-2.1.9-.9 2.1-.9-2.1-2.1-.9 2.1-.9z"/>',
  tool: '<path d="M14.5 4.5a4.5 4.5 0 0 0-5.6 5.6L4 15v5h5l4.9-4.9a4.5 4.5 0 0 0 5.6-5.6l-2.8 2.8-2.8-.8-.8-2.8z"/>',
  stop: '<rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor"/>',
  locate: '<circle cx="12" cy="12" r="6"/><path d="M12 2.5v4M12 17.5v4M2.5 12h4M17.5 12h4"/>',
  // Revert to here: back to a mark; to before: back past it.
  revhere: '<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/><path d="M20 3v4"/>',
  revbefore: '<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/><path d="M2 3v12"/>',
  // The tools' icons, in the two colours of the tools' own (the line in
  // its category's colour, .a marks in its accent, .af hollow dots, .r
  // reference lines): queries as Inspect, edits as what they change.
  'q-summary': '<rect x="4.5" y="4" width="15" height="17" rx="1.5"/><path class="r" d="M9 4V2.8h6V4"/><path class="a" d="M8 10h8M8 13.5h8M8 17h5"/>',
  'q-feature': '<path d="M4 6.5h16M4 12h16M4 17.5h16"/><circle class="af" cx="9" cy="6.5" r="2"/><circle class="af" cx="15.5" cy="12" r="2"/><circle class="af" cx="7" cy="17.5" r="2"/>',
  'q-sketch': '<path class="r" d="M3 20h10"/><path d="M4 16c2-6 6-9 11-9"/><circle class="af" cx="4" cy="16" r="1.5"/><circle class="af" cx="15" cy="7" r="1.5"/><circle class="a" cx="17" cy="16" r="3"/><path class="a" d="M19.2 18.2l2.3 2.3"/>',
  'q-topology': '<path d="M12 3l7.8 4.5v9L12 21l-7.8-4.5v-9zM4.2 7.5L12 12l7.8-4.5M12 12v9"/><circle class="af" cx="12" cy="12" r="1.6"/><circle class="af" cx="4.2" cy="7.5" r="1.6"/><circle class="af" cx="19.8" cy="16.5" r="1.6"/><path class="a" d="M12 3l7.8 4.5"/>',
  'q-render': '<path d="M3.5 8.5h3.5l1.8-2.5h6.4l1.8 2.5h3.5V19h-17z"/><circle class="a" cx="12" cy="13.2" r="3.4"/><circle class="af" cx="17.8" cy="11" r=".9"/>',
  'e-param': '<rect x="3.5" y="5" width="17" height="14" rx="2"/><path class="a" d="M7.5 10l3 4M10.5 10l-3 4M13.5 10.5h3.5M13.5 13.5h3.5"/>',
  'e-addsketch': '<path class="r" d="M4 20h9"/><path d="M13 5.5l3.5 3.5L9 16.5H5.5V13z"/><path class="a" d="M19 13v7M15.5 16.5h7"/>',
  'e-editsketch': '<path class="r" d="M3 20h18"/><path d="M4 16c2.5-5 6-8 10-8"/><circle class="af" cx="14" cy="8" r="1.5"/><path class="a" d="M17 3.5l3.5 3.5L14 13.5h-3.5V10z"/>',
  'e-set': '<path d="M4 7h9M4 12h6M4 17h7"/><circle class="af" cx="15.5" cy="7" r="2"/><path class="a" d="M17.5 11l3 3-6 6h-3v-3z"/>',
  'e-visible': '<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle class="a" cx="12" cy="12" r="3"/>',
  'e-remove': '<path d="M5 7h14M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13"/><path class="a" d="M10 11l4 5M14 11l-4 5"/>',
  'e-rmbody': '<path d="M11 3.5l7 4v4M4 7.5v8.5l7 4M4 7.5l7-4M4 7.5l7 4 7-4M11 11.5V20"/><path class="a" d="M14.5 17.5h7"/>',
  'e-align': '<path class="r" d="M3 20.5h18"/><rect class="t" x="4" y="9" width="6" height="11.5" rx="1"/><rect x="13" y="4" width="7" height="7" rx="1"/><path class="a" d="M16.5 12.5v6M14.5 16.5l2 2 2-2"/>',
  'e-scale': '<rect class="r" x="3" y="9" width="12" height="12" rx="1" stroke-dasharray="1.6 1.8"/><rect class="t" x="3" y="13" width="8" height="8" rx="1"/><path class="a" d="M13 11l7-7M15 4h5v5"/>',
  'e-split': '<path class="t" d="M4 8l8-4 8 4v8l-8 4-8-4z"/><path class="a" d="M2.5 13.5l19-5"/>',
  'e-draft': '<path class="r" d="M7 4v16"/><path class="t" d="M3 20h16L15 4H7"/><path class="a" d="M7 8.5a4.5 4.5 0 0 1 3.3-1.1"/>',
  'e-link': '<path class="r" d="M3 20h18"/><path d="M12 3l7 4v6l-7 4-7-4V7z"/><path class="a" d="M12 17v3M8.5 20h7" stroke-dasharray="1.6 1.6"/>',
  'e-rename': '<rect class="t" x="3" y="7" width="18" height="10" rx="2"/><path class="a" d="M7 10v4M14 5.5v13M12 5.5h4M12 18.5h4"/>',
  'e-units': '<path class="t" d="M3 9h18v6H3z"/><path class="a" d="M7 9v3M11 9v2M15 9v3M19 9v2"/>',
  'e-tol': '<circle class="t" cx="12" cy="12" r="7.5"/><path class="a" d="M12 8.5v7M8.5 12h7M8.5 17.5h7"/>',
  plug: '<path d="M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4"/>',
});

for (const [cat, names] of Object.entries({
  insp: 'q-summary q-feature q-sketch q-topology q-render', mod: 'e-param e-set e-visible e-remove e-rename e-units e-tol e-draft e-scale e-split',
  sketch: 'e-addsketch e-editsketch e-link', solid: 'e-rmbody e-align',
})) for (const n of names.split(' ')) CAT[n] = cat;

const m = {
  client: false,      // Claude Code connected
  busy: false,        // the port in use, so starting failed
  tokenShown: false,
  open: false,        // the panel
  calls: [],          // the client's calls: { tool, args, state, result, feat }
  startErr: '',       // Start refused: the port in use
  yours: [],          // the user's own edits meanwhile: { at (calls before it), said }
  revertAsk: null,    // the confirm: { i, at: 'after' | 'before' }
  told: false,        // a change of the user's the client hasn't heard of yet
  ask: null,          // the client wants to change what the user has open: { name, args }
  ctx: null,          // a row's context menu: { i, x, y } in the row
};

const snap = () => ({ timeline: structuredClone(st.doc.timeline), hidden: new Set(st.doc.hidden) });

// The settings this adds, as mock.js keeps the others.
Object.assign(SET_DEFAULTS, { mcp: false, mcpPort: '7311' });
Object.assign(SET_SAID, { mcp: ['Serve over MCP'], mcpPort: ['MCP port', v => v] });
for (const k of ['mcp', 'mcpPort']) st[k] = SET_DEFAULTS[k];
DESKTOP_ONLY.push('mcp', 'mcpPort');

const portOk = () => /^\d+$/.test(st.mcpPort) && +st.mcpPort >= 1024 && +st.mcpPort <= 65535;
// Serving: switched on, and started (a port in use makes Start fail).
const serving = () => st.mcp && !st.web && !m.busy && portOk();
const TOKEN = 'vrd_7HqX2mPz9kLw4RtB';
const url = () => `http://127.0.0.1:${st.mcpPort}/mcp`;

// ---------------------------------------------------------------- the settings' tab
function settingsTab({ row, sw }) {
  if (st.web) return [];
  const bad = !portOk();
  const copy = what => `<button class="op icon" data-act="toast:${what} copied" data-tip="Copy" data-tip-at="below">${icon('copy')}</button>`;
  const cmds = [
    `claude mcp add --scope user --transport http varde ${url()} --header "Authorization: Bearer ${TOKEN}"`,
    // Codex's `mcp add` takes no header: its ~/.codex/config.toml does,
    // a table on lines of its own.
    `[mcp_servers.varde]\nurl = "${url()}"\nhttp_headers = { Authorization = "Bearer ${TOKEN}" }`,
    // opencode takes it in its config, opencode.json, under "mcp"; no
    // OAuth, which it'd otherwise try.
    `"varde": { "type": "remote", "url": "${url()}", "oauth": false, "headers": { "Authorization": "Bearer ${TOKEN}" } }`,
  ];
  return [['mcp', 'MCP', 'Lets an outside agent (Claude Code, Codex, an IDE) read and edit the open design; its edits are undone with Undo, as yours are. Only this computer can connect.', [
    row('Serve over MCP', serving() ? `On at <code>127.0.0.1:${st.mcpPort}</code>, one client at a time` : st.mcp && !st.web ? 'Not started: see the port' : 'Off', sw('mcp'), 'mcp'),
    // Its error, or a port in use (Start failed), said in the row.
    row('Port', bad ? '<span class="bad">A number from 1024 to 65535</span>'
      : st.mcp && m.busy ? `<span class="bad">Port ${st.mcpPort} is in use: choose another</span>` : 'Where it listens, on 127.0.0.1 only',
      `<input class="sfield${bad || (st.mcp && m.busy) ? ' bad' : ''}" data-field="mcp-port" value="${st.mcpPort}" size="6">`, 'mcpPort'),
    ...(serving() ? [
      row('Client', 'Connected now; Disconnect drops it', m.client
        ? `<span class="clients"><span class="mcpchip"><i></i>Claude Code</span><span class="muted">since 14:02</span><button class="op" data-act="mcp-disconnect">Disconnect</button></span>`
        : '<span class="muted">None</span>'),
    ] : []),
    row('Token', 'Clients send it with each request; a new one drops the client',
      `<code class="tok">${m.tokenShown ? TOKEN : 'vrd_•••••••••••••••'}</code><button class="op icon" data-act="mcp-token" data-tip="${m.tokenShown ? 'Hide' : 'Show'}" data-tip-at="below">${icon(m.tokenShown ? 'eyeoff' : 'eye')}</button>${copy('Token')}<button class="op" data-act="mcp-regen">Regenerate</button>`),
    `<div class="srow cmd"><div class="sl"><b>Add to a client</b><span>Claude Code: paste into a terminal. Codex: add to <code>~/.codex/config.toml</code>. opencode: add to <code>opencode.json</code> under <code>"mcp"</code>. Each carries the token</span></div><div class="cmds">
      ${cmds.map(c => `<div><code>${c.replace(/\n/g, '<br>')}</code>${copy('Command')}</div>`).join('')}</div></div>`,
  ]]];
}

// ---------------------------------------------------------------- the panel
// The toggle shows while the server is on (switched on in the settings).
const on = () => st.mcp && !st.web;
// Open, it stays while stopped, so its Start can serve again.
const button = () => st.doc && (on() || m.open) ? `<button class="op icon${m.open ? ' on' : ''}" data-act="mcp-panel" data-tip="MCP requests" data-tip-at="below">${icon('agent')}</button>` : '';

// Each tool as the panel names it: in the user's words; one not listed
// shows as the client named it.
const TOOL_SAID = {
  design_summary: 'Looked over the design', feature: 'Read the settings of', sketch: 'Read the sketch',
  body_topology: 'Listed the faces and edges of', measure: 'Measured', 
  set_parameter: 'Set parameter', add_sketch: 'New sketch', edit_sketch: 'Edit sketch', extrude: 'Extrude',
  fillet: 'Fillet', chamfer: 'Chamfer', set_feature: 'Change', remove_feature: 'Delete',
  remove_body: 'Delete body', set_visible: 'Show or hide', render: 'Took a picture of', revolve: 'Revolve',
  set_sketch_plane: 'Move sketch', add_link: 'Project into', combine: 'Combine', move_body: 'Move',
  mirror: 'Mirror', pattern: 'Pattern', align: 'Align', scale: 'Scale', split: 'Split', shell: 'Shell',
  offset_face: 'Offset face', draft: 'Draft', sweep: 'Sweep', loft: 'Loft', rename: 'Rename',
  set_color: 'Colour', set_opacity: 'Opacity', add_parameter: 'New parameter',
  rename_parameter: 'Rename parameter', remove_parameter: 'Delete parameter', set_units: 'Units',
  set_tolerance: 'Tolerance', set_rollback: 'Move the marker to',
};
const TOOL_ICON = {
  design_summary: 'q-summary', feature: 'q-feature', sketch: 'q-sketch',
  body_topology: 'q-topology', measure: 'measure', render: 'q-render',
  set_parameter: 'e-param', add_sketch: 'e-addsketch', edit_sketch: 'e-editsketch', extrude: 'extrude',
  revolve: 'revolve', fillet: 'fillet', chamfer: 'chamfer', set_feature: 'e-set', remove_feature: 'e-remove',
  remove_body: 'e-rmbody', set_visible: 'e-visible', set_sketch_plane: 'plane', add_link: 'e-link',
  combine: 'combine', move_body: 'move', mirror: 'bmirror', pattern: 'lpattern', align: 'e-align',
  scale: 'e-scale', split: 'e-split', shell: 'shell', offset_face: 'offset', draft: 'e-draft',
  sweep: 'sweep', loft: 'loft', rename: 'e-rename', set_color: 'paint', set_opacity: 'paint',
  add_parameter: 'params', rename_parameter: 'params', remove_parameter: 'params',
  set_units: 'e-units', set_tolerance: 'e-tol', set_rollback: 'rollback',
};
// A pattern's icon follows its kind: circular, or linear by default.
const toolIcon = c => c.tool === 'pattern' && /circular/.test(c.args) ? 'cpattern' : TOOL_ICON[c.tool] || 'tool';
const isEdit = c => c.state === 'applied' || c.state === 'broke';
// What would go reverting to call i's state `at`: the differences between
// that state and now, as the client's diff says them, net and with no
// author (the editor keeps no labels of steps): features gone, back or
// named again, by the Timelines; and the rest (parameters, bodies) by the
// later changes' keys, the latest of each saying it (a body hidden then
// deleted is back, shown with it).
function after(i, at) {
  const from = at === 'before' ? i : i + 1;
  const c = m.calls[i], then = c.snaps[at].timeline, now = st.doc.timeline;
  const out = [
    ...now.filter(t => !then.some(u => u.id === t.id)).map(t => `${t.name} gone`),
    ...then.filter(t => !now.some(u => u.id === t.id)).map(t => `${t.name} back`),
    ...then.filter(t => now.some(u => u.id === t.id && u.name !== t.name)).map(t => `${now.find(u => u.id === t.id).name} named ${t.name} again`),
  ];
  const later = [...m.calls.filter((d, j) => j >= from && isEdit(d) && !d.reverted && d.key), ...m.yours.filter(y => y.at >= from && !y.reverted && y.key)];
  const seen = new Set();
  // A feature's change goes unsaid where the feature itself is gone or back.
  const there = k => !k.startsWith('f:') || then.some(t => t.id === k.slice(2)) && now.some(t => t.id === k.slice(2));
  for (const d of later.reverse()) if (!seen.has(d.key) && there(d.key)) { seen.add(d.key); out.push(d.undo); }
  return out;
}
const TOOL_STATE = { done: ['check', 'Done'], applied: ['check', 'Applied'], broke: ['alert', 'Applied, fails'], failed: ['close', 'Refused'] };
// Whether call i's after is the design now: no edit of anyone's since, so
// Revert to here would change nothing.
const latest = i => !m.calls.some((c, j) => j > i && isEdit(c) && !c.reverted) && !m.yours.some(y => y.at > i && !y.reverted);
// A call's actions, in its context menu.
function actions(c, i) {
  const showable = c.feat && st.mode !== 'sketch' && st.doc.timeline.some(t => t.id === c.feat);
  const back = isEdit(c) && c.snaps;
  return [
    showable && [`mcp-show:${i}`, 'locate', 'Show in Timeline'],
    back && !latest(i) && [`mcp-revert:${i}:after`, 'revhere', 'Revert to here'],
    back && [`mcp-revert:${i}:before`, 'revbefore', 'Revert to before'],
  ].filter(Boolean);
}
// How long ago each came: the session's calls a couple of minutes apart.
const ago = i => `${Math.max(1, m.calls.length - i) * 2} min`;
function row(c, i) {
  const [ic, said] = TOOL_STATE[c.state];
  const edit = isEdit(c);
  const acts = actions(c, i);
  // The result shows in the tooltip, with what the row offers.
  const tip = [c.result || said, edit ? 'One undo step: your Undo takes it back' : '', acts.length ? 'Right-click: Show in Timeline, Revert' : '']
    .filter(Boolean).join(' · ').replace(/"/g, '&quot;');
  const menu = m.ctx?.i === i && acts.length
    ? `<div class="menu tctx" style="left:${m.ctx.x}px;top:${m.ctx.y}px">${acts.map(([a, ic, label]) => `<button data-act="${a}">${icon(ic)}${label}</button>`).join('')}</div>` : '';
  return `<div class="tcall ${c.state}${c.reverted ? ' reverted' : ''}${m.ctx?.i === i ? ' ctxon' : ''}" data-i="${i}" data-tip="${tip}" data-tip-at="below">
    <div class="tr">${icon(toolIcon(c))}<b>${TOOL_SAID[c.tool] || `<code>${c.tool}</code>`}</b><span class="ta">${c.args}</span>${c.reverted ? '<span class="rv">Reverted</span>' : ''}<span class="tt">${ago(i)}</span><span class="ts">${icon(ic)}</span></div>
    ${c.told ? `<div class="tres told"><span>Told: since its last call, ${c.told}</span></div>` : ''}
    ${menu}
  </div>${m.yours.filter(y => y.at === i + 1).map(y => `<div class="abody-note${y.reverted ? ' reverted' : ''}">You: ${y.said}${y.reverted ? ' · Reverted' : ''}</div>`).join('')}`;
}
function panel() {
  if (!m.open || st.web || !st.doc) return '';
  const port = `127.0.0.1:${st.mcpPort}`;
  const live = serving();
  const run = live
    ? `<button class="op" data-act="mcp-run:stop" data-tip="Stop the server; the client disconnects" data-tip-at="below">${icon('stop')}Stop</button>`
    : `<button class="op primary" data-act="mcp-run:start" data-tip="Serve on ${port}" data-tip-at="below">${icon('plug')}Start</button>`;
  const who = live && m.client
    ? `<span class="mcpchip" data-tip="MCP server on ${port} · Claude Code connected since 14:02" data-tip-at="below"><i></i>MCP</span><span class="aclient">Claude Code</span>
      <button class="op" data-act="mcp-disconnect" data-tip="Drop the client now" data-tip-at="below">Disconnect</button>`
    : `<span class="mcpchip off"${live ? ` data-tip="MCP server on ${port}, no client" data-tip-at="below"` : ''}><i></i>MCP</span><span class="aclient muted">${live ? 'No client' : 'Stopped'}</span>`;
  const body = !live
    ? `<div class="abody-empty">The MCP server is stopped. Start it to let clients such as Claude Code or Codex connect at <code>${port}/mcp</code>${m.startErr ? `. <span class="bad">${m.startErr}</span>` : ''}.</div>`
    : m.calls.length ? m.calls.map(row).join('')
    : `<div class="abody-empty">A client connected over MCP reads and edits the open design; its calls show here, its edits applied as yours are.</div>`;
  return `<div class="agent"><div class="ahead">${who}<span class="spacer"></span>${run}
    <button class="op icon" data-act="mcp-panel" data-tip="Hide; the server goes on" data-tip-at="below">${icon('close')}</button></div>
    <div class="abody">${body}${live && !m.client && m.calls.length ? '<div class="abody-note">Claude Code disconnected</div>' : ''}</div></div>${confirm()}${ask()}`;
}
// A client edit while the user has a sketch or panel open, with something
// unfinished in it: theirs is applied first, or the call refused. With
// nothing unfinished it doesn't ask: the session closes, the edit applies.
// Past 30 s (clients drop calls on their side) the call returns, not
// applied; the popup stays, saying the edit was cancelled, for the user to
// close: the client isn't asked to retry.
function ask() {
  const q = m.ask;
  if (!q) return '';
    // Apply yours tried, and the user's OK refused: the popup says why and
  // stays, the call still waiting.
  const refused = q.refused ? `<p class="bad">${q.refused}</p>` : '';
  if (q.cancelled) return `<div class="modal-bg"><div class="modal sreset" data-act="noop">
    <h2>Claude Code's edit was cancelled</h2>
    <p class="muted">It waited 30 s while you were editing ${q.name}, and wasn't applied: ${q.args}. Tell it to try again if you want it.</p>
    <div class="rbtns"><button class="op primary" data-act="mcp-ask:close">Close</button></div>
  </div></div>`;
  return `<div class="modal-bg"><div class="modal sreset" data-act="noop">
    <h2>Claude Code wants to change the design</h2>
    <p class="muted">You're editing ${q.name}. Apply yours and close it to let its edit (${q.args}) go ahead, or keep editing and it's told you are.</p>${refused}
    <div class="rbtns"><button class="op" data-act="mcp-ask:keep">Keep editing</button><button class="op primary" data-act="mcp-ask:apply">Apply yours</button></div>
  </div></div>`;
}
// Revert asks first when changes after the point would go, listing them.
function confirm() {
  const q = m.revertAsk;
  if (!q) return '';
  const c = m.calls[q.i], gone = after(q.i, q.at);
  return `<div class="modal-bg" data-act="mcp-revert-no"><div class="modal sreset" data-act="noop">
    <h2>Revert to ${q.at === 'before' ? `before ${c.made}` : `just after ${c.made}`}?</h2>
    <p class="muted">The design would differ from now in ${gone.length === 1 ? 'this' : 'these ' + gone.length + ' ways'}; Undo brings it back.</p>
    <div class="rlist">${gone.map(g => `<div><span>${g}</span></div>`).join('')}</div>
    <div class="rbtns"><button class="op" data-act="mcp-revert-no">Cancel</button><button class="op primary" data-act="mcp-revert-ok">Revert</button></div>
  </div></div>`;
}
// Greyed rows are stored here, as the mock has no Undo to take a revert
// back; the app works them out from the history each time.
function revert(i, at) {
  const c = m.calls[i], s = c.snaps[at], from = at === 'before' ? i : i + 1;
  st.doc.timeline = structuredClone(s.timeline); st.doc.hidden = new Set(s.hidden);
  m.calls.forEach((d, j) => { if (j >= from && isEdit(d)) d.reverted = true; });
  m.yours.forEach(y => { if (y.at >= from) y.reverted = true; });
  if (st.sel?.kind === 'feature' && !st.doc.timeline.some(t => t.id === st.sel.id)) st.sel = null;
  m.revertAsk = null; m.told = true;
  toast(`Reverted to ${at === 'before' ? 'before' : 'just after'} ${c.made}: one change, Undo takes it back`);
}

// ---------------------------------------------------------------- the status bar's chip
const chip = () => serving() && m.client && st.doc
  ? `<button class="mcpchip sb" data-act="mcp-panel" data-tip="Claude Code is connected over MCP: edits may come from it. Opens its requests" data-tip-at="above"><i></i>MCP · Claude Code</button>` : '';

// ---------------------------------------------------------------- actions
function act(k, v) {
  switch (k) {
    case 'mcp-panel': m.open = !m.open; return true;
    case 'mcp-run':
      // A port in use makes Start fail, said in the panel and the Port row.
      if (v === 'start') {
        st.mcp = true;
        if (m.busy) { m.startErr = `Port ${st.mcpPort} is in use: choose another in the settings`; toast(m.startErr); }
        else { m.startErr = ''; toast(`Serving on 127.0.0.1:${st.mcpPort}`); }
      } else { st.mcp = false; m.client = false; m.open = true; toast('MCP server stopped'); }
      return true;
    case 'mcp-revert': {
      m.ctx = null;
      const [i, at] = v.split(':');
      if (after(+i, at).length) m.revertAsk = { i: +i, at }; else revert(+i, at);
      return true;
    }
    case 'mcp-revert-ok': revert(m.revertAsk.i, m.revertAsk.at); return true;
    case 'mcp-revert-no': m.revertAsk = null; return true;
    case 'mcp-show': {
      m.ctx = null;
      const c = m.calls[+v];
      st.settings = false; st.panel = 'timeline'; st.sel = { kind: 'feature', id: c.feat };
      return true;
    }
    case 'mcp-ask': {
      const q = m.ask; m.ask = null;
      if (v === 'close') return true;
      if (v === 'apply') {
        m.calls.push({ tool: 'edit_sketch', args: q.args, state: 'applied', result: `Applied after you applied yours and closed ${q.name}`, feat: q.feat, made: `${q.name} edited` });
        if (st.mode === 'sketch') toast(`Your changes applied; ${q.name} closed for Claude Code's edit`);
      } else m.calls.push({ tool: 'edit_sketch', args: q.args, state: 'failed', result: `Refused: you're editing ${q.name}` });
      return true;
    }
    case 'mcp-disconnect': m.client = false; toast('Claude Code disconnected'); return true;
    case 'mcp-token': m.tokenShown = !m.tokenShown; return true;
    case 'mcp-regen': m.client = false; toast('New token: Claude Code dropped, give it the new one'); return true;
  }
  return false;
}
// A call's context menu, where it was right-clicked; any other click shuts it.
document.addEventListener('contextmenu', e => {
  const row = e.target.closest?.('.tcall[data-i]');
  if (!row) return;
  e.preventDefault();
  const at = row.getBoundingClientRect();
  m.ctx = actions(m.calls[+row.dataset.i], +row.dataset.i).length ? { i: +row.dataset.i, x: e.clientX - at.left, y: e.clientY - at.top } : null;
  render();
});
document.addEventListener('click', e => { if (m.ctx && !e.target.closest?.('.tctx')) { m.ctx = null; render(); } }, true);
// The port keeps what's typed; it's checked as the field is left.
document.addEventListener('input', e => { if (e.target.dataset?.field === 'mcp-port') { st.mcpPort = e.target.value.trim(); m.busy = false; } });
document.addEventListener('change', e => { if (e.target.dataset?.field === 'mcp-port') render(); });

// /mcp: serving, Claude Code connected, its session below; /mcp-panel:
// with the panel open; /mcp-stopped: the panel open, the server stopped;
// /mcp-revert: the confirm open for Revert to before the extrude;
// /mcp-reverted: that done, and the client's next call told of it;
// /mcp-ctx: the extrude's context menu open; /mcp-ask: the client
// wants to change the sketch the user is editing, which has changes;
// /mcp-busy: switched on, the port in use; /tab-mcp: the settings open on
// the MCP tab.
// With no MCP flag the bracket opens on the session with the panel open,
// so the mock shows something; /nomcp starts as the app does, the server off.
const MCP_FLAGS = ['mcp', 'mcp-panel', 'mcp-ctx', 'mcp-ask', 'mcp-ask-cancelled', 'mcp-ask-refused', 'mcp-stopped', 'mcp-revert', 'mcp-reverted', 'mcp-busy', 'tab-mcp', 'nomcp'];
function start() {
  if (!MCP_FLAGS.some(flag) && st.doc?.kind === 'bracket' && !flag('web')) HASH.push('mcp', 'mcp-panel');
  // The flags that show the session imply it: /mcp-reverted alone has one.
    else if (['mcp-panel', 'mcp-ctx', 'mcp-ask', 'mcp-ask-cancelled', 'mcp-ask-refused', 'mcp-stopped', 'mcp-revert', 'mcp-reverted', 'tab-mcp'].some(flag) && !flag('nomcp') && !flag('mcp') && st.doc?.kind === 'bracket') HASH.push('mcp');
  if (flag('tab-mcp')) st.settings = true;
  if (flag('mcp')) { st.mcp = true; m.client = true; session(); }
  if (flag('mcp-busy')) { st.mcp = true; m.busy = true; m.startErr = `Port ${st.mcpPort} is in use: choose another in the settings`; }
  if (flag('mcp-panel') || flag('mcp-stopped') || flag('mcp-ctx') || flag('mcp-ask') || flag('mcp-ask-cancelled') || flag('mcp-ask-refused')) m.open = true;
  // The sketch being edited, or on the model page Sketch 4, as if open.
    if (flag('mcp-ask') || flag('mcp-ask-cancelled') || flag('mcp-ask-refused')) {
    const t = st.mode === 'sketch' && st.doc.timeline.find(t => t.id === st.sketch.id) || { id: 's4', name: 'Sketch 4' };
    m.ask = { name: t.name, feat: t.id, args: `${t.name}: the slot 2 mm wider` };
  }
    // /mcp-ask-refused: Apply yours tried, the open panel's OK refused.
  if (flag('mcp-ask-refused') && m.ask) m.ask.refused = 'Your Extrude 2 wasn’t applied: its preview fails (the profile is open). Fix it, or keep editing.';
  // /mcp-ask-cancelled: the same, its 30 s past: the call returned refused.
  if (flag('mcp-ask-cancelled') && m.ask) {
    m.ask.cancelled = true;
    m.calls.push({ tool: 'edit_sketch', args: m.ask.args, state: 'failed', result: `Not applied: the user was editing ${m.ask.name}` });
  }
  if (flag('mcp-stopped')) { st.mcp = false; m.client = false; }
  const ex = m.calls.findIndex(c => c.feat === 'e2' && c.tool === 'extrude');
  if ((flag('mcp-revert') || flag('mcp-reverted')) && m.calls[ex]?.snaps) { m.open = true; m.revertAsk = { i: ex, at: 'before' }; }
  if (flag('mcp-ctx') && ex >= 0) m.ctx = { i: ex, x: 200, y: 22 };
  if (flag('mcp-reverted') && m.revertAsk) {
    revert(ex, 'before');
    m.calls.push({ tool: 'design_summary', args: '', state: 'done', told: 'you reverted to before Extrude 2', result: '3 bodies · 11 features · 1 failed' });
    m.told = false;
  }
}
// Claude Code's session on the bracket: a look at the design, a feature,
// a sketch, a body's faces, a measure and a picture; then edits, each
// result naming what it made: a parameter, a new sketch drawn in, an
// extrude, a chamfer and a revolve applied, a setting changed, a body
// hidden, a fillet applied that fails (it stays, as the user's would, and
// the client is told why), a delete refused as others depend on it, and a
// body and a feature deleted. Each edit says what undoing it would look
// like, for the revert's confirm, which lists differences.
// After the extrude the user renamed Hole 2 (a change of theirs a revert
// to before the extrude would undo too).
function session() {
  const q = (tool, args, result) => m.calls.push({ tool, args, state: 'done', result });
  q('design_summary', '', '3 bodies · 10 features · 1 failed');
  q('feature', 'Fillet 2', 'R6 · 4 edges · fails: R6 is more than the 4 mm face allows');
  q('sketch', 'Sketch 4', '1 closed profile · fully constrained');
  q('body_topology', 'Body 1', '14 faces · 36 edges · 24 corners');
  q('measure', 'Body 1 top face to bottom face', '50 mm');
  q('render', 'Body 1, iso, 768 px', 'A 768 × 540 picture');
  if (!st.doc?.timeline.length || st.web) return;
  // An edit, applied: what it adds to the Timeline, if anything.
  const edit = (c, feat) => {
    const before = snap();
    if (feat) st.doc.timeline.push(feat);
    c.change?.(); delete c.change;
    m.calls.push({ state: 'applied', ...c, feat: feat?.id || c.feat, made: feat?.name || c.made, snaps: { before, after: snap() } });
  };
  edit({ tool: 'set_parameter', args: 'thickness = 6 mm', result: 'Applied · thickness 5 → 6 mm · Regenerated', made: 'thickness = 6 mm', key: 'p:thickness', undo: 'thickness back to 5 mm' });
  edit({ tool: 'add_sketch', args: 'on Body 1 top face', result: 'Applied · made Sketch 5 (id 41)', undo: 'Sketch 5 gone' },
    { id: 's5', type: 'sketch', name: 'Sketch 5', meta: 'Face' });
  edit({ tool: 'edit_sketch', args: 'Sketch 5: a circle Ø10, centred, its size 10 mm; a line through its centre', result: 'Applied · circle → curve 3, axis → line 4 · 1 closed profile, fully constrained', feat: 's5', made: 'Sketch 5 drawn', key: 'f:s5', undo: 'Sketch 5 empty again' });
  edit({ tool: 'extrude', args: 'Sketch 4, 12 mm, as a new body', result: 'Applied · made Extrude 2 (id 43) and Body 4 · Regenerated', undo: 'Extrude 2 and Body 4 gone' },
    { id: 'e2', type: 'extrude', name: 'Extrude 2', meta: '12 mm', info: 'One side · 12 mm · New body' });
  st.doc.timeline.find(t => t.id === 'h2').name = 'Bolt hole';
  m.yours.push({ at: m.calls.length, said: 'Renamed Hole 2 to Bolt hole', undo: 'Bolt hole named Hole 2 again' });
  edit({ tool: 'chamfer', args: 'Extrude 2 outer edges, 1 mm', told: 'you renamed Hole 2 to Bolt hole', result: 'Applied · made Chamfer 1 (id 44) · Regenerated', undo: 'Chamfer 1 gone' },
    { id: 'c1', type: 'chamfer', name: 'Chamfer 1', meta: '1 mm', info: '4 edges · 1 mm' });
  edit({ tool: 'revolve', args: 'Sketch 5 profile 1 about line 4, 360 deg, join', result: 'Applied · made Revolve 2 (id 45) · Regenerated', undo: 'Revolve 2 gone' },
    { id: 'r2', type: 'revolve', name: 'Revolve 2', meta: '360°', info: 'Full · Join' });
  edit({ tool: 'set_feature', args: 'Extrude 2 distance 14 mm', result: 'Applied · Extrude 2 distance 12 → 14 mm · Regenerated', feat: 'e2', made: 'Extrude 2 to 14 mm', key: 'f:e2', undo: 'Extrude 2 distance back to 12 mm' });
  edit({ tool: 'set_visible', args: 'Hide Body 3', result: 'Applied · Body 3 hidden', made: 'Body 3 hidden', key: 'b:3', undo: 'Body 3 shown' });
  edit({ tool: 'fillet', args: 'Extrude 2 top edges, R8', state: 'broke', result: 'Applied · made Fillet 3 (id 46) · Regenerated: Fillet 3 fails (R8 is more than the 6 mm face allows)', undo: 'Fillet 3 gone' },
    { id: 'f3', type: 'fillet', name: 'Fillet 3', meta: 'R8', info: '2 edges · R8 mm', failed: 'R8 is more than the 6 mm face allows' });
  m.calls.push({ tool: 'remove_feature', args: 'Sketch 1', state: 'failed', result: 'Refused: Extrude 1 is made from it; delete that first to delete it' });
  edit({ tool: 'remove_body', args: 'Body 3', result: 'Applied · Body 3 deleted · Regenerated', made: 'Body 3 deleted', key: 'b:3', undo: 'Body 3 back' });
  edit({ tool: 'remove_feature', args: 'Fillet 3', result: 'Applied · Fillet 3 deleted · Regenerated', made: 'Fillet 3 deleted', undo: 'Fillet 3 back', change: () => { st.doc.timeline = st.doc.timeline.filter(t => t.id !== 'f3'); } });
}

window.mcp = { settingsTab, chip, act, start, button, panel };
})();
