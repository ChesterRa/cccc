<script setup>
import { withBase } from 'vitepress'

const releaseModules = import.meta.glob('./v*_release_notes.md')

function compareSemverDesc(a, b) {
  const parse = (version) => version.split('.').map((part) => Number(part) || 0)
  const left = parse(a)
  const right = parse(b)
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const diff = (right[index] || 0) - (left[index] || 0)
    if (diff !== 0) return diff
  }
  return 0
}

const releases = Object.keys(releaseModules)
  .map((fileName) => /^\.\/v(\d+\.\d+\.\d+)_release_notes\.md$/.exec(fileName))
  .filter(Boolean)
  .map((match) => ({
    version: match[1],
    text: `v${match[1]} Release Notes`,
    link: `/release/v${match[1]}_release_notes.html`,
  }))
  .sort((a, b) => compareSemverDesc(a.version, b.version))

const latestReleases = releases.slice(0, 3)
</script>

# Release Hub

Use this page to find published release notes. New release note files matching `v*_release_notes.md` are listed automatically.

## Latest

<ul>
  <li v-for="release in latestReleases" :key="release.version">
    <a :href="withBase(release.link)">{{ release.text }}</a>
  </li>
</ul>

## All Releases

<ul>
  <li v-for="release in releases" :key="release.version">
    <a :href="withBase(release.link)">{{ release.text }}</a>
  </li>
</ul>

## Related Docs

- [Getting Started](/guide/getting-started/)
- [Operations Runbook](/guide/operations)
- [Features Reference](/reference/features)
