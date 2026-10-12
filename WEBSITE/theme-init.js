// Se carga en <head>: aplica el tema guardado antes del primer pintado.
try {
  const saved = localStorage.getItem('theme');
  if (saved) document.documentElement.dataset.theme = saved;
} catch {
  /* almacenamiento no disponible: se usa el tema del sistema */
}
