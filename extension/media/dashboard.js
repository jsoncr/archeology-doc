// Dashboard de archeology-doc (webview). Dibuja el JSON de `adoc status`;
// las acciones se envían a la extensión con postMessage.

(function () {
  'use strict';
  const vscode = acquireVsCodeApi();
  // Textos en el idioma de la extensión (media/i18n.js); sin él, se ven las claves.
  const { t, lang } = window.AdocI18n || { t: (k) => k, lang: 'en' };
  const $ = (id) => document.getElementById(id);
  let status = null;
  let findingFilter = { severity: 'all', state: 'open' };
  let docsFilter = 'all';

  // ── Utilidades ──────────────────────────────────────────────────────────

  /** Escapa texto escrito por agentes o personas antes de insertarlo. */
  function esc(s) {
    return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  }

  function bytes(b) {
    if (b === null || b === undefined) return '—';
    const units = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    let v = b;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${v < 10 && i ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
  }

  const num = (n) => (n === null || n === undefined ? '—' : Number(n).toLocaleString(lang));

  // Colores de lenguajes (los más comunes); el resto, gris.
  const LANG_COLORS = {
    JavaScript: '#f1e05a', TypeScript: '#3178c6', Python: '#3572a5', Rust: '#dea584', Go: '#00add8', Java: '#b07219',
    Kotlin: '#a97bff', 'C#': '#178600', 'C++': '#f34b7d', C: '#555555', Ruby: '#701516', PHP: '#4f5d95', Swift: '#f05138',
    Templates: '#a3be8c', HTML: '#e34c26', CSS: '#563d7c', Vue: '#41b883', Svelte: '#ff3e00', Shell: '#89e051',
    SQL: '#e38c00', JSON: '#8b949e', YAML: '#cb171e', Markdown: '#6e7781', TOML: '#9c4221', Dart: '#00b4ab',
  };

  function ms(v) {
    if (v === null || v === undefined) return '—';
    const s = Math.round(v / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ${String(s % 60).padStart(2, '0')}s`;
    return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`;
  }

  function ago(iso) {
    if (!iso) return '—';
    const d = (Date.now() - new Date(iso).getTime()) / 1000;
    if (d < 60) return t('web.ago.now');
    if (d < 3600) return t('web.ago.minutes', { n: Math.round(d / 60) });
    if (d < 86400) return t('web.ago.hours', { n: Math.round(d / 3600) });
    return t('web.ago.days', { n: Math.round(d / 86400) });
  }

  /** Markdown mínimo para el extracto del overview (sobre texto ya escapado). */
  function md(text) {
    const inline = (s) =>
      esc(s)
        .replace(/`([^`]+)`/g, '<code>$1</code>')
        .replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>')
        .replace(/\[\[([^\]|]+)\|([^\]]+)\]\]/g, '$2')
        .replace(/\[\[([^\]]+)\]\]/g, '$1')
        .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1');
    return text
      .split(/\n{2,}/)
      .map((block) => {
        const b = block.trim();
        if (!b) return '';
        if (b.startsWith('## ') || b.startsWith('### ')) return `<h4>${inline(b.replace(/^#+\s*/, ''))}</h4>`;
        if (/^[-*] /m.test(b)) {
          // Las líneas de continuación se unen al punto anterior.
          const items = [];
          for (const line of b.split('\n')) {
            if (/^\s*[-*] /.test(line)) items.push(line.replace(/^\s*[-*] /, ''));
            else if (items.length) items[items.length - 1] += ' ' + line.trim();
          }
          return `<ul>${items.map((l) => `<li>${inline(l)}</li>`).join('')}</ul>`;
        }
        return `<p>${inline(b.replace(/\n/g, ' '))}</p>`;
      })
      .join('');
  }

  const SEVERITIES = ['critical', 'high', 'medium', 'low'];
  const SEV_LABEL = { unknown: '—' };
  for (const x of SEVERITIES) SEV_LABEL[x] = t(`web.sev.${x}`);
  const STATUS_LABEL = {};
  for (const x of ['pending', 'done', 'failed', 'running', 'manual', 'question']) STATUS_LABEL[x] = t(`web.status.${x}`);
  const KIND_LABEL = {};
  for (const k of ['overview', 'module', 'feature', 'finding', 'test-report', 'spec-report', 'glossary', 'decision', 'memory', 'faq', 'moc']) {
    KIND_LABEL[k] = t(`kind.${k}`);
  }
  const pill = (cls, text) => `<span class="pill ${esc(cls)}">${esc(text)}</span>`;
  const openLink = (file, text, line) =>
    `<a href="#" data-open="${esc(file)}"${line ? ` data-line="${line}"` : ''}>${text}</a>`;
  const evidenceLink = (src) => {
    const m = /^(.*):(\d+)(?:-\d+)?$/.exec(src);
    const file = `${status.project.root}/${m ? m[1] : src}`;
    return openLink(file, `<code>${esc(src)}</code>`, m ? m[2] : undefined);
  };
  const runBtn = (args, label, title, cls = '') =>
    `<button class="sm ${cls}" data-run="${esc(args)}" title="${esc(title || 'adoc ' + args)}">${label}</button>`;
  const empty = (text) => `<p class="empty">${text}</p>`;

  // ── Proyecto (inventario) ───────────────────────────────────────────────

  /** Barra de proporción por lenguaje (por líneas). */
  function langBar(langs, total) {
    if (!total) return '';
    return `<div class="langbar">${langs
      .filter((l) => l.lines > 0)
      .map((l) => `<i style="width:${(100 * l.lines) / total}%;background:${LANG_COLORS[l.name] || '#8b949e'}" title="${esc(t('web.linesTitle', { name: l.name, n: num(l.lines) }))}"></i>`)
      .join('')}</div>`;
  }

  function inventoryStrip(inv) {
    const main = inv.languages[0];
    return `<div class="strip">
      <span>${t('web.strip.files', { n: `<b>${num(inv.total_files)}</b>` })}</span>
      <span>${t('web.strip.disk', { n: `<b>${bytes(inv.total_bytes)}</b>` })}</span>
      <span>${t('web.strip.lines', { n: `<b>${num(inv.total_lines)}</b>` })}</span>
      <span>${t('web.strip.codeFiles', { n: `<b>${num(inv.code_files)}</b>` })}</span>
      ${main ? `<span>${t('web.strip.main', { name: `<b>${esc(main.name)}</b>` })}</span>` : ''}
      <span>${t('web.strip.modules', { n: `<b>${inv.modules.length}</b>` })}</span>
      <span>${t('web.strip.tests', { n: `<b>${inv.test_files.length}</b>` })}${inv.disabled_tests.length ? ` <span class="medium">(${t('web.disabled', { n: inv.disabled_tests.length })})</span>` : ''}</span>
      ${inv.git ? `<span>${t('web.strip.commits', { n: `<b>${num(inv.git.commits_analyzed)}</b>`, authors: inv.git.authors.length })}</span>` : ''}
      <a href="#" data-goto="project">${t('web.strip.more')}</a>
    </div>${langBar(inv.languages, inv.total_lines)}`;
  }

  function renderProject() {
    const inv = status.inventory;
    if (!inv) {
      $('project').innerHTML =
        empty(t('web.project.noInventory')) +
        `<p>${runBtn('scan', t('web.scan'))}</p>`;
      return;
    }
    const textBytes = inv.languages.reduce((a, l) => a + l.bytes, 0);
    const card = (value, label, extra = '') =>
      `<div class="card static"><div class="value">${value}</div><div class="label">${label}</div>${extra}</div>`;
    let html = `<div class="row-info"><span>${t('web.project.inventoryOf', { date: esc(new Date(inv.generated_at).toLocaleString(lang)), ago: ago(inv.generated_at) })}</span><span class="grow"></span>${runBtn('scan', t('web.rescan'))}</div>`;
    html += `<div class="cards">
      ${card(num(inv.total_files), t('web.files'), `<div class="sub">${t('web.card.codeSub', { n: num(inv.code_files) })}</div>`)}
      ${card(bytes(inv.total_bytes), t('web.card.diskSize'), `<div class="sub">${t('web.card.diskSub', { text: bytes(textBytes), other: bytes(Math.max(0, inv.total_bytes - textBytes)) })}</div>`)}
      ${card(num(inv.total_lines), t('web.lines'), `<div class="sub">${t('web.card.languagesSub', { n: inv.languages.length })}</div>`)}
      ${card(inv.modules.length, t('web.modules'), `<div class="sub">${t('web.profileOf', { profile: esc(status.profile) })}</div>`)}
      ${card(inv.test_files.length, t('web.card.testFiles'), `<div class="sub">${inv.test_frameworks.length ? esc(inv.test_frameworks.join(', ')) : t('web.card.noFramework')}${inv.disabled_tests.length ? ` · <span class="medium">${t('web.disabled', { n: inv.disabled_tests.length })}</span>` : ''}</div>`)}
      ${card(inv.entrypoints.length, t('web.card.entrypoints'))}
      ${inv.git ? card(num(inv.git.commits_analyzed), t('web.card.commits'), `<div class="sub">${t('web.authorsCount', { n: inv.git.authors.length })}</div>`) : ''}
      ${card(Object.values(inv.markers_by_kind).reduce((a, b) => a + b, 0), t('web.card.markers'), `<div class="sub">${Object.entries(inv.markers_by_kind).map(([k, v]) => `${esc(k)} ${v}`).join(' · ') || t('web.none')}</div>`)}
    </div>`;

    // Lenguajes.
    html += `<div class="cols"><div class="box"><h3>${t('web.languages')}</h3>${langBar(inv.languages, inv.total_lines)}
      <table class="compact"><thead><tr><th>${t('web.th.language')}</th><th class="r">${t('web.files')}</th><th class="r">${t('web.lines')}</th><th class="r">${t('web.th.size')}</th><th class="r">%</th></tr></thead><tbody>
      ${inv.languages
        .map(
          (l) => `<tr><td><span class="dot" style="background:${LANG_COLORS[l.name] || '#8b949e'}"></span> ${esc(l.name)}</td>
          <td class="r">${num(l.files)}</td><td class="r">${num(l.lines)}</td><td class="r">${bytes(l.bytes)}</td>
          <td class="r">${inv.total_lines ? ((100 * l.lines) / inv.total_lines).toFixed(1) : 0}%</td></tr>`,
        )
        .join('')}
      </tbody></table></div>`;

    // Stack y estructura.
    const chips = (list) => (list.length ? list.map((x) => `<span class="tag">${esc(x)}</span>`).join(' ') : '<span class="muted">—</span>');
    const files = (list, max = 12) =>
      list.length
        ? `<ul class="files">${list.slice(0, max).map((f) => `<li>${openLink(`${status.project.root}/${f.replace(/ \(.*\)$/, '')}`, `<code>${esc(f)}</code>`)}</li>`).join('')}${list.length > max ? `<li class="muted">${t('web.moreCount', { n: list.length - max })}</li>` : ''}</ul>`
        : '<span class="muted">—</span>';
    html += `<div class="box"><h3>${t('web.stack')}</h3><dl>
      <dt>${t('web.frameworks')}</dt><dd>${chips(inv.frameworks)}</dd>
      <dt>${t('web.tests')}</dt><dd>${chips(inv.test_frameworks)}</dd>
      <dt>${t('web.manifests')}</dt><dd>${files(inv.manifests)}</dd>
      <dt>${t('web.entry')}</dt><dd>${files(inv.entrypoints)}</dd>
      <dt>CI</dt><dd>${files(inv.ci_files)}</dd>
      <dt>${t('web.priorDocs')}</dt><dd>${files(inv.doc_files, 6)}</dd>
    </dl></div></div>`;

    // Git.
    if (inv.git) {
      const g = inv.git;
      const maxHot = g.hotspots.length ? g.hotspots[0][1] : 1;
      html += `<div class="cols"><div class="box"><h3>${t('web.gitHistory')}</h3><dl>
        <dt>${t('web.lastCommit')}</dt><dd>${g.last_commit_hash ? `<code>${esc(g.last_commit_hash)}</code> ${esc(g.last_commit_subject || '')} <span class="muted">· ${ago(g.last_commit_date)}</span>` : '—'}</dd>
        <dt>Commits</dt><dd>${t('web.analyzed', { n: num(g.commits_analyzed) })}</dd>
        <dt>${t('web.authors')}</dt><dd>${g.authors.map((a) => esc(a.replace(/^\s*\d+\s+/, ''))).join('<br>') || '—'}</dd>
      </dl></div>
      <div class="box"><h3>${t('web.hotspots')}</h3>
        ${g.hotspots.length ? `<table class="compact">${g.hotspots
          .map(([f, n]) => `<tr><td>${openLink(`${status.project.root}/${f}`, `<code>${esc(f)}</code>`)}</td><td class="r nowrap">${n}</td><td class="hot"><i style="width:${(100 * n) / maxHot}%"></i></td></tr>`)
          .join('')}</table>` : empty(t('web.noData'))}
      </div></div>`;
    }

    // Módulos.
    html += `<div class="box"><h3>${t('web.modules')} (${inv.modules.length})</h3><table class="compact"><thead><tr><th>${t('web.th.module')}</th><th>${t('web.th.path')}</th><th class="r">${t('web.files')}</th><th class="r">${t('web.th.code')}</th><th class="r">${t('web.th.size')}</th><th>${t('web.languages')}</th><th>${t('web.tests')}</th></tr></thead><tbody>
      ${[...inv.modules]
        .sort((a, b) => b.bytes - a.bytes)
        .map(
          (m) => `<tr><td><b>${esc(m.name)}</b></td><td><code>${esc(m.path)}${m.recursive === false ? '/*' : '/'}</code></td><td class="r">${num(m.files)}</td>
          <td class="r">${num(m.code_files)}</td><td class="r">${bytes(m.bytes)}</td><td>${esc((m.languages || []).join(', '))}</td>
          <td>${m.has_tests ? `<span class="ok">${t('web.yes')}</span>` : `<span class="muted">${t('web.no')}</span>`}</td></tr>`,
        )
        .join('')}
      </tbody></table></div>`;

    // Marcadores.
    if (inv.markers.length) {
      html += `<div class="box"><h3>${t('web.codeMarkers')}</h3><table class="compact">${inv.markers
        .map((m) => `<tr><td>${pill(m.kind === 'TODO' ? 'pending' : 'question', m.kind)}</td><td>${openLink(`${status.project.root}/${m.path}`, `<code>${esc(m.path)}:${m.line}</code>`, m.line)}</td><td class="muted">${esc(m.text)}</td></tr>`)
        .join('')}</table></div>`;
    }
    $('project').innerHTML = html;
  }

  // ── Resumen ─────────────────────────────────────────────────────────────

  function renderSummary() {
    const s = status;
    const total = s.tasks.length;
    const done = s.tasks.filter((x) => x.status === 'done').length;
    const pending = total - done;
    const open = s.findings.filter((f) => f.state === 'open' || f.state === 'confirmed');
    const q = s.questions;
    const failed = s.tasks.filter((x) => x.status === 'failed');
    const sev = SEVERITIES.filter((x) => s.severity_totals[x]).map((x) => `<span class="${x}">${s.severity_totals[x]} ${SEV_LABEL[x].toLowerCase()}</span>`).join(' · ');

    const card = (value, label, extra = '', tab = '') =>
      `<div class="card"${tab ? ` data-goto="${tab}"` : ''}><div class="value">${value}</div><div class="label">${label}</div>${extra}</div>`;
    let html = `<div class="cards">
      ${card(`${s.progress}%`, t('web.sum.plan', { done, total }), `<div class="bar"><i style="width:${Math.min(100, s.progress)}%"></i></div>`, 'tasks')}
      ${card(pending, t('web.sum.todo'), failed.length ? `<div class="sub failed">${t('web.sum.failed', { n: failed.length })}</div>` : '', 'tasks')}
      ${card(open.length, t('web.sum.openFindings'), sev ? `<div class="sub">${sev}</div>` : '', 'findings')}
      ${card(q.agent.length + q.team.length, t('web.sum.pendingQuestions'), `<div class="sub">${t('web.sum.questionsSub', { agent: q.agent.length, team: q.team.length })}</div>`, 'questions')}
      ${card(s.features_without_tests.length, t('web.sum.featuresNoTests'), `<div class="sub">${t('web.sum.missingScenarios', { n: s.missing_scenarios })}</div>`, 'docs')}
      ${card(ms(s.timing.total_agent_ms), t('web.sum.agentTime'), `<div class="sub">${t('web.sum.runsSub', { n: s.timing.runs, ago: ago(s.timing.last_run_at) })}</div>`, 'tasks')}
      ${card(s.timing.estimated_remaining_ms ? `~${ms(s.timing.estimated_remaining_ms)}` : '—', t('web.sum.remaining'), `<div class="sub">${t('web.sum.avgSub', { time: ms(s.timing.avg_done_ms) })}</div>`, 'tasks')}
      ${card(s.notes.length, t('web.sum.notes'), s.stale_notes.length ? `<div class="sub stale">${t('web.sum.staleSub', { n: s.stale_notes.length })}</div>` : `<div class="sub">${t('web.sum.upToDate')}</div>`, 'docs')}
    </div>`;

    // Lo que requiere atención, en orden de urgencia.
    const items = [];
    for (const a of q.agent) items.push(`<li class="q">❓ ${t('web.att.asks', { task: esc(a.task), question: esc(a.question) })} <a href="#" data-goto="questions">${t('web.answer')}</a></li>`);
    for (const f of failed) items.push(`<li class="failed">✗ ${t('web.att.failed', { id: esc(f.id) })}${f.last_run && f.last_run.reason ? `: ${esc(f.last_run.reason.slice(0, 160))}` : ''} ${runBtn(`task run ${f.id}`, t('web.retry'))}</li>`);
    for (const f of open.filter((f) => f.severity === 'critical' || f.severity === 'high').slice(0, 6)) {
      items.push(`<li>${pill(f.severity, SEV_LABEL[f.severity])} ${openLink(f.file, esc(f.title))}</li>`);
    }
    if (s.stale_notes.length) items.push(`<li class="stale">⟳ ${t('web.att.stale', { n: s.stale_notes.length })} ${runBtn('inspect', t('web.regenerate'))}</li>`);
    if (q.team.length) items.push(`<li>👥 ${t('web.att.team', { n: q.team.length })} <a href="#" data-goto="questions">${t('web.view')}</a></li>`);
    html += `<div class="cols"><div class="box"><h3>${t('web.att.title')}</h3>${items.length ? `<ul class="attention">${items.join('')}</ul>` : empty(t('web.att.none'))}</div>`;

    // Agente y perfil.
    html += `<div class="box"><h3>${t('web.project')}</h3><dl>
      <dt>${t('web.agent')}</dt><dd>${esc(s.agent.active)}${s.agent.model ? ` · ${esc(s.agent.model)}` : ''} <span class="muted">(${esc(s.agent.mode)})</span></dd>
      <dt>${t('web.profile')}</dt><dd>${esc(s.profile)}</dd>
      <dt>${t('web.th.path')}</dt><dd><code>${esc(s.project.root)}</code></dd>
      <dt>${t('web.workState')}</dt><dd><code>${esc(s.project.workspace)}</code></dd>
    </dl>`;
    if (s.specs && s.specs.totals) {
      const totals = s.specs.totals;
      const totalReq = Object.values(totals).reduce((a, b) => a + b, 0) || 1;
      const seg = (k, cls) => (totals[k] ? `<i class="${cls}" style="width:${(100 * totals[k]) / totalReq}%" title="${k}: ${totals[k]}"></i>` : '');
      html += `<h3>Specs (OpenSpec)</h3><div class="stack">${seg('implemented', 'ok')}${seg('partial', 'medium')}${seg('contradicted', 'high')}${seg('not-found', 'critical')}${seg('unevaluated', 'none')}</div>
        <div class="sub">${Object.entries(totals).map(([k, v]) => `${esc(k)}: ${v}`).join(' · ')}</div>`;
    }
    html += `</div></div>`;

    if (s.overview_excerpt) {
      const overview = s.notes.find((n) => n.kind === 'overview');
      html += `<div class="box overview"><h3>${t('web.aboutProject')} ${overview ? openLink(overview.file, `<span class="muted">${t('web.openOverview')}</span>`) : ''}</h3>${md(s.overview_excerpt)}</div>`;
    }
    if (s.inventory) {
      html = inventoryStrip(s.inventory) + html;
    }
    $('summary').innerHTML = html;
  }

  // ── Tareas ──────────────────────────────────────────────────────────────

  function renderTasks() {
    const s = status;
    const order = { question: 0, failed: 1, running: 2, pending: 3, manual: 4, done: 5 };
    const tasks = [...s.tasks].sort((a, b) => (order[a.status] ?? 9) - (order[b.status] ?? 9) || a.id.localeCompare(b.id));
    let html = `<div class="row-info">
      <span>${t('web.tasks.totalTime', { time: `<b>${ms(s.timing.total_agent_ms)}</b>` })}</span>
      <span>${t('web.tasks.avg', { time: `<b>${ms(s.timing.avg_done_ms)}</b>` })}</span>
      <span>${t('web.tasks.remaining', { time: `<b>${s.timing.estimated_remaining_ms ? '~' + ms(s.timing.estimated_remaining_ms) : '—'}</b>` })}</span>
      <span>${t('web.tasks.lastRun', { time: `<b>${ago(s.timing.last_run_at)}</b>` })}</span>
      <span class="grow"></span>
      ${runBtn('plan --force', t('web.replan'))}
    </div>`;
    if (!tasks.length) {
      $('tasks').innerHTML = html + empty(t('web.tasks.none')) + `<p>${runBtn('scan', t('web.scan'))} ${runBtn('plan', t('web.plan'))}</p>`;
      return;
    }
    html += `<table><thead><tr><th>${t('web.th.status')}</th><th>${t('web.th.task')}</th><th>${t('web.th.type')}</th><th>${t('web.modules')}</th><th>${t('web.th.duration')}</th><th>${t('web.th.attempts')}</th><th>${t('web.th.last')}</th><th></th></tr></thead><tbody>`;
    for (const task of tasks) {
      const r = task.last_run;
      const modules = (task.module || '').split(',').filter(Boolean);
      const scope = modules.length > 3 ? `${esc(modules.slice(0, 3).join(', '))} <span class="muted">+${modules.length - 3}</span>` : esc(modules.join(', ') || '—');
      const actions = [
        task.status !== 'done' ? runBtn(`task run ${task.id}`, '▶', t('web.run')) : runBtn(`task run ${task.id}`, '↻', t('web.rerun')),
        runBtn(`task run ${task.id} --interactive`, '💬', t('web.interactive')),
        task.status === 'question' ? `<button class="sm" data-goto="questions" title="${esc(t('web.answer'))}">✍</button>` : '',
        task.log ? `<button class="sm" data-open="${esc(task.log)}" title="${esc(t('web.viewLog'))}">log</button>` : '',
      ].join('');
      html += `<tr>
        <td>${pill(task.status, STATUS_LABEL[task.status] || task.status)}</td>
        <td>${openLink(task.file, `<b>${esc(task.id)}</b>`)}${task.question ? `<div class="sub q">❓ ${esc(task.question)}</div>` : ''}${r && r.reason && task.status !== 'done' ? `<div class="sub failed">${esc(r.reason.slice(0, 140))}</div>` : ''}</td>
        <td>${esc(task.kind)}</td>
        <td class="scope">${scope}</td>
        <td>${r ? ms(r.duration_ms) : '—'}</td>
        <td>${r ? r.attempts : '—'}</td>
        <td>${r ? `${ago(r.finished_at)}<div class="sub">${esc(r.agent)}</div>` : '—'}</td>
        <td class="nowrap">${actions}</td>
      </tr>`;
    }
    $('tasks').innerHTML = html + '</tbody></table>';
  }

  // ── Hallazgos ───────────────────────────────────────────────────────────

  function renderFindings() {
    const s = status;
    const chips = ['all', ...SEVERITIES]
      .map((x) => `<button class="chip ${findingFilter.severity === x ? 'on' : ''}" data-sev="${x}">${x === 'all' ? t('web.sevAll') : SEV_LABEL[x]}${x !== 'all' && s.severity_totals[x] ? ` · ${s.severity_totals[x]}` : ''}</button>`)
      .join('');
    const states = ['open', 'all', 'resolved', 'dismissed'].map((v) => [v, t(`web.state.${v}`)])
      .map(([v, l]) => `<option value="${v}" ${findingFilter.state === v ? 'selected' : ''}>${l}</option>`)
      .join('');
    const rank = (f) => SEVERITIES.indexOf(f.severity) + 1 || 9;
    const list = s.findings
      .filter((f) => findingFilter.severity === 'all' || f.severity === findingFilter.severity)
      .filter((f) => findingFilter.state === 'all' || (findingFilter.state === 'open' ? f.state === 'open' || f.state === 'confirmed' : f.state === findingFilter.state))
      .sort((a, b) => rank(a) - rank(b));
    let html = `<div class="row-info">${chips}<select id="fstate">${states}</select><span class="grow"></span>${runBtn('check', t('web.validate'))}</div>`;
    if (!list.length) {
      $('findings').innerHTML = html + empty(s.findings.length ? t('web.findings.noneFilter') : t('web.findings.none'));
      return;
    }
    html += `<table><thead><tr><th>${t('web.th.severity')}</th><th>${t('web.th.finding')}</th><th>${t('web.th.type')}</th><th>${t('web.th.status')}</th><th>${t('web.th.evidence')}</th></tr></thead><tbody>`;
    for (const f of list) {
      html += `<tr><td>${pill(f.severity, SEV_LABEL[f.severity] || f.severity)}</td><td>${openLink(f.file, esc(f.title))}</td>
        <td>${esc(f.kind)}</td><td>${esc(f.state)}</td><td>${f.sources.map(evidenceLink).join(' ')}</td></tr>`;
    }
    $('findings').innerHTML = html + '</tbody></table>';
  }

  // ── Documentación ───────────────────────────────────────────────────────

  function renderDocs() {
    const s = status;
    const stale = new Set(s.stale_notes.map((n) => `${s.project.root}/${n}`));
    const counts = {};
    for (const n of s.notes) if (n.kind !== 'finding') counts[n.kind] = (counts[n.kind] || 0) + 1;
    const kinds = Object.keys(counts).sort((a, b) => (KIND_LABEL[a] || a).localeCompare(KIND_LABEL[b] || b));
    let html = `<div class="row-info">
      <button class="chip ${docsFilter === 'all' ? 'on' : ''}" data-kind="all">${t('web.docs.all', { n: Object.values(counts).reduce((a, b) => a + b, 0) })}</button>
      ${kinds.map((k) => `<button class="chip ${docsFilter === k ? 'on' : ''}" data-kind="${esc(k)}">${esc(KIND_LABEL[k] || k)} · ${counts[k]}</button>`).join('')}
      <span class="grow"></span>${runBtn('preview', t('web.viewSite'))} ${runBtn('report', t('web.htmlReport'))} <button class="sm" data-goto="graph">${t('web.graph')}</button>
    </div>`;
    if (s.features_without_tests.length && (docsFilter === 'all' || docsFilter === 'feature')) {
      html += `<div class="box warn"><b>${t('web.docs.noTests', { n: s.features_without_tests.length })}</b> ${s.features_without_tests.map(esc).join(', ')}</div>`;
    }
    const notes = s.notes
      .filter((n) => n.kind !== 'finding' && (docsFilter === 'all' || n.kind === docsFilter))
      .sort((a, b) => (KIND_LABEL[a.kind] || a.kind).localeCompare(KIND_LABEL[b.kind] || b.kind) || a.title.localeCompare(b.title));
    if (!notes.length) {
      $('docs').innerHTML = html + empty(t('web.docs.none'));
      return;
    }
    html += `<table><thead><tr><th>${t('web.th.note')}</th><th>${t('web.th.type')}</th><th>${t('web.th.status')}</th><th>${t('web.th.evidence')}</th></tr></thead><tbody>`;
    for (const n of notes) {
      html += `<tr><td>${openLink(n.file, esc(n.title))}${stale.has(n.file) ? ' ' + pill('stale', t('web.stale')) : ''}</td>
        <td>${esc(KIND_LABEL[n.kind] || n.kind)}</td><td>${esc(n.status || '—')}</td><td>${n.sources.length || '—'}</td></tr>`;
    }
    $('docs').innerHTML = html + '</tbody></table>';
  }

  // ── Preguntas ───────────────────────────────────────────────────────────

  function renderQuestions() {
    const q = status.questions;
    const openQuestions = `${status.project.knowledge}/memory/open-questions.md`;
    let html = `<div class="box ask"><h3>${t('web.q.askTitle')}</h3>
      <div class="ask-row"><input id="ask" placeholder="${esc(t('web.q.placeholder'))}"><button class="primary" id="askBtn">${t('web.q.ask')}</button></div>
      <div class="sub">${t('web.q.askHint')}</div></div>`;

    html += `<h3>${t('web.q.fromAgent')} <span class="muted">· ${t('web.q.blocking')}</span></h3>`;
    html += q.agent.length
      ? q.agent
          .map(
            (a) => `<div class="box qbox"><div><b>${esc(a.task)}</b> · ${esc(a.question)}</div>
          <div class="ask-row"><input data-answer-input="${esc(a.task)}" placeholder="${esc(t('web.q.yourAnswer'))}"><button class="primary" data-answer="${esc(a.task)}">${t('web.answer')}</button>
          ${runBtn(`task run ${a.task} --interactive`, `💬 ${t('web.q.live')}`, t('web.q.liveTitle'))}</div></div>`,
          )
          .join('')
      : empty(t('web.q.noneAgent'));

    html += `<h3>${t('web.q.team')} <span class="muted">· ${openLink(openQuestions, t('web.q.openFile'))}</span></h3>`;
    html += q.team.length
      ? `<ul class="team">${q.team.map((entry) => `<li>${esc(entry.replace(/^\d{4}-\d{2}-\d{2} · /, ''))}</li>`).join('')}</ul>`
      : empty(t('web.q.noneTeam'));

    html += `<h3>${t('web.q.faq')}</h3>`;
    html += q.faq.length
      ? q.faq
          .map(
            (f) => `<details class="faq"><summary>${esc(f.question)} <span class="muted">· ${f.origin === 'agente' ? t('origin.agent') : t('origin.team')}</span></summary>
          <div class="answer">${md(f.answer)}</div><div class="sub">${openLink(f.file, t('web.q.source'))}</div></details>`,
          )
          .join('')
      : empty(t('web.q.noneFaq'));
    $('questions').innerHTML = html;
  }

  // ── Render y eventos ────────────────────────────────────────────────────

  /**
   * Completa los campos que un CLI más antiguo no envía: el dashboard debe
   * funcionar (con menos datos) aunque la extensión sea más nueva que `adoc`.
   */
  function normalize(s) {
    const outdated = [];
    const need = (key, value) => {
      if (s[key] === undefined || s[key] === null) {
        if (value !== null) outdated.push(key);
        s[key] = value;
      }
    };
    need('tasks', []);
    need('findings', []);
    need('notes', []);
    need('stale_notes', []);
    need('severity_totals', {});
    need('features_without_tests', []);
    need('missing_scenarios', 0);
    need('questions', { agent: [], team: [], faq: [] });
    need('timing', { runs: 0, total_agent_ms: 0, avg_done_ms: null, estimated_remaining_ms: null, last_run_at: null });
    s.overview_excerpt = s.overview_excerpt || null;
    if (s.inventory === undefined) {
      outdated.push('inventory');
      s.inventory = null;
    }
    s.questions.agent = s.questions.agent || [];
    s.questions.team = s.questions.team || [];
    s.questions.faq = s.questions.faq || [];
    s.agent = s.agent || { active: '?', model: null, mode: '?', available: [] };
    s.project = s.project || { name: '?', root: '', workspace: '', knowledge: '' };
    s.__outdated = outdated;
    return s;
  }

  /** Pinta una pestaña; si falla, muestra el error en ella en vez de dejarla en blanco. */
  function safe(id, fn) {
    try {
      fn();
    } catch (err) {
      $(id).innerHTML = `<div class="box warn"><b>${t('web.error.section')}</b><div class="sub">${esc(err && err.message)}</div>
        <div class="sub">${t('web.error.hint')}</div></div>`;
      console.error(err);
    }
  }

  function render() {
    if (!status) {
      for (const id of ['summary', 'project', 'tasks', 'findings', 'docs', 'questions']) {
        $(id).innerHTML = empty(t('web.noProject'));
      }
      return;
    }
    status = normalize(status);
    $('name').textContent = status.project.name;
    $('meta').textContent = `${status.agent.active}${status.agent.model ? ' · ' + status.agent.model : ''} · ${t('web.profileOf', { profile: status.profile })} · ${status.progress}% · ${status.project.root}`;
    $('outdated').hidden = !status.__outdated.length;
    $('outdated').innerHTML = status.__outdated.length
      ? `⚠ ${t('web.outdated', { fields: esc(status.__outdated.join(', ')) })}`
      : '';
    safe('summary', renderSummary);
    safe('project', renderProject);
    safe('tasks', renderTasks);
    safe('findings', renderFindings);
    safe('docs', renderDocs);
    safe('questions', renderQuestions);
  }

  let graph = null;
  function showTab(tab) {
    document.querySelectorAll('#tabs button').forEach((b) => b.classList.toggle('active', b.dataset.tab === tab));
    document.querySelectorAll('.panel').forEach((p) => p.classList.toggle('active', p.dataset.panel === tab));
    vscode.setState({ tab });
    if (tab === 'graph' && graph) requestAnimationFrame(() => graph.resize());
  }

  document.addEventListener('click', (ev) => {
    const el = ev.target.closest('[data-open],[data-run],[data-command],[data-goto],[data-tab],[data-sev],[data-kind],[data-answer],#askBtn,#reload');
    if (!el) return;
    ev.preventDefault();
    if (el.dataset.tab) showTab(el.dataset.tab);
    else if (el.dataset.goto) showTab(el.dataset.goto);
    else if (el.dataset.open) vscode.postMessage({ type: 'open', file: el.dataset.open, line: el.dataset.line ? Number(el.dataset.line) : undefined });
    else if (el.dataset.run) vscode.postMessage({ type: 'run', args: el.dataset.run.split(' ') });
    else if (el.dataset.command) vscode.postMessage({ type: 'command', id: el.dataset.command });
    else if (el.dataset.sev) {
      findingFilter.severity = el.dataset.sev;
      renderFindings();
    } else if (el.dataset.kind) {
      docsFilter = el.dataset.kind;
      renderDocs();
    } else if (el.dataset.answer) {
      const input = document.querySelector(`[data-answer-input="${CSS.escape(el.dataset.answer)}"]`);
      if (input && input.value.trim()) vscode.postMessage({ type: 'answer', task: el.dataset.answer, text: input.value });
    } else if (el.id === 'askBtn') {
      const input = $('ask');
      if (input.value.trim()) {
        vscode.postMessage({ type: 'ask', question: input.value });
        input.value = '';
      }
    } else if (el.id === 'reload') vscode.postMessage({ type: 'refresh' });
  });
  document.addEventListener('change', (ev) => {
    if (ev.target.id === 'fstate') {
      findingFilter.state = ev.target.value;
      renderFindings();
    }
  });
  document.addEventListener('keydown', (ev) => {
    if (ev.key !== 'Enter') return;
    if (ev.target.id === 'ask') $('askBtn').click();
    if (ev.target.dataset && ev.target.dataset.answerInput) {
      document.querySelector(`[data-answer="${CSS.escape(ev.target.dataset.answerInput)}"]`)?.click();
    }
  });

  window.addEventListener('message', (ev) => {
    const msg = ev.data || {};
    if (msg.type === 'graph') {
      if (graph) graph.setData(msg.graph);
    } else if (msg.type === 'status') {
      status = msg.status;
      render();
    } else if (msg.type === 'tab') {
      showTab(msg.tab);
    }
  });

  // Grafo: renderizador compartido con el CLI (media/graph.js). Los clics
  // abren el archivo en el editor; los datos llegan con el mensaje `graph`.
  graph = window.AdocGraph
    ? window.AdocGraph.mount($('graph'), {
        t: {
          search: t('web.graph.search'),
          fit: t('web.graph.fit'),
          refresh: t('web.graph.refresh'),
          zoom_in: t('web.graph.zoomIn'),
          zoom_out: t('web.graph.zoomOut'),
          stats: t('web.graph.stats'),
          stale: t('web.stale'),
          legend: t('web.graph.legend'),
          close: t('web.graph.close'),
          path: t('web.graph.path'),
          empty: t('web.graph.empty'),
          edge_relation: t('web.graph.edgeRelation'),
          edge_evidence: t('web.graph.edgeEvidence'),
          // Etiquetas de tipo (`kind.*`): mismas claves que el resto del dashboard.
          ...Object.fromEntries(
            ['overview', 'module', 'feature', 'finding', 'test-report', 'spec-report', 'glossary', 'decision', 'memory', 'faq', 'moc', 'code'].map(
              (k) => [`kind.${k}`, t(`kind.${k}`)],
            ),
          ),
        },
        onOpen: (node) => vscode.postMessage({ type: 'open', file: node.file, line: node.line }),
        onRefresh: () => vscode.postMessage({ type: 'refresh' }),
      })
    : null;
  const saved = vscode.getState();
  showTab((saved && saved.tab) || 'summary');
  vscode.postMessage({ type: 'dashboard-ready' });
  // Pide los datos del grafo (antes lo hacía media/graph.js al arrancar).
  if (graph) vscode.postMessage({ type: 'ready' });
})();
