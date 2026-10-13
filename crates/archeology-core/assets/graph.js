// Grafo del conocimiento (estilo Obsidian) de archeology-doc.
//
// Copia canónica y única del renderizador: la embebe el CLI (`adoc graph
// --format html`, `adoc report`, `adoc site sync`) y la extensión de VS Code
// la copia a `extension/media/graph.js` al compilar. No depende de ningún
// entorno (ni VS Code ni VitePress): recibe un contenedor, los textos y un
// callback para abrir nodos.
//
// Sin dependencias: simulación de fuerzas + canvas. La simulación (`Layout`)
// no toca el DOM y se puede probar con Node (`require('./graph.js').Layout`).
//
// Uso en el navegador:
//   const g = AdocGraph.mount(contenedor, {
//     t: { search: 'Buscar…', ... } | (clave, params) => texto,
//     onOpen(node) { ...; return false para mostrar la ficha del nodo },
//     onRefresh() { ... },              // opcional: botón «Actualizar»
//     hiddenKinds: ['moc', 'code'],     // tipos ocultos al empezar
//   });
//   g.setData({ nodes, edges });
//
// Los textos del grafo (títulos de notas, rutas) vienen de agentes o personas:
// solo se insertan con `textContent`, nunca como HTML.

(function (root) {
  'use strict';

  // ── Simulación de fuerzas ───────────────────────────────────────────────

  /** Disposición por fuerzas: repulsión entre nodos, resortes en aristas, gravedad al centro. */
  class Layout {
    constructor(nodes, edges) {
      this.nodes = nodes;
      const index = new Map(nodes.map((n, i) => [n.id, i]));
      this.links = edges
        .map((e) => ({ s: index.get(e.source), t: index.get(e.target), kind: e.kind }))
        .filter((l) => l.s !== undefined && l.t !== undefined);
      // Posición inicial en espiral: estable y sin solapes.
      nodes.forEach((n, i) => {
        if (n.x === undefined) {
          const a = i * 2.399963;
          const r = 12 * Math.sqrt(i + 1);
          n.x = Math.cos(a) * r;
          n.y = Math.sin(a) * r;
        }
        n.vx = 0;
        n.vy = 0;
      });
      this.alpha = 1;
    }

    /** Un paso de simulación; devuelve `false` cuando ya está estable. */
    step() {
      const nodes = this.nodes;
      const n = nodes.length;
      if (this.alpha < 0.005 || n === 0) {
        return false;
      }
      const repulsion = 900;
      // Repulsión O(n²) con corte de distancia: suficiente para vaults de cientos de notas.
      for (let i = 0; i < n; i++) {
        const a = nodes[i];
        if (a.hidden) continue;
        for (let j = i + 1; j < n; j++) {
          const b = nodes[j];
          if (b.hidden) continue;
          let dx = a.x - b.x;
          let dy = a.y - b.y;
          let d2 = dx * dx + dy * dy;
          if (d2 > 160000) continue;
          if (d2 < 0.01) {
            dx = Math.random() - 0.5;
            dy = Math.random() - 0.5;
            d2 = 0.5;
          }
          const f = (repulsion * this.alpha) / d2;
          a.vx += dx * f;
          a.vy += dy * f;
          b.vx -= dx * f;
          b.vy -= dy * f;
        }
      }
      for (const l of this.links) {
        const a = nodes[l.s];
        const b = nodes[l.t];
        if (a.hidden || b.hidden) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 1;
        const rest = l.kind === 'evidence' ? 55 : 80;
        const f = ((d - rest) / d) * 0.06 * this.alpha;
        a.vx += dx * f;
        a.vy += dy * f;
        b.vx -= dx * f;
        b.vy -= dy * f;
      }
      for (const p of nodes) {
        if (p.hidden) continue;
        // Gravedad al centro: mantiene compactas las notas sin conexiones.
        p.vx -= p.x * 0.015 * this.alpha;
        p.vy -= p.y * 0.015 * this.alpha;
        if (p.fixed) {
          p.vx = 0;
          p.vy = 0;
          continue;
        }
        p.vx *= 0.6;
        p.vy *= 0.6;
        p.x += p.vx;
        p.y += p.vy;
      }
      this.alpha *= 0.985;
      return true;
    }

    reheat(value = 0.6) {
      this.alpha = Math.max(this.alpha, value);
    }
  }

  // ── Colores ─────────────────────────────────────────────────────────────

  const KIND_COLORS = {
    overview: '#a78bfa',
    module: '#60a5fa',
    feature: '#34d399',
    finding: '#f87171',
    'test-report': '#fbbf24',
    'spec-report': '#2dd4bf',
    glossary: '#c084fc',
    decision: '#f472b6',
    memory: '#94a3b8',
    faq: '#38bdf8',
    moc: '#64748b',
    code: '#9ca3af',
  };
  const SEVERITY_COLORS = { critical: '#dc2626', high: '#ef4444', medium: '#f59e0b', low: '#3b82f6' };

  function colorOf(n) {
    if (n.kind === 'finding' && n.severity && SEVERITY_COLORS[n.severity]) {
      return SEVERITY_COLORS[n.severity];
    }
    return KIND_COLORS[n.kind] || '#9ca3af';
  }

  /** Sustituye `{nombre}` por `params[nombre]` (una sola pasada). */
  function interpolate(text, params) {
    return String(text).replace(/\{(\w+)\}/g, (m, name) =>
      params && params[name] !== undefined ? String(params[name]) : m,
    );
  }

  // ── Vista (solo en el navegador) ────────────────────────────────────────

  /** Crea un elemento con clase y, opcionalmente, texto (siempre como texto). */
  function el(tag, className, text) {
    const e = document.createElement(tag);
    if (className) e.className = className;
    if (text !== undefined) e.textContent = text;
    return e;
  }

  /**
   * Monta el grafo dentro de `container` y devuelve su controlador.
   * `options.t`: diccionario `{clave: texto}` o función `(clave, params) => texto`.
   */
  function mount(container, options) {
    const opts = options || {};
    const strings = opts.t || {};
    /** Texto traducido; sin traducción, la propia clave. */
    function text(key, params) {
      if (typeof strings === 'function') {
        const v = strings(key, params);
        return v === undefined || v === null ? key : String(v);
      }
      const v = strings[key];
      return v === undefined ? key : interpolate(v, params);
    }
    /** Etiqueta de un tipo de nota; sin traducción, el identificador del tipo. */
    function kindLabel(kind) {
      const label = text(`kind.${kind}`);
      return label === `kind.${kind}` ? kind : label;
    }

    // ── DOM ──
    container.classList.add('adoc-graph');
    container.textContent = '';
    const canvas = el('canvas', 'adoc-graph-canvas');
    canvas.setAttribute('role', 'img');
    const ctx = canvas.getContext('2d');
    const bar = el('div', 'adoc-graph-bar');
    const search = el('input', 'adoc-graph-search');
    search.type = 'search';
    search.placeholder = text('search');
    search.setAttribute('aria-label', text('search'));
    const zoomIn = el('button', 'adoc-graph-btn', '+');
    zoomIn.type = 'button';
    zoomIn.title = text('zoom_in');
    const zoomOut = el('button', 'adoc-graph-btn', '−');
    zoomOut.type = 'button';
    zoomOut.title = text('zoom_out');
    const fitBtn = el('button', 'adoc-graph-btn', text('fit'));
    fitBtn.type = 'button';
    bar.append(search, zoomIn, zoomOut, fitBtn);
    if (typeof opts.onRefresh === 'function') {
      const refresh = el('button', 'adoc-graph-btn', text('refresh'));
      refresh.type = 'button';
      refresh.addEventListener('click', () => opts.onRefresh());
      bar.append(refresh);
    }
    const stats = el('span', 'adoc-graph-stats');
    bar.append(stats);
    // Leyenda: color por tipo, con casilla para mostrar u ocultar cada tipo.
    const legend = el('div', 'adoc-graph-legend');
    const tip = el('div', 'adoc-graph-tip');
    tip.hidden = true;
    const details = el('div', 'adoc-graph-details');
    details.hidden = true;
    const empty = el('div', 'adoc-graph-empty', text('empty'));
    empty.hidden = true;
    container.append(canvas, bar, legend, tip, details, empty);

    let graph = { nodes: [], edges: [] };
    let nodeMap = new Map();
    let layout = null;
    let neighbors = new Map();
    let degree = new Map();
    const view = { x: 0, y: 0, k: 1 };
    let hover = null;
    let drag = null;
    let pan = null;
    let query = '';
    let selected = null;
    const hiddenKinds = new Set(opts.hiddenKinds || ['moc', 'code']);
    let running = false;
    let userMoved = false;
    let fitted = false;
    let destroyed = false;

    /** Colores y fuente del tema (variables CSS del contenedor; cambian con el tema). */
    function theme() {
      const css = getComputedStyle(container);
      const v = (name, fallback) => css.getPropertyValue(name).trim() || fallback;
      return {
        fg: v('--adoc-graph-fg', '#ccc'),
        muted: v('--adoc-graph-muted', '#888'),
        bg: v('--adoc-graph-bg', '#1e1e1e'),
        // El canvas no resuelve variables CSS: se usa la fuente calculada.
        family: css.fontFamily || 'sans-serif',
      };
    }

    function resize() {
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      draw();
    }

    function setData(data) {
      // Conserva posiciones al refrescar (el agente añade notas mientras miras).
      const old = new Map((layout ? layout.nodes : []).map((n) => [n.id, n]));
      graph = { nodes: (data && data.nodes) || [], edges: (data && data.edges) || [] };
      nodeMap = new Map(graph.nodes.map((n) => [n.id, n]));
      for (const n of graph.nodes) {
        const prev = old.get(n.id);
        if (prev) {
          n.x = prev.x;
          n.y = prev.y;
        }
      }
      neighbors = new Map(graph.nodes.map((n) => [n.id, new Set()]));
      degree = new Map(graph.nodes.map((n) => [n.id, 0]));
      for (const e of graph.edges) {
        neighbors.get(e.source)?.add(e.target);
        neighbors.get(e.target)?.add(e.source);
        degree.set(e.source, (degree.get(e.source) || 0) + 1);
        degree.set(e.target, (degree.get(e.target) || 0) + 1);
      }
      if (selected && !nodeMap.has(selected.id)) hideDetails();
      layout = new Layout(graph.nodes, graph.edges);
      if (old.size) layout.alpha = 0.3;
      empty.hidden = graph.nodes.length > 0;
      buildLegend();
      applyFilters();
      // Pre-cálculo: el grafo aparece casi asentado (≤120 ms, también en vaults grandes).
      const now = () => (typeof performance !== 'undefined' ? performance.now() : Date.now());
      const t0 = now();
      while (now() - t0 < 120 && layout.step()) {
        /* simulando */
      }
      if (!userMoved) fit();
      animate();
    }

    function buildLegend() {
      const counts = {};
      for (const n of graph.nodes) counts[n.kind] = (counts[n.kind] || 0) + 1;
      legend.textContent = '';
      const kinds = Object.keys(counts).sort((a, b) => (a === 'code') - (b === 'code') || a.localeCompare(b));
      legend.hidden = kinds.length === 0;
      if (!kinds.length) return;
      legend.append(el('div', 'adoc-graph-legend-title', text('legend')));
      for (const kind of kinds) {
        const label = el('label', 'adoc-graph-legend-item');
        const box = el('input');
        box.type = 'checkbox';
        box.checked = !hiddenKinds.has(kind);
        box.addEventListener('change', () => {
          if (box.checked) hiddenKinds.delete(kind);
          else hiddenKinds.add(kind);
          applyFilters();
        });
        const dot = el('span', 'adoc-graph-dot');
        dot.style.background = KIND_COLORS[kind] || '#9ca3af';
        label.append(box, dot, document.createTextNode(`${kindLabel(kind)} (${counts[kind]})`));
        legend.append(label);
      }
      // Estilo de las aristas: relaciones (continuas) y evidencias (punteadas).
      const edgeKinds = new Set(graph.edges.map((e) => (e.kind === 'evidence' ? 'evidence' : 'relation')));
      for (const kind of ['relation', 'evidence']) {
        if (!edgeKinds.has(kind)) continue;
        const row = el('div', 'adoc-graph-legend-edge');
        row.append(el('span', `adoc-graph-line adoc-graph-line-${kind}`), document.createTextNode(text(`edge_${kind}`)));
        legend.append(row);
      }
    }

    function applyFilters() {
      for (const n of graph.nodes) n.hidden = hiddenKinds.has(n.kind);
      const visible = graph.nodes.filter((n) => !n.hidden).length;
      const edges = graph.edges.filter((e) => !isHidden(e.source) && !isHidden(e.target)).length;
      stats.textContent = text('stats', { nodes: visible, edges });
      if (selected && selected.hidden) hideDetails();
      if (layout) layout.reheat(0.5);
      animate();
    }

    function isHidden(id) {
      const n = nodeMap.get(id);
      return !n || n.hidden;
    }

    function radius(n) {
      return 3.5 + Math.min(9, Math.sqrt(degree.get(n.id) || 0) * 1.6) + (n.kind === 'overview' ? 3 : 0);
    }

    function toScreen(p) {
      return { x: (p.x + view.x) * view.k + canvas.clientWidth / 2, y: (p.y + view.y) * view.k + canvas.clientHeight / 2 };
    }
    function toWorld(x, y) {
      return { x: (x - canvas.clientWidth / 2) / view.k - view.x, y: (y - canvas.clientHeight / 2) / view.k - view.y };
    }

    function nodeAt(x, y) {
      const w = toWorld(x, y);
      let best = null;
      let bestD = Infinity;
      for (const n of graph.nodes) {
        if (n.hidden) continue;
        const d = Math.hypot(n.x - w.x, n.y - w.y);
        if (d < radius(n) + 4 / view.k && d < bestD) {
          best = n;
          bestD = d;
        }
      }
      return best;
    }

    function matches(n) {
      return !query || String(n.title).toLowerCase().includes(query) || String(n.id).toLowerCase().includes(query);
    }

    function draw() {
      if (destroyed) return;
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      const { fg, muted, bg, family } = theme();
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, w, h);
      if (!layout) return;
      const few = graph.nodes.filter((n) => !n.hidden).length <= 40;
      const focusNode = hover || selected;
      const focus = focusNode ? neighbors.get(focusNode.id) : null;

      ctx.lineWidth = 1;
      for (const e of graph.edges) {
        const a = nodeMap.get(e.source);
        const b = nodeMap.get(e.target);
        if (!a || !b || a.hidden || b.hidden) continue;
        const lit = focusNode && (a === focusNode || b === focusNode);
        ctx.strokeStyle = lit ? fg : muted;
        ctx.globalAlpha = focusNode ? (lit ? 0.9 : 0.06) : e.kind === 'evidence' ? 0.18 : 0.35;
        if (e.kind === 'evidence') ctx.setLineDash([3, 3]);
        const pa = toScreen(a);
        const pb = toScreen(b);
        ctx.beginPath();
        ctx.moveTo(pa.x, pa.y);
        ctx.lineTo(pb.x, pb.y);
        ctx.stroke();
        ctx.setLineDash([]);
      }

      for (const n of graph.nodes) {
        if (n.hidden) continue;
        const p = toScreen(n);
        const r = radius(n) * Math.max(0.6, Math.min(1.6, view.k));
        const dim = (focusNode && n !== focusNode && !(focus && focus.has(n.id))) || !matches(n);
        ctx.globalAlpha = dim ? 0.15 : 1;
        ctx.fillStyle = colorOf(n);
        ctx.beginPath();
        ctx.arc(p.x, p.y, r, 0, Math.PI * 2);
        ctx.fill();
        if (n.stale) {
          ctx.strokeStyle = '#f59e0b';
          ctx.lineWidth = 2;
          ctx.stroke();
          ctx.lineWidth = 1;
        }
        if (n === selected) {
          ctx.strokeStyle = fg;
          ctx.lineWidth = 2;
          ctx.beginPath();
          ctx.arc(p.x, p.y, r + 3, 0, Math.PI * 2);
          ctx.stroke();
          ctx.lineWidth = 1;
        }
        // Etiquetas: al acercar, al pasar el cursor, vecinos y nodos importantes.
        // Con pocos nodos visibles se rotulan todos; en grafos grandes, los más conectados.
        const important = few || (degree.get(n.id) || 0) >= 6 || n.kind === 'overview';
        const show =
          n === focusNode ||
          (focus && focus.has(n.id)) ||
          view.k > 1.3 ||
          (important && view.k > 0.6) ||
          (query && matches(n));
        if (show && !dim) {
          ctx.fillStyle = fg;
          ctx.font = `${n === focusNode ? 'bold ' : ''}11px ${family}`;
          ctx.textAlign = 'center';
          const title = String(n.title);
          const label = title.length > 42 ? title.slice(0, 40) + '…' : title;
          ctx.fillText(label, p.x, p.y + r + 12);
        }
      }
      ctx.globalAlpha = 1;
    }

    /** Encuadra los nodos visibles. */
    function fit() {
      const visible = graph.nodes.filter((n) => !n.hidden);
      // Oculto (pestaña inactiva): no hay tamaño con el que encuadrar.
      if (!visible.length || canvas.clientWidth === 0) return false;
      const xs = visible.map((n) => n.x);
      const ys = visible.map((n) => n.y);
      const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
      const w = Math.max(1, x1 - x0);
      const h = Math.max(1, y1 - y0);
      view.k = Math.max(0.15, Math.min(2, 0.85 * Math.min(canvas.clientWidth / w, canvas.clientHeight / h)));
      view.x = -(x0 + x1) / 2;
      view.y = -(y0 + y1) / 2;
      draw();
      return true;
    }

    /** Zoom manteniendo fijo el punto de pantalla (x, y). */
    function zoomAt(factor, x, y) {
      userMoved = true;
      const before = toWorld(x, y);
      view.k = Math.max(0.15, Math.min(4, view.k * factor));
      const after = toWorld(x, y);
      view.x += after.x - before.x;
      view.y += after.y - before.y;
      draw();
    }

    function centerOn(n) {
      userMoved = true;
      view.x = -n.x;
      view.y = -n.y;
      view.k = Math.max(view.k, 1.2);
      draw();
    }

    function animate() {
      if (running || destroyed) return;
      running = true;
      let frames = 0;
      const tick = () => {
        if (destroyed) return;
        // Varios pasos por fotograma: se asienta en ~1 s en vez de ~6 s.
        let moving = false;
        for (let i = 0; i < 4 && layout; i++) moving = layout.step();
        frames++;
        // Mientras se acomoda, seguir encuadrando hasta que el usuario mueva la vista.
        if (!userMoved && frames % 15 === 0) fit();
        draw();
        if (moving || drag) requestAnimationFrame(tick);
        else {
          running = false;
          if (!fitted && !userMoved) {
            fitted = fit();
          }
        }
      };
      requestAnimationFrame(tick);
    }

    // ── Ficha del nodo (destinos sin `onOpen` o cuando `onOpen` devuelve false) ──

    function showDetails(n) {
      selected = n;
      details.textContent = '';
      const close = el('button', 'adoc-graph-close', '×');
      close.type = 'button';
      close.title = text('close');
      close.setAttribute('aria-label', text('close'));
      close.addEventListener('click', hideDetails);
      details.append(close, el('div', 'adoc-graph-details-title', String(n.title)));
      const meta = [kindLabel(n.kind)];
      if (n.severity) meta.push(n.severity);
      if (n.state) meta.push(n.state);
      else if (n.status) meta.push(n.status);
      if (n.stale) meta.push(text('stale'));
      details.append(el('div', 'adoc-graph-details-meta', meta.join(' · ')));
      const path = n.path || n.file;
      if (path) {
        const row = el('div', 'adoc-graph-details-path');
        row.append(el('span', 'adoc-graph-details-label', `${text('path')}: `));
        row.append(el('code', '', n.line ? `${path}:${n.line}` : String(path)));
        details.append(row);
      }
      // Solo enlaces seguros: file:, http(s): o rutas relativas del sitio.
      if (n.href && /^(file:|https?:|\/)/i.test(String(n.href))) {
        const a = el('a', 'adoc-graph-details-link', text('open'));
        a.href = String(n.href);
        a.rel = 'noopener';
        details.append(a);
      }
      details.hidden = false;
      draw();
    }

    function hideDetails() {
      selected = null;
      details.hidden = true;
      draw();
    }

    function open(n) {
      const handled = typeof opts.onOpen === 'function' ? opts.onOpen(n) : false;
      if (handled === false) showDetails(n);
    }

    // ── Interacción (eventos de puntero: ratón, lápiz y táctil) ──

    function local(ev) {
      const r = canvas.getBoundingClientRect();
      return { x: ev.clientX - r.left, y: ev.clientY - r.top };
    }

    function onPointerMove(ev) {
      const { x, y } = local(ev);
      if (drag) {
        if (!drag.moved && Math.hypot(ev.clientX - drag.startX, ev.clientY - drag.startY) < 3) return;
        const w = toWorld(x, y);
        drag.node.x = w.x;
        drag.node.y = w.y;
        drag.moved = true;
        tip.hidden = true;
        layout.reheat(0.3);
        animate();
        return;
      }
      if (pan) {
        userMoved = true;
        view.x += (ev.clientX - pan.x) / view.k;
        view.y += (ev.clientY - pan.y) / view.k;
        pan = { x: ev.clientX, y: ev.clientY };
        draw();
        return;
      }
      const n = nodeAt(x, y);
      if (n !== hover) {
        hover = n;
        canvas.style.cursor = n ? 'pointer' : 'grab';
        draw();
      }
      if (n) {
        const extra = n.kind === 'finding' ? ` · ${n.severity || ''} ${n.state || ''}` : n.status ? ` · ${n.status}` : '';
        tip.textContent = `${n.title} (${kindLabel(n.kind)}${extra})${n.stale ? ` · ${text('stale')}` : ''}`;
        tip.style.left = `${x + 14}px`;
        tip.style.top = `${y + 14}px`;
        tip.hidden = false;
      } else {
        tip.hidden = true;
      }
    }

    function onPointerDown(ev) {
      if (ev.button !== undefined && ev.button !== 0) return;
      const { x, y } = local(ev);
      const n = nodeAt(x, y);
      try {
        canvas.setPointerCapture(ev.pointerId);
      } catch {
        /* sin captura (navegadores antiguos) */
      }
      if (n) {
        drag = { node: n, moved: false, startX: ev.clientX, startY: ev.clientY };
        n.fixed = true;
      } else {
        pan = { x: ev.clientX, y: ev.clientY };
        canvas.style.cursor = 'grabbing';
      }
    }

    function onPointerUp() {
      if (drag) {
        drag.node.fixed = false;
        const { node, moved } = drag;
        drag = null;
        if (!moved) open(node);
      }
      pan = null;
      canvas.style.cursor = hover ? 'pointer' : 'grab';
    }

    function onLeave() {
      hover = null;
      tip.hidden = true;
      draw();
    }

    function onWheel(ev) {
      ev.preventDefault();
      const { x, y } = local(ev);
      zoomAt(Math.exp(-ev.deltaY * 0.0015), x, y);
    }

    canvas.addEventListener('pointermove', onPointerMove);
    canvas.addEventListener('pointerdown', onPointerDown);
    canvas.addEventListener('pointerup', onPointerUp);
    canvas.addEventListener('pointercancel', onPointerUp);
    canvas.addEventListener('pointerleave', onLeave);
    canvas.addEventListener('wheel', onWheel, { passive: false });
    search.addEventListener('input', () => {
      query = search.value.trim().toLowerCase();
      draw();
    });
    search.addEventListener('keydown', (ev) => {
      // Enter: centrar la primera coincidencia visible.
      if (ev.key !== 'Enter' || !query) return;
      const hit = graph.nodes.find((n) => !n.hidden && matches(n));
      if (hit) centerOn(hit);
    });
    zoomIn.addEventListener('click', () => zoomAt(1.25, canvas.clientWidth / 2, canvas.clientHeight / 2));
    zoomOut.addEventListener('click', () => zoomAt(0.8, canvas.clientWidth / 2, canvas.clientHeight / 2));
    fitBtn.addEventListener('click', () => {
      userMoved = false;
      fit();
    });

    // Tamaño: el contenedor puede cambiar sin que cambie la ventana (pestañas, paneles).
    let observer = null;
    if (typeof ResizeObserver !== 'undefined') {
      observer = new ResizeObserver(() => resize());
      observer.observe(container);
    }
    window.addEventListener('resize', resize);
    // Cambio de tema (clase `dark` de VitePress, `vscode-dark` de VS Code o
    // preferencia del sistema): redibujar con los colores nuevos.
    let themeObserver = null;
    if (typeof MutationObserver !== 'undefined') {
      themeObserver = new MutationObserver(() => draw());
      for (const target of [document.documentElement, document.body]) {
        if (target) themeObserver.observe(target, { attributes: true, attributeFilter: ['class', 'data-theme', 'style'] });
      }
    }
    const darkQuery = window.matchMedia ? window.matchMedia('(prefers-color-scheme: dark)') : null;
    if (darkQuery && darkQuery.addEventListener) darkQuery.addEventListener('change', draw);
    resize();

    return {
      setData,
      resize() {
        resize();
        // Al mostrarse (p. ej. una pestaña): encuadrar salvo que el usuario ya haya movido la vista.
        if (layout && !userMoved) fit();
      },
      fit,
      /** Diagnóstico (tests). */
      debug: () => ({
        view: { ...view },
        width: canvas.clientWidth,
        height: canvas.clientHeight,
        alpha: layout ? layout.alpha : null,
        nodes: graph.nodes.length,
        visible: graph.nodes.filter((n) => !n.hidden).length,
        fitted,
        userMoved,
      }),
      destroy() {
        destroyed = true;
        if (observer) observer.disconnect();
        if (themeObserver) themeObserver.disconnect();
        if (darkQuery && darkQuery.removeEventListener) darkQuery.removeEventListener('change', draw);
        window.removeEventListener('resize', resize);
        container.textContent = '';
      },
    };
  }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = { Layout, KIND_COLORS, colorOf, interpolate };
  } else {
    root.AdocGraph = { mount, Layout, KIND_COLORS, colorOf };
  }
})(typeof window !== 'undefined' ? window : globalThis);
