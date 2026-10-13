---
task_id: {{ task_id }}
type: {{ type }}
module: {{ module }}
status: pending
depends_on: [{{ depends_on }}]
---
# Objective
{{ objective }}

# Read
{{ read }}

# Produce
{{ produce }}

# Rules
- Cite every statement with path:line.
- Mark confidence: low if you cannot verify it in the code.
- Do not modify files outside the output paths (never openspec/specs/).
- Respect the managed regions `<!-- archeology-doc:begin ... -->`.
- Write the notes in English.
- When done, add an entry in knowledge/memory/sessions/.
