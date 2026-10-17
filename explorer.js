// The function map: loads data/symbols.json (tools/symbols_export.py) and lets the reader search, filter and follow
// calls. A link to symbols.html#Name or #module-address opens that function.
"use strict";

// what each module is (from the overlay's own function names and the game-flow notes; "?" where not yet confirmed)
const MODULES = {
  static: "Core code (always loaded)",
  ov00: "Start-up initialisation", ov01: "Protection layer", ov02: "Scenes: dates, skinship",
  ov03: "Schedule input", ov04: "Map (other areas)", ov05: "Speech recognition", ov06: "Profile (handwriting)",
  ov07: "Gallery options", ov08: "Calendar", ov09: "Message screen", ov10: "Download Play (multiboot)",
  ov11: "Message dialog", ov12: "Card exchange", ov13: "Title screen", ov14: "Puzzle", ov15: "Card exchange",
  ov16: "Card exchange", ov17: "Card exchange", ov18: "Card exchange", ov19: "Purpose", ov20: "Time setting",
  ov21: "Title menu", ov22: "Select mode", ov23: "Event scene", ov24: "? (overlay 24)", ov25: "? (overlay 25)",
  ov26: "Wireless (ad hoc)", ov27: "File select", ov28: "File delete", ov29: "Gallery", ov30: "Gallery",
  ov31: "Gallery", ov32: "Gallery", ov33: "Gallery", ov34: "Schedule", ov35: "The room", ov36: "Web",
  ov37: "Dream", ov38: "LovePlus menu", ov39: "Menu (aim)", ov40: "Menu (aim)", ov41: "Overnight",
  ov42: "Time setting", ov43: "Game-time reconciliation",
};
const KINDS = [
  ["game", "the game's own code"], ["sdk", "Nintendo SDK"], ["nitrosystem", "NitroSystem"], ["library", "library internals"],
];
const CONFS = ["exact", "verified", "high", "medium", "low"];
const WHY = {
  exact: "Matched byte for byte against the library it comes from.",
  verified: "Checked against the running game (the ported game flow, 311,719 frames).",
  high: "Well supported by what the function calls, reads and writes.",
  medium: "A reasonable reading; details may be off.",
  low: "Marks the function's area; doesn't claim to know what it does.",
};
const SHOW = 300;

const $ = (id) => document.getElementById(id);
const hex = (a) => a.toString(16).padStart(8, "0");
const esc = (t) => String(t).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

let S = [], CALLS = [], CALLERS = [], byName = new Map(), byKey = new Map();
const state = { q: "", mod: "", kinds: new Set(KINDS.map((k) => k[0])), confs: new Set(CONFS), shown: SHOW, sel: -1 };

function key(i) { return S[i][0] + "-" + hex(S[i][1]); }

function load() {
  fetch("data/symbols.json").then((r) => {
    if (!r.ok) throw new Error(r.status + " " + r.statusText);
    return r.json();
  }).then((d) => {
    S = d.symbols; CALLS = d.calls;
    CALLERS = S.map(() => []);
    CALLS.forEach((cs, i) => cs.forEach((j) => CALLERS[j].push(i)));
    S.forEach((s, i) => {
      if (!byName.has(s[3])) byName.set(s[3], i);
      byKey.set(key(i), i);
    });
    setup();
    fromHash();
    render();
  }).catch((e) => {
    $("count").textContent = "The map could not be loaded (" + e.message + "). Reload the page to try again.";
  });
}

function counts(field, filter) {
  const c = {};
  for (const s of S) if (filter(s)) c[s[field]] = (c[s[field]] || 0) + 1;
  return c;
}

function setup() {
  const mods = [...new Set(S.map((s) => s[0]))];
  const sel = $("mod");
  for (const m of mods) {
    const o = document.createElement("option");
    o.value = m; o.textContent = m + " · " + (MODULES[m] || "");
    sel.appendChild(o);
  }
  sel.addEventListener("change", () => { state.mod = sel.value; state.shown = SHOW; render(); });
  const kc = counts(4, () => true), cc = counts(5, () => true);
  for (const [k, label] of KINDS) $("kinds").appendChild(chip(k, label, kc[k] || 0, state.kinds));
  for (const c of CONFS) {
    const b = chip(c, c, cc[c] || 0, state.confs);
    b.insertBefore(Object.assign(document.createElement("i"), { className: "dot c-" + c }), b.firstChild);
    $("confs").appendChild(b);
  }
  let t;
  $("q").addEventListener("input", () => { clearTimeout(t); t = setTimeout(() => { state.q = $("q").value.trim(); state.shown = SHOW; render(); }, 120); });
  // the module overview
  const box = $("modules");
  for (const m of mods) {
    const list = S.filter((s) => s[0] === m);
    const c = {}; for (const s of list) c[s[5]] = (c[s[5]] || 0) + 1;
    const b = document.createElement("button");
    b.className = "mod"; b.type = "button";
    const bar = CONFS.map((k) => c[k] ? `<i class="c-${k}" style="width:${(100 * c[k] / list.length).toFixed(2)}%;border-radius:0;opacity:${k === "low" ? .55 : 1}"></i>` : "").join("");
    b.innerHTML = `<span class="top"><span class="id">${esc(m)}</span><span class="cnt">${list.length.toLocaleString()}</span></span><span class="lab">${esc(MODULES[m] || m)}</span><span class="confbar">${bar}</span>`;
    b.addEventListener("click", () => { state.mod = m; $("mod").value = m; state.shown = SHOW; render(); });
    box.appendChild(b);
  }
  window.addEventListener("hashchange", () => { fromHash(); render(); });
}

function chip(value, label, n, set) {
  const b = document.createElement("button");
  b.type = "button"; b.className = "chip"; b.setAttribute("aria-pressed", "true");
  b.innerHTML = `${esc(label)} <span class="n">${n.toLocaleString()}</span>`;
  b.addEventListener("click", () => {
    if (set.has(value)) set.delete(value); else set.add(value);
    b.setAttribute("aria-pressed", set.has(value) ? "true" : "false");
    state.shown = SHOW; render();
  });
  return b;
}

function matches() {
  const q = state.q.toLowerCase();
  let addr = null;
  const m = q.match(/^(?:0x)?([0-9a-f]{6,8})$/);
  if (m) addr = parseInt(m[1], 16);
  const out = [];
  for (let i = 0; i < S.length; i++) {
    const s = S[i];
    if (state.mod && s[0] !== state.mod) continue;
    if (!state.kinds.has(s[4]) || !state.confs.has(s[5])) continue;
    if (q) {
      // an address finds the function that contains it
      const hit = addr !== null ? (s[1] <= addr && addr < s[1] + Math.max(s[2] || 1, 1)) : (s[3].toLowerCase().includes(q) || s[6].toLowerCase().includes(q));
      if (!hit) continue;
    }
    out.push(i);
  }
  // names that start with the query first
  if (q && addr === null) out.sort((a, b) => (S[a][3].toLowerCase().startsWith(q) ? 0 : 1) - (S[b][3].toLowerCase().startsWith(q) ? 0 : 1));
  return out;
}

function render() {
  const list = matches();
  $("count").textContent = list.length === S.length ? `${S.length.toLocaleString()} functions` : `${list.length.toLocaleString()} of ${S.length.toLocaleString()} functions`;
  $("sortnote").textContent = state.q && !/^(0x)?[0-9a-f]{6,8}$/i.test(state.q) ? "best matches first" : "by address";
  const rows = $("rows");
  if (!list.length) {
    rows.innerHTML = `<p class="empty">No functions match. Try a shorter search, or turn some kinds or confidence levels back on.</p>`;
  } else {
    rows.innerHTML = list.slice(0, state.shown).map((i) => {
      const s = S[i];
      return `<button type="button" class="r" data-i="${i}"${i === state.sel ? ' aria-current="true"' : ""}><i class="dot c-${s[5]}" title="${s[5]}"></i><span class="nm">${esc(s[3])}</span><span class="ad">${s[0]} ${hex(s[1])}</span>${s[6] ? `<span class="nt">${esc(s[6])}</span>` : ""}</button>`;
    }).join("") + (list.length > state.shown ? `<button type="button" class="more" id="more">Show ${Math.min(SHOW, list.length - state.shown).toLocaleString()} more</button>` : "");
    const more = $("more");
    if (more) more.addEventListener("click", () => { state.shown += SHOW; render(); });
  }
  rows.querySelectorAll(".r").forEach((b) => b.addEventListener("click", () => select(+b.dataset.i, true)));
  renderDetail();
}

function link(i) {
  const s = S[i];
  return `<li><a href="#${key(i)}" title="${esc(s[0] + " " + hex(s[1]) + (s[6] ? " · " + s[6] : ""))}"><i class="dot c-${s[5]}"></i><span class="t">${esc(s[3])}</span></a></li>`;
}

function renderDetail() {
  const i = state.sel;
  $("overview").hidden = i >= 0;
  $("fn").hidden = i < 0;
  if (i < 0) return;
  const s = S[i];
  const calls = CALLS[i] || [], callers = CALLERS[i] || [];
  const kind = (KINDS.find((k) => k[0] === s[4]) || [s[4], s[4]])[1];
  $("fn").innerHTML = `
    <div style="display:grid;gap:6px">
      <h2>${esc(s[3])}</h2>
      <span class="badge"><i class="dot c-${s[5]}"></i>${esc(s[5])} · ${esc(kind)}</span>
      <p class="why">${esc(WHY[s[5]] || "")}</p>
    </div>
    <dl class="facts">
      <dt>module</dt><dd>${esc(s[0])} · ${esc(MODULES[s[0]] || "")}</dd>
      <dt>address</dt><dd>0x${hex(s[1])}${s[2] ? " – 0x" + hex(s[1] + s[2] - 1) : ""}</dd>
      <dt>size</dt><dd>${s[2] ? s[2].toLocaleString() + " bytes" : "not a function (a marker)"}</dd>
    </dl>
    ${s[6] ? `<p class="notetext">${esc(s[6])}</p>` : ""}
    <div class="edges">
      <div><h3>Calls (${calls.length})</h3>${calls.length ? `<ul>${calls.map(link).join("")}</ul>` : `<p class="none">No direct calls.</p>`}</div>
      <div><h3>Called by (${callers.length})</h3>${callers.length ? `<ul>${callers.map(link).join("")}</ul>` : `<p class="none">No direct callers found (it may be reached through a pointer).</p>`}</div>
    </div>
    <p class="why"><a href="#" id="back">← All modules</a></p>`;
  $("back").addEventListener("click", (e) => { e.preventDefault(); history.replaceState(null, "", location.pathname); state.sel = -1; render(); });
}

function select(i, push) {
  state.sel = i;
  if (push) history.replaceState(null, "", "#" + key(i));
  render();
  if (window.matchMedia("(max-width: 860px)").matches) $("detail").scrollIntoView({ block: "start" });
}

function fromHash() {
  const h = decodeURIComponent(location.hash.slice(1));
  if (!h) { state.sel = -1; return; }
  let i = byKey.get(h);
  if (i === undefined) i = byName.get(h);
  if (i === undefined) { state.sel = -1; return; }
  state.sel = i;
}

load();
