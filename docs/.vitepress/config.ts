import { readdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig, type DefaultTheme, HeadConfig } from 'vitepress'

const DOCS_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..')

function compareSemverDesc(a: string, b: string): number {
  const parse = (version: string) => version.split('.').map((part) => Number(part) || 0)
  const left = parse(a)
  const right = parse(b)
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const diff = (right[index] || 0) - (left[index] || 0)
    if (diff !== 0) return diff
  }
  return 0
}

function buildReleaseSidebarItems(): DefaultTheme.SidebarItem[] {
  const releaseDir = resolve(DOCS_DIR, 'release')
  const releaseItems = readdirSync(releaseDir)
    .map((fileName) => /^v(\d+\.\d+\.\d+)_release_notes\.md$/.exec(fileName))
    .filter((match): match is RegExpExecArray => !!match)
    .map((match) => ({
      version: match[1],
      text: `v${match[1]} Release Notes`,
      link: `/release/v${match[1]}_release_notes`,
    }))
    .sort((a, b) => compareSemverDesc(a.version, b.version))
    .map(({ text, link }) => ({ text, link }))

  return [
    { text: 'Overview', link: '/release/' },
    ...releaseItems,
  ]
}

// Privacy-friendly visit counting (no cookies, no personal data).
// To enable: create a site at https://www.goatcounter.com (free for open source),
// then set the site code here, e.g. 'cccc'. Leave empty to ship no analytics at all.
const GOATCOUNTER_CODE = ''

const analyticsHead: HeadConfig[] = GOATCOUNTER_CODE
  ? [[
      'script',
      {
        'data-goatcounter': `https://${GOATCOUNTER_CODE}.goatcounter.com/count`,
        async: '',
        src: 'https://gc.zgo.at/count.js'
      }
    ]]
  : []

export default defineConfig({
  title: 'CCCC',
  description: 'Multi-Agent Collaboration Kernel',

  // GitHub Pages base path
  base: '/cccc/',

  // Keep local-only planning/archive notes out of the published docs build.
  srcExclude: [
    '_archive_local/**',
    'ITERATION_PLAN.md',
    'guide/agent-framework-coevolution-prd.md',
    'plan/**',
    'review/**',
    'superpowers/**',
    'vnext/**',
    'voice-secretary/**'
  ],

  // Ignore legacy local-only links in excluded docs.
  ignoreDeadLinks: [
    /archive/,
    /localhost:8848\/ui\/index/
  ],

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/cccc/logo.svg' }],
    ...analyticsHead
  ],

  themeConfig: {
    logo: '/logo.svg',

    nav: [
      { text: 'Guide', link: '/guide/' },
      { text: 'Reference', link: '/reference/architecture' },
      { text: 'Standards', link: '/standards/' },
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
            { text: 'Supported Runtimes', link: '/guide/runtimes' },
            { text: 'Group Bridge', link: '/guide/group-bridge' },
            { text: 'ChatGPT Web Model Runtime', link: '/guide/web-model-runtime' },
            { text: 'Group Space + NotebookLM', link: '/guide/group-space-notebooklm' },
            { text: 'Capability Allowlist', link: '/guide/capability-allowlist' },
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
      '/standards/': [
        {
          text: 'Standards',
          items: [
            { text: 'Overview', link: '/standards/' },
            { text: 'CCCS v1', link: '/standards/CCCS_V1' },
            { text: 'Daemon IPC v1', link: '/standards/CCCC_DAEMON_IPC_V1' },
            { text: 'Context Ops v1', link: '/standards/CCCC_CONTEXT_OPS_V1' }
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
          items: buildReleaseSidebarItems()
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
