---
id: find-{{ slug }}
type: finding
title: {{ title }}
status: draft
confidence: {{ confidence | default("medium") }}
kind: {{ kind }}
severity: {{ severity }}
state: open
sources:
{{ sources }}
related: []
generated_by: {{ agent }}
updated: {{ date }}
locked: false
---

# {{ title }}

## Descripción

{{ description }}

## Evidencia

{{ evidence }}

## Impacto

## Recomendación
