// Textos del webview en el idioma de la extensión (`archeologyDoc.language`).
// La extensión deja el diccionario en `<body data-i18n="{lang, strings}">`
// (no hay scripts inline: la CSP exige nonce). Se carga antes que graph.js y
// dashboard.js, que usan `AdocI18n.t(clave, parámetros)`.

(function (root) {
  'use strict';
  let data = { lang: 'en', strings: {} };
  try {
    const raw = document.body && document.body.dataset.i18n;
    if (raw) data = JSON.parse(raw);
  } catch (err) {
    console.error('archeology-doc: i18n', err);
  }
  const strings = data.strings || {};
  const lang = data.lang === 'es' ? 'es' : 'en';

  /**
   * Texto de `key` con los marcadores `{nombre}` sustituidos por `params`.
   * Los textos son de la extensión (pueden llevar HTML); los parámetros que
   * vengan de agentes o personas se escapan antes de pasarlos.
   */
  function t(key, params) {
    const text = strings[key];
    if (text === undefined) return key;
    return text.replace(/\{(\w+)\}/g, (m, name) => (params && params[name] !== undefined ? String(params[name]) : m));
  }

  root.AdocI18n = { t, lang };
})(window);
