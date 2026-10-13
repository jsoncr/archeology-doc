// Sidebar regenerado por `adoc site sync|dev|build` (no editar).
import sidebar from './sidebar.generated.json'

// Generado por archeology-doc — edítalo libremente, no se regenera si existe.
// Sin `import ... from 'vitepress'` en tiempo de ejecución: así funciona también
// con VitePress compartido (`adoc site setup`) o global (`npm i -g vitepress`).
export default {
  title: '{{ project_name }}',
  description: 'Documentación generada con archeology-doc',
  lang: 'es',
  cleanUrls: true,
  themeConfig: {
    sidebar,
    outline: { label: 'En esta página' },
    docFooter: { prev: 'Anterior', next: 'Siguiente' },
    lastUpdated: { text: 'Última actualización' },
    returnToTopLabel: 'Volver arriba',
    sidebarMenuLabel: 'Menú',
    darkModeSwitchLabel: 'Apariencia',
    lightModeSwitchTitle: 'Cambiar a tema claro',
    darkModeSwitchTitle: 'Cambiar a tema oscuro',
    notFound: {
      title: 'PÁGINA NO ENCONTRADA',
      quote: 'La página que buscas no existe o se ha movido.',
      linkText: 'Ir al inicio',
    },
    search: {
      provider: 'local',
      options: {
        translations: {
          button: { buttonText: 'Buscar', buttonAriaLabel: 'Buscar' },
          modal: {
            noResultsText: 'Sin resultados para',
            resetButtonTitle: 'Borrar la búsqueda',
            displayDetails: 'Mostrar detalles',
            footer: { selectText: 'seleccionar', navigateText: 'navegar', closeText: 'cerrar' },
          },
        },
      },
    },
  },
} satisfies import('vitepress').UserConfig
