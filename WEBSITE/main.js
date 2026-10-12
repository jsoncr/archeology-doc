// Copiar bloques de código, pestañas de instalación y tema claro/oscuro.

(function () {
  'use strict';

  // Botón "Copiar" en cada bloque marcado con data-copy (sin prompts ni comentarios).
  document.querySelectorAll('[data-copy]').forEach((block) => {
    const button = document.createElement('button');
    button.className = 'copy';
    button.type = 'button';
    button.textContent = 'Copiar';
    button.addEventListener('click', async () => {
      const code = block.querySelector('code').cloneNode(true);
      code.querySelectorAll('.prompt').forEach((p) => p.remove());
      const text = code.textContent
        .split('\n')
        .map((l) => l.replace(/\s+#.*$/, '').replace(/^#.*$/, ''))
        .join('\n')
        .replace(/\n{3,}/g, '\n\n')
        .trim();
      try {
        await navigator.clipboard.writeText(text);
        button.textContent = 'Copiado ✓';
      } catch {
        button.textContent = 'Selecciona y copia';
      }
      setTimeout(() => (button.textContent = 'Copiar'), 1600);
    });
    block.appendChild(button);
  });

  // Pestañas (patrón ARIA tablist: clic y flechas izquierda/derecha).
  const tabs = Array.from(document.querySelectorAll('.tab'));
  const select = (tab) => {
    tabs.forEach((t) => {
      const on = t === tab;
      t.classList.toggle('active', on);
      t.setAttribute('aria-selected', String(on));
      t.tabIndex = on ? 0 : -1;
    });
    document.querySelectorAll('.panel').forEach((p) => p.classList.toggle('active', p.dataset.panel === tab.dataset.tab));
  };
  tabs.forEach((tab, i) => {
    tab.addEventListener('click', () => select(tab));
    tab.addEventListener('keydown', (ev) => {
      const step = ev.key === 'ArrowRight' ? 1 : ev.key === 'ArrowLeft' ? -1 : 0;
      if (!step) return;
      const next = tabs[(i + step + tabs.length) % tabs.length];
      select(next);
      next.focus();
    });
  });

  // Instalación según el sistema operativo.
  const OS_LABEL = { macos: 'macOS', linux: 'Linux', windows: 'Windows' };
  const detectOs = () => {
    const platform = ((navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '').toLowerCase();
    const ua = navigator.userAgent.toLowerCase();
    if (platform.includes('win') || ua.includes('windows')) return 'windows';
    if (platform.includes('mac') || ua.includes('mac os')) return 'macos';
    if (platform.includes('linux') || ua.includes('linux') || ua.includes('x11')) return 'linux';
    return null;
  };
  const os = detectOs();
  if (os) {
    const tab = tabs.find((t) => t.dataset.tab === os);
    if (tab) select(tab);
    const detected = document.getElementById('os-detected');
    detected.textContent = `Detectado: ${OS_LABEL[os]}`;
    detected.hidden = false;
    // El comando del inicio también se adapta.
    if (os === 'windows') {
      document.querySelectorAll('[data-oneliner]').forEach((code) => {
        code.innerHTML = '<span class="prompt">&gt;</span> irm <span data-base></span>install.ps1 | iex';
      });
    }
    const hint = document.getElementById('hero-os');
    hint.textContent = `Instalación para ${OS_LABEL[os]} · otras opciones en «Instalar»`;
    hint.hidden = false;
  }

  // URL pública de /download: la del sitio donde se abre la página.
  if (location.protocol === 'http:' || location.protocol === 'https:') {
    const base = new URL('download/', location.href).href;
    document.querySelectorAll('[data-base]').forEach((el) => (el.textContent = base));
  } else {
    document.querySelectorAll('[data-base]').forEach((el) => (el.textContent = 'https://tu-sitio/download/'));
  }

  // Tabla de descargas y tamaños desde download/latest.json (lo genera WEBSITE/dist.sh).
  const PLATFORM = {
    'aarch64-apple-darwin': 'macOS · Apple Silicon',
    'x86_64-apple-darwin': 'macOS · Intel',
    'x86_64-unknown-linux-gnu': 'Linux · x86_64',
    'aarch64-unknown-linux-gnu': 'Linux · ARM64',
    'x86_64-pc-windows-gnu': 'Windows · x64',
  };
  const platformOf = (file) => {
    const target = Object.keys(PLATFORM).find((t) => file.includes(t));
    if (target) return PLATFORM[target];
    if (file.endsWith('.vsix')) return 'Extensión de VS Code';
    if (file === 'install.sh') return 'Instalador macOS / Linux';
    if (file === 'install.ps1') return 'Instalador Windows';
    return '';
  };
  const human = (n) => (n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`);
  const cell = (text, cls) => {
    const td = document.createElement('td');
    if (cls) td.className = cls;
    td.textContent = text;
    return td;
  };
  fetch('download/latest.json', { cache: 'no-cache' })
    .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
    .then((manifest) => {
      document.getElementById('dl-version').textContent = `v${manifest.version}`;
      const body = document.getElementById('dl-table');
      body.textContent = '';
      const byFile = {};
      manifest.files.forEach((f) => {
        byFile[f.file] = f;
        const tr = document.createElement('tr');
        const name = document.createElement('td');
        const link = document.createElement('a');
        link.href = `download/${encodeURIComponent(f.file)}`;
        link.textContent = f.file;
        link.setAttribute('download', '');
        name.appendChild(link);
        const hash = cell(f.sha256, 'hash');
        hash.title = f.sha256;
        tr.append(name, cell(platformOf(f.file)), cell(human(f.size)), hash);
        body.appendChild(tr);
      });
      // Tamaño en los botones; los que no tienen binario publicado se desactivan.
      document.querySelectorAll('a.dl').forEach((a) => {
        const f = byFile[a.dataset.file];
        if (f) {
          a.querySelector('small').textContent = human(f.size);
        } else {
          a.classList.add('disabled');
          a.removeAttribute('href');
          a.setAttribute('aria-disabled', 'true');
          a.querySelector('small').textContent = 'no disponible';
        }
      });
    })
    .catch(() => {
      /* Sin manifiesto (abierto como archivo o aún sin publicar): se deja el texto de ayuda. */
    });

  // Tema: sigue al sistema; el botón alterna y se recuerda.
  const root = document.documentElement;
  try {
    const saved = localStorage.getItem('theme');
    if (saved) root.dataset.theme = saved;
  } catch {
    /* almacenamiento no disponible: se usa el tema del sistema */
  }
  document.getElementById('theme').addEventListener('click', () => {
    const dark = root.dataset.theme
      ? root.dataset.theme === 'dark'
      : window.matchMedia('(prefers-color-scheme: dark)').matches;
    root.dataset.theme = dark ? 'light' : 'dark';
    try {
      localStorage.setItem('theme', root.dataset.theme);
    } catch {
      /* sin persistencia */
    }
  });
})();
