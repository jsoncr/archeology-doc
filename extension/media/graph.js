// Grafo del conocimiento (estilo Obsidian) para el webview de la extensión.
// Sin dependencias: simulación de fuerzas + canvas. La simulación (`Layout`)
// no toca el DOM y se puede probar con Node.

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

  // ── Vista (solo en el navegador) ────────────────────────────────────────

  const KIND_LABELS = {
    overview: 'Overview',
    module: 'Módulos',
    feature: 'Funcionalidades',
    finding: 'Hallazgos',
    'test-report': 'Tests',
    'spec-report': 'Specs',
    glossary: 'Glosario',
    decision: 'Decisiones',
    memory: 'Memoria',
    faq: 'FAQ',
    moc: 'Índices',
    code: 'Código',
  };
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

  function start(vscode) {
    const canvas = document.getElementById('graph');
    const ctx = canvas.getContext('2d');
    const tip = document.getElementById('tip');
    const search = document.getElementById('search');
    const filters = document.getElementById('filters');
    const stats = document.getElementById('stats');
    const css = getComputedStyle(document.body);
    const fg = css.getPropertyValue('--vscode-foreground').trim() || '#ccc';
    const muted = css.getPropertyValue('--vscode-descriptionForeground').trim() || '#888';
    const bg = css.getPropertyValue('--vscode-editor-background').trim() || '#1e1e1e';
    // El canvas no resuelve variables CSS: se lee la fuente real del tema.
    const family = css.getPropertyValue('--vscode-font-family').trim() || 'sans-serif';

    let graph = { nodes: [], edges: [] };
    let nodeMap = new Map();
    let layout = null;
    let neighbors = new Map();
    let degree = new Map();
    let view = { x: 0, y: 0, k: 1 };
    let hover = null;
    let drag = null;
    let pan = null;
    let query = '';
    const hiddenKinds = new Set(['moc', 'code']);
    let running = false;

    function resize() {
      const dpr = window.devicePixelRatio || 1;
      canvas.width = canvas.clientWidth * dpr;
      canvas.height = canvas.clientHeight * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      draw();
    }

    function load(data) {
      // Conserva posiciones al refrescar (el agente añade notas mientras miras).
      const old = new Map((layout ? layout.nodes : []).map((n) => [n.id, n]));
      graph = data;
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
      layout = new Layout(graph.nodes, graph.edges);
      if (old.size) layout.alpha = 0.3;
      buildFilters();
      applyFilters();
      // Pre-cálculo: el grafo aparece casi asentado (≤120 ms, también en vaults grandes).
      const t0 = performance.now();
      while (performance.now() - t0 < 120 && layout.step()) {
        /* simulando */
      }
      if (!userMoved) fit();
      animate();
    }

    function buildFilters() {
      const counts = {};
      for (const n of graph.nodes) counts[n.kind] = (counts[n.kind] || 0) + 1;
      filters.innerHTML = '';
      for (const kind of Object.keys(counts).sort((a, b) => (a === 'code') - (b === 'code') || a.localeCompare(b))) {
        const label = document.createElement('label');
        const box = document.createElement('input');
        box.type = 'checkbox';
        box.checked = !hiddenKinds.has(kind);
        box.addEventListener('change', () => {
          if (box.checked) hiddenKinds.delete(kind);
          else hiddenKinds.add(kind);
          applyFilters();
        });
        const dot = document.createElement('span');
        dot.className = 'dot';
        dot.style.background = KIND_COLORS[kind] || '#9ca3af';
        label.append(box, dot, `${KIND_LABELS[kind] || kind} (${counts[kind]})`);
        filters.append(label);
      }
    }

    function applyFilters() {
      for (const n of graph.nodes) n.hidden = hiddenKinds.has(n.kind);
      const visible = graph.nodes.filter((n) => !n.hidden).length;
      const edges = graph.edges.filter((e) => !isHidden(e.source) && !isHidden(e.target)).length;
      stats.textContent = `${visible} nodos · ${edges} relaciones`;
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

    function draw() {
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, w, h);
      if (!layout) return;
      const nodes = nodeMap;
      const few = graph.nodes.filter((n) => !n.hidden).length <= 40;
      const focus = hover ? neighbors.get(hover.id) : null;
      const matches = (n) => !query || n.title.toLowerCase().includes(query) || n.id.toLowerCase().includes(query);

      ctx.lineWidth = 1;
      for (const e of graph.edges) {
        const a = nodes.get(e.source);
        const b = nodes.get(e.target);
        if (!a || !b || a.hidden || b.hidden) continue;
        const lit = hover && (a === hover || b === hover);
        ctx.strokeStyle = lit ? fg : muted;
        ctx.globalAlpha = hover ? (lit ? 0.9 : 0.06) : e.kind === 'evidence' ? 0.18 : 0.35;
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
        const dim = (hover && n !== hover && !(focus && focus.has(n.id))) || !matches(n);
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
        // Etiquetas: al acercar, al pasar el cursor, vecinos y nodos importantes.
        // Con pocos nodos visibles se rotulan todos; en grafos grandes, los más conectados.
        const important = few || (degree.get(n.id) || 0) >= 6 || n.kind === 'overview';
        const show = n === hover || (focus && focus.has(n.id)) || view.k > 1.3 || (important && view.k > 0.6) || (query && matches(n));
        if (show && !dim) {
          ctx.fillStyle = fg;
          ctx.font = `${n === hover ? 'bold ' : ''}11px ${family}`;
          ctx.textAlign = 'center';
          const label = n.title.length > 42 ? n.title.slice(0, 40) + '…' : n.title;
          ctx.fillText(label, p.x, p.y + r + 12);
        }
      }
      ctx.globalAlpha = 1;
    }

    /** Encuadra los nodos visibles. */
    let userMoved = false;
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

    let fitted = false;
    function animate() {
      if (running) return;
      running = true;
      let frames = 0;
      const tick = () => {
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

    // ── Interacción ──
    canvas.addEventListener('mousemove', (ev) => {
      const r = canvas.getBoundingClientRect();
      const x = ev.clientX - r.left;
      const y = ev.clientY - r.top;
      if (drag) {
        const w = toWorld(x, y);
        drag.x = w.x;
        drag.y = w.y;
        drag.moved = true;
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
        tip.textContent = `${n.title} (${KIND_LABELS[n.kind] || n.kind}${extra})${n.stale ? ' · desactualizada' : ''}`;
        tip.style.left = `${x + 14}px`;
        tip.style.top = `${y + 14}px`;
        tip.hidden = false;
      } else {
        tip.hidden = true;
      }
    });
    canvas.addEventListener('mousedown', (ev) => {
      const r = canvas.getBoundingClientRect();
      const n = nodeAt(ev.clientX - r.left, ev.clientY - r.top);
      if (n) {
        drag = n;
        n.fixed = true;
        n.moved = false;
      } else {
        pan = { x: ev.clientX, y: ev.clientY };
        canvas.style.cursor = 'grabbing';
      }
    });
    window.addEventListener('mouseup', () => {
      if (drag) {
        drag.fixed = false;
        if (!drag.moved) {
          vscode.postMessage({ type: 'open', file: drag.file, line: drag.line });
        }
        drag = null;
      }
      pan = null;
      canvas.style.cursor = hover ? 'pointer' : 'grab';
    });
    canvas.addEventListener('mouseleave', () => {
      hover = null;
      tip.hidden = true;
      draw();
    });
    canvas.addEventListener(
      'wheel',
      (ev) => {
        ev.preventDefault();
        userMoved = true;
        const r = canvas.getBoundingClientRect();
        const before = toWorld(ev.clientX - r.left, ev.clientY - r.top);
        view.k = Math.max(0.15, Math.min(4, view.k * Math.exp(-ev.deltaY * 0.0015)));
        const after = toWorld(ev.clientX - r.left, ev.clientY - r.top);
        view.x += after.x - before.x;
        view.y += after.y - before.y;
        draw();
      },
      { passive: false },
    );
    search.addEventListener('input', () => {
      query = search.value.trim().toLowerCase();
      draw();
    });
    document.getElementById('fit').addEventListener('click', () => {
      userMoved = false;
      fit();
    });
    document.getElementById('refresh').addEventListener('click', () => vscode.postMessage({ type: 'refresh' }));
    window.addEventListener('resize', resize);
    window.addEventListener('message', (ev) => {
      if (ev.data && ev.data.type === 'graph') load(ev.data.graph);
    });
    resize();
    vscode.postMessage({ type: 'ready' });
    // Para el dashboard: la pestaña del grafo empieza oculta (tamaño 0).
    return {
      resize() {
        resize();
        // Al mostrar la pestaña: encuadrar salvo que el usuario ya haya movido la vista.
        if (layout && !userMoved) fit();
      },
      fit,
      /** Diagnóstico (tests de la extensión). */
      debug: () => ({
        view: { ...view },
        width: canvas.clientWidth,
        height: canvas.clientHeight,
        alpha: layout ? layout.alpha : null,
        nodes: graph.nodes.length,
        fitted,
        userMoved,
      }),
    };
  }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = { Layout };
  } else {
    // El dashboard lo inicia con su propia instancia de la API de VS Code.
    root.AdocGraph = { start };
  }
})(typeof window !== 'undefined' ? window : globalThis);
