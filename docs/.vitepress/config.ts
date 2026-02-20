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

  locales: {
    root: {
      label: 'English',
      lang: 'en'
    },
    zh: {
      label: '简体中文',
      lang: 'zh-CN',
      link: '/zh/',
      description: '多智能体协作内核',
      themeConfig: {
        nav: [
          { text: '指南', link: '/zh/guide/' },
          { text: '参考', link: '/zh/reference/architecture' },
          { text: 'SDK', link: '/zh/sdk/' },
          { text: '发布', link: '/release/' }
        ],

        sidebar: {
          '/zh/guide/': [
            {
              text: '用户指南',
              items: [
                { text: '简介', link: '/zh/guide/' }
              ]
            },
            {
              text: '快速开始',
              collapsed: false,
              items: [
                { text: '概览', link: '/zh/guide/getting-started/' },
                { text: 'Web UI 快速上手', link: '/zh/guide/getting-started/web' },
                { text: 'CLI 快速上手', link: '/zh/guide/getting-started/cli' },
                { text: 'Docker 部署', link: '/zh/guide/getting-started/docker' }
              ]
            },
            {
              text: '核心指南',
              items: [
                { text: '使用场景', link: '/zh/guide/use-cases' },
                { text: '工作流', link: '/zh/guide/workflows' },
                { text: '运维手册', link: '/zh/guide/operations' },
                { text: 'Web UI', link: '/zh/guide/web-ui' },
                { text: '最佳实践', link: '/zh/guide/best-practices' },
                { text: '常见问题', link: '/zh/guide/faq' }
              ]
            },
            {
              text: 'IM 桥接',
              collapsed: false,
              items: [
                { text: '概览', link: '/zh/guide/im-bridge/' },
                { text: 'Telegram', link: '/zh/guide/im-bridge/telegram' },
                { text: 'Slack', link: '/zh/guide/im-bridge/slack' },
                { text: 'Discord', link: '/zh/guide/im-bridge/discord' },
                { text: '飞书', link: '/zh/guide/im-bridge/feishu' },
                { text: '钉钉', link: '/zh/guide/im-bridge/dingtalk' }
              ]
            }
          ],
          '/zh/reference/': [
            {
              text: '参考',
              items: [
                { text: '产品定位', link: '/zh/reference/positioning' },
                { text: '架构', link: '/zh/reference/architecture' },
                { text: '功能特性', link: '/zh/reference/features' },
                { text: 'CLI', link: '/zh/reference/cli' }
              ]
            }
          ],
          '/zh/sdk/': [
            {
              text: 'SDK',
              items: [
                { text: '概览', link: '/zh/sdk/' },
                { text: '客户端 SDK', link: '/zh/sdk/CLIENT_SDK' }
              ]
            }
          ]
        },

        outline: {
          label: '本页目录'
        },

        lastUpdated: {
          text: '最后更新'
        },

        docFooter: {
          prev: '上一页',
          next: '下一页'
        },

        darkModeSwitchLabel: '深色模式',
        returnToTopLabel: '返回顶部',
        sidebarMenuLabel: '菜单',

        footer: {
          message: '基于 Apache-2.0 许可发布',
          copyright: 'Copyright 2024-present CCCC Contributors'
        }
      }
    }
  },

  themeConfig: {
    logo: '/logo.svg',

    nav: [
      { text: 'Guide', link: '/guide/' },
      { text: 'Reference', link: '/reference/architecture' },
      { text: 'SDK', link: '/sdk/' },
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
      '/sdk/': [
        {
          text: 'SDK',
          items: [
            { text: 'Overview', link: '/sdk/' },
            { text: 'Client SDK', link: '/sdk/CLIENT_SDK' }
          ]
        }
      ],
      '/release/': [
        {
          text: 'Release Hub',
          items: [
            { text: 'Overview', link: '/release/' },
            { text: 'v0.4.0 Release Notes', link: '/release/v0.4.0_release_notes' },
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
