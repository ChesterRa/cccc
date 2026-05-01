import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'CCCC',
  description: 'Multi-Agent Collaboration Kernel',

  // GitHub Pages base path
  base: '/cccc/',

  // Keep local-only planning/archive notes out of the published docs build.
  srcExclude: [
    '_archive_local/**',
    'ITERATION_PLAN.md',
    'plan/**',
    'review/**',
    'superpowers/**',
    'voice-secretary/**'
  ],

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
            { text: 'ChatGPT Web Model Runtime', link: '/guide/web-model-runtime' },
            { text: 'Group Space + NotebookLM', link: '/guide/group-space-notebooklm' },
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
            { text: 'DingTalk', link: '/guide/im-bridge/dingtalk' },
            { text: 'WeCom', link: '/guide/im-bridge/wecom' }
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
            { text: 'v0.4.12 Release Notes', link: '/release/v0.4.12_release_notes' },
            { text: 'v0.4.11 Release Notes', link: '/release/v0.4.11_release_notes' },
            { text: 'v0.4.10 Release Notes', link: '/release/v0.4.10_release_notes' },
            { text: 'v0.4.9 Release Notes', link: '/release/v0.4.9_release_notes' },
            { text: 'v0.4.8 Release Notes', link: '/release/v0.4.8_release_notes' },
            { text: 'v0.4.7 Release Notes', link: '/release/v0.4.7_release_notes' },
            { text: 'v0.4.6 Release Notes', link: '/release/v0.4.6_release_notes' },
            { text: 'v0.4.5 Release Notes', link: '/release/v0.4.5_release_notes' },
            { text: 'v0.4.4 Release Notes', link: '/release/v0.4.4_release_notes' },
            { text: 'v0.4.3 Release Notes', link: '/release/v0.4.3_release_notes' },
            { text: 'v0.4.2 Release Notes', link: '/release/v0.4.2_release_notes' },
            { text: 'v0.4.1 Release Notes', link: '/release/v0.4.1_release_notes' },
            { text: 'v0.4.0 Release Notes', link: '/release/v0.4.0_release_notes' }
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
