// Sidebar regenerado por `adoc site sync|dev|build` (no editar).
import sidebar from './sidebar.generated.json'

// Generado por archeology-doc — edítalo libremente, no se regenera si existe.
// Sin `import ... from 'vitepress'` en tiempo de ejecución: así funciona también
// con VitePress compartido (`adoc site setup`) o global (`npm i -g vitepress`).
export default {
  title: '{{ project_name }}',
  description: 'Documentación generada con archeology-doc',
  lang: '{{ language }}',
  cleanUrls: true,
  themeConfig: {
    sidebar,
    search: { provider: 'local' },
  },
} satisfies import('vitepress').UserConfig
