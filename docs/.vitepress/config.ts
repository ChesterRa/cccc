import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'CCCC',
  description: 'Multi-Agent Collaboration Kernel',

  // GitHub Pages base path
  base: '/cccc/',

  // Ignore dead links in legacy vnext docs
  ignoreDeadLinks: [
    /archive/,
    /localhost:8848\/ui\/index/
  ],

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/cccc/logo.svg' }]
  ],

  themeConfig: {
    logo: '/logo.svg',

    nav: [
      { text: 'Guide', link: '/guide/' },
      { text: 'Reference', link: '/reference/architecture' },
      { text: 'Release', link: '/release/' }
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'User Guide',
          items: [
            { text: 'Introduction', link: '/guide/' }
          ]
        },
        {
          text: 'Getting Started',
          collapsed: false,
          items: [
            { text: 'Overview', link: '/guide/getting-started/' },
            { text: 'Web UI Quick Start', link: '/guide/getting-started/web' },
            { text: 'CLI Quick Start', link: '/guide/getting-started/cli' },
            { text: 'Docker Deployment', link: '/guide/getting-started/docker' }
          ]
        },
        {
          text: 'Core Guides',
          items: [
            { text: 'Use Cases', link: '/guide/use-cases' },
            { text: 'Workflows', link: '/guide/workflows' },
            { text: 'Operations Runbook', link: '/guide/operations' },
            { text: 'Web UI', link: '/guide/web-ui' },
            { text: 'Best Practices', link: '/guide/best-practices' },
            { text: 'FAQ', link: '/guide/faq' }
          ]
        },
        {
          text: 'IM Bridge',
          collapsed: false,
          items: [
            { text: 'Overview', link: '/guide/im-bridge/' },
            { text: 'Telegram', link: '/guide/im-bridge/telegram' },
            { text: 'Slack', link: '/guide/im-bridge/slack' },
            { text: 'Discord', link: '/guide/im-bridge/discord' },
            { text: 'Feishu', link: '/guide/im-bridge/feishu' },
            { text: 'DingTalk', link: '/guide/im-bridge/dingtalk' }
          ]
        }
      ],
      '/reference/': [
        {
          text: 'Reference',
          items: [
            { text: 'Positioning', link: '/reference/positioning' },
            { text: 'Architecture', link: '/reference/architecture' },
            { text: 'Features', link: '/reference/features' },
            { text: 'CLI', link: '/reference/cli' }
          ]
        }
      ],
      '/release/': [
        {
          text: 'Release Program',
          items: [
            { text: 'Overview', link: '/release/' },
            { text: 'Technical Debt Board (0.4.0)', link: '/release/DEBT_BOARD_0_4_0' },
            { text: 'RC19 Release Board', link: '/release/RC19_RELEASE_BOARD' },
            { text: 'Audit Method', link: '/release/RC19_AUDIT_METHOD' },
            { text: 'Owner Map', link: '/release/RC19_OWNER_MAP' },
            { text: 'Findings Register', link: '/release/RC19_FINDINGS_REGISTER' },
            { text: 'Quality Gates', link: '/release/RC19_GATES' },
            { text: 'Execution Checklist', link: '/release/RC19_EXECUTION_CHECKLIST' },
            { text: 'Contract Gap Baseline', link: '/release/rc19_contract_gap' },
            { text: 'Core Findings (R3)', link: '/release/rc19_core_findings' },
            { text: 'Port Parity Findings (R4)', link: '/release/rc19_port_parity' },
            { text: 'Rehearsal Report (R8)', link: '/release/rc19_rehearsal_report' },
            { text: 'File Matrix', link: '/release/rc19_file_matrix' }
          ]
        }
      ]
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/ChesterRa/cccc' }
    ],

    footer: {
      message: 'Released under the Apache-2.0 License.',
      copyright: 'Copyright 2024-present CCCC Contributors'
    },

    search: {
      provider: 'local'
    }
  }
})
