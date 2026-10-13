---
task_id: {{ task_id }}
type: {{ type }}
module: {{ module }}
status: pending
depends_on: [{{ depends_on }}]
---
# Objetivo
{{ objective }}

# Lee
{{ read }}

# Produce
{{ produce }}

# Reglas
- Cita cada afirmación con ruta:línea.
- Marca confidence: low si no puedes verificarlo en el código.
- No modifiques archivos fuera de las rutas de salida (nunca openspec/specs/).
- Respeta las regiones gestionadas `<!-- archeology-doc:begin ... -->`.
- Escribe las notas en español.
- Al terminar, agrega una entrada en knowledge/memory/sessions/.
