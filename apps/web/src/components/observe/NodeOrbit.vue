<script setup lang="ts">
import { useElementSize } from '@vueuse/core'
import type { RouteLocationRaw } from 'vue-router'

import Icon from '~/components/Icon.vue'
import type { TreeNode } from '~/generated'
import { countPresences, presenceColor, presenceLabels, typeIcons, type Presence } from '~/lib/monitor-status'
import { innerItems, itemKey, itemName, itemsOf, monitorsIn, worstOf, type MonitorTree, type ScopeItem } from '~/lib/monitor-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * A node as a small solar system: the node in the middle with the share of healthy monitors,
 * what sits directly in it on the inner ring, and what sits in those on an outer fan. Rings show
 * the status mix below each item, links run in the worst status, and only problems pulse and get
 * a label on the fan. Every item links to its dashboard or monitor page. The ring grows with
 * what it holds, up to 36 items, and each fan holds as many leaves as its arc has room for; the
 * wall shows a node with more in full. A satellite's words sit in a twin
 * link hidden from assistive technology, so they click like the circle without a second name.
 */
const props = defineProps<{ tree: MonitorTree; node: TreeNode }>()

const { t } = useI18n()
const router = useRouter()
const id = useId()
const box = useTemplateRef<HTMLElement>('box')
const size = useElementSize(box)

/**
 * The picture keeps its own scale: a larger box shows more empty space around it, a smaller one
 * shrinks it to fit. A crowded node needs a wider picture.
 */
const view = computed(() => {
  const width = Math.max(W, 2 * (layout.value.outer + 56), size.width.value)
  const height = Math.max(H, 2 * (layout.value.outer + 52), size.height.value)
  return `${round(CX - width / 2)} ${round(CY - height / 2)} ${round(width)} ${round(height)}`
})

const W = 640
const H = 560
/** The orb's radius; its rings and the inner disc follow it. */
const ORB = 62
const CX = 320
const CY = 280
/** Items on the inner ring before the rest are only counted. */
const RING = 36
/** Arc length each leaf of a fan needs at the outer radius. */
const LEAF_GAP = 16
/** Arc length each satellite gets on the inner ring, so labels do not touch. */
const SPACING = 112
const SEGMENTS: Presence[] = ['up', 'maintenance', 'pending', 'down', 'unknown', 'paused']

const linkTo = (item: ScopeItem): RouteLocationRaw =>
  item.kind === 'node'
    ? { name: 'node', params: { nodeId: item.node.id } }
    : { name: 'node-monitor', params: { nodeId: item.monitor.node, monitorId: item.monitor.id } }

const point = (radius: number, angle: number) => ({ x: CX + radius * Math.cos(angle), y: CY + radius * Math.sin(angle) })
const round = (value: number) => Math.round(value * 10) / 10

function arc(x: number, y: number, r: number, from: number, to: number) {
  const a = { x: x + r * Math.cos(from), y: y + r * Math.sin(from) }
  const b = { x: x + r * Math.cos(to), y: y + r * Math.sin(to) }
  return `M${round(a.x)} ${round(a.y)} A${r} ${r} 0 ${to - from > Math.PI ? 1 : 0} 1 ${round(b.x)} ${round(b.y)}`
}

/** A ring split by status share; one status fills it whole. */
function segments(x: number, y: number, r: number, counts: Record<Presence, number>) {
  const total = SEGMENTS.reduce((sum, presence) => sum + counts[presence], 0)
  let at = -Math.PI / 2
  return SEGMENTS.flatMap((presence) => {
    const share = counts[presence]
    if (share === 0) return []
    const span = (share / total) * Math.PI * 2
    const start = at
    at += span
    return [{ presence, whole: share === total, d: share === total ? '' : arc(x, y, r, start + 0.07, start + span - 0.07) }]
  })
}

/** The second label line: what needs attention, else how it is doing. */
function subline(counts: Record<Presence, number>, total: number, latency: number | undefined, monitor: boolean) {
  if (monitor && total === 1 && counts.up === 0) {
    const presence = SEGMENTS.find((entry) => counts[entry] > 0) ?? 'unknown'
    return { text: t(presenceLabels[presence]), tone: presence === 'down' ? 'fill-status-down' : 'fill-muted' }
  }
  if (counts.down > 0) return { text: t('observe.orbit.down', { count: counts.down }), tone: 'fill-status-down' }
  if (counts.pending > 0) return { text: t('observe.orbit.pending', { count: counts.pending }), tone: 'fill-status-pending' }
  if (counts.maintenance > 0 && counts.up === 0) return { text: t('monitors.status.maintenance'), tone: 'fill-muted' }
  if (monitor) return { text: latency === undefined ? '' : `${latency} ms`, tone: 'fill-muted' }
  return { text: t('observe.orbit.up', { count: total }), tone: 'fill-muted' }
}

const layout = computed(() => {
  const items = itemsOf(props.tree, props.node.id)
  const shown = items.slice(0, RING)
  const fanned = shown.some((item) => innerItems(props.tree, item).length > 0)
  const inner = Math.max(fanned ? 150 : 185, Math.round((shown.length * SPACING) / (Math.PI * 2)))
  const outer = inner + 78
  const step = (Math.PI * 2) / Math.max(shown.length, 1)

  const satellites = shown.map((item, at) => {
    const angle = -Math.PI / 2 + at * step
    const { x, y } = point(inner, angle)
    const monitors = monitorsIn(props.tree, item)
    const counts = countPresences(monitors)
    const presence = worstOf(monitors)
    const radius = item.kind === 'node' ? 22 : 15
    const inside = innerItems(props.tree, item)
    const spread = Math.min(step * 0.7, 1.35)
    const fan = inside.slice(0, Math.max(1, Math.floor((spread * outer) / LEAF_GAP) + 1))
    const cos = Math.cos(angle)
    const sin = Math.sin(angle)
    const leaves = fan.map((leaf, index) => {
      const bearing = angle + (fan.length === 1 ? 0 : (index / (fan.length - 1) - 0.5) * spread)
      const end = point(outer, bearing)
      const bend = point(inner + 46, bearing)
      const leafPresence = worstOf(monitorsIn(props.tree, leaf))
      const labelled = leaf.kind === 'node' || leafPresence === 'down' || leafPresence === 'pending'
      const label = point(outer + (leaf.kind === 'node' ? 16 : 13), bearing)
      const lean = Math.cos(bearing)
      return {
        key: itemKey(leaf),
        name: itemName(leaf),
        to: linkTo(leaf),
        x: round(end.x),
        y: round(end.y),
        node: leaf.kind === 'node',
        presence: leafPresence,
        path: `M${round(x + (radius + 4) * cos)} ${round(y + (radius + 4) * sin)} Q${round(bend.x)} ${round(bend.y)} ${round(end.x)} ${round(end.y)}`,
        label: labelled ? { x: round(label.x), y: round(label.y + 4), anchor: lean > 0.3 ? 'start' : lean < -0.3 ? 'end' : 'middle' } : null,
      }
    })
    const more = inside.length - fan.length
    const morePoint = point(outer + 30, angle)
    return {
      key: itemKey(item),
      name: itemName(item),
      to: linkTo(item),
      x: round(x),
      y: round(y),
      radius,
      presence,
      total: monitors.length,
      icon: item.kind === 'monitor' ? typeIcons[item.monitor.type] : null,
      sub: subline(counts, monitors.length, item.kind === 'monitor' ? item.monitor.latency_ms : undefined, item.kind === 'monitor'),
      ring: segments(x, y, radius + 4, counts),
      link: {
        x1: round(CX + (ORB + 6) * cos),
        y1: round(CY + (ORB + 6) * sin),
        x2: round(x - (radius + 8) * cos),
        y2: round(y - (radius + 8) * sin),
      },
      leaves,
      more: more > 0 ? { count: more, x: round(morePoint.x), y: round(morePoint.y + 4) } : null,
    }
  })

  const all = props.tree.below(props.node.id)
  const counts = countPresences(all)
  const active = all.length - counts.paused
  const healthy = counts.up + counts.maintenance
  const share = active > 0 ? healthy / active : 1
  return {
    satellites,
    fanned,
    inner,
    outer,
    hidden: items.length - shown.length,
    healthy,
    active,
    presence: worstOf(all),
    progress: share >= 1 ? null : arc(CX, CY, ORB - 12, -Math.PI / 2, -Math.PI / 2 + share * Math.PI * 2),
  }
})

/** The orb takes the worst status that needs someone; otherwise it reads healthy. */
const orb = computed(() => {
  const presence = layout.value.presence
  return presenceColor[presence === 'down' || presence === 'pending' ? presence : 'up']
})

/** Faint spokes and the turning tick ring, just outside the fan. */
const spokes = computed(() =>
  Array.from({ length: 24 }, (_, at) => {
    const angle = (at / 24) * Math.PI * 2
    const from = point(ORB + 22, angle)
    const to = point(layout.value.outer + 28, angle)
    return { x1: round(from.x), y1: round(from.y), x2: round(to.x), y2: round(to.y) }
  })
)
const ticks = computed(() =>
  Array.from({ length: 144 }, (_, at) => {
    const angle = (at / 144) * Math.PI * 2
    const from = point(layout.value.outer + 33, angle)
    const to = point(layout.value.outer + (at % 12 === 0 ? 44 : 38), angle)
    return { x1: round(from.x), y1: round(from.y), x2: round(to.x), y2: round(to.y) }
  })
)

const href = (to: RouteLocationRaw) => router.resolve(to).href
/** Plain clicks stay in the app; modified clicks open a tab as any link does. */
const follow = (event: MouseEvent, to: RouteLocationRaw) => {
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return
  event.preventDefault()
  void router.push(to)
}
const label = (name: string, presence: Presence) => `${name}, ${t(presenceLabels[presence])}`
</script>

<template>
  <div
    ref="box"
    class="relative min-h-0 min-w-0"
  >
    <svg
      :viewBox="view"
      role="group"
      :aria-label="t('observe.orbit.label', { name: node.name ?? '' })"
      class="absolute inset-0 size-full motion-safe:animate-in motion-safe:fade-in motion-safe:zoom-in-95"
    >
      <defs>
        <radialGradient
          :id="`${id}-orb`"
          cx="38%"
          cy="32%"
          r="75%"
        >
          <stop
            offset="0"
            :style="{ stopColor: `color-mix(in oklch, ${orb} 45%, white)` }"
          />
          <stop
            offset="0.55"
            :style="{ stopColor: orb }"
          />
          <stop
            offset="1"
            :style="{ stopColor: `color-mix(in oklch, ${orb} 45%, black)` }"
          />
        </radialGradient>
        <filter
          :id="`${id}-glow`"
          x="-50%"
          y="-50%"
          width="200%"
          height="200%"
        >
          <feGaussianBlur stdDeviation="20" />
        </filter>
      </defs>

      <g
        class="fill-none stroke-faint/40"
        aria-hidden="true"
      >
        <circle
          :cx="CX"
          :cy="CY"
          :r="layout.inner"
        />
        <circle
          :cx="CX"
          :cy="CY"
          r="104"
          stroke-dasharray="2 6"
        />
        <circle
          v-if="layout.fanned"
          :cx="CX"
          :cy="CY"
          :r="layout.outer"
          stroke-dasharray="2 6"
        />
        <line
          v-for="(spoke, at) in spokes"
          :key="at"
          v-bind="spoke"
          class="stroke-faint/15"
        />
        <g
          class="stroke-faint/50 motion-safe:animate-orbit-spin"
          :style="{ transformOrigin: `${CX}px ${CY}px` }"
        >
          <line
            v-for="(tick, at) in ticks"
            :key="at"
            v-bind="tick"
          />
        </g>
      </g>

      <g
        class="fill-none"
        aria-hidden="true"
      >
        <template
          v-for="satellite in layout.satellites"
          :key="satellite.key"
        >
          <line
            v-bind="satellite.link"
            :stroke="presenceColor[satellite.presence]"
            :stroke-opacity="satellite.presence === 'up' ? 0.45 : 0.9"
            stroke-width="2"
            stroke-dasharray="2 6"
            stroke-linecap="round"
            :class="satellite.presence === 'down' ? 'motion-safe:animate-orbit-flow-fast' : 'motion-safe:animate-orbit-flow'"
          />
          <path
            v-for="leaf in satellite.leaves"
            :key="leaf.key"
            :d="leaf.path"
            :stroke="presenceColor[leaf.presence]"
            :stroke-opacity="leaf.presence === 'up' ? 0.25 : 0.7"
            stroke-width="1.2"
          />
        </template>
      </g>

      <template
        v-for="satellite in layout.satellites"
        :key="satellite.key"
      >
        <a
          v-for="leaf in satellite.leaves"
          :key="leaf.key"
          :href="href(leaf.to)"
          :aria-label="label(leaf.name, leaf.presence)"
          class="cursor-pointer focus-visible:outline-none [&:focus-visible>circle:last-of-type]:stroke-ring"
          @click="follow($event, leaf.to)"
        >
          <circle
            v-if="leaf.presence === 'down' || leaf.presence === 'pending'"
            :cx="leaf.x"
            :cy="leaf.y"
            :r="leaf.node ? 7 : 5"
            fill="none"
            :stroke="presenceColor[leaf.presence]"
            stroke-width="2"
            class="origin-center [transform-box:fill-box] motion-reduce:hidden"
            :class="leaf.presence === 'down' ? 'motion-safe:animate-orbit-pulse' : 'motion-safe:animate-orbit-pulse-slow'"
          />
          <circle
            :cx="leaf.x"
            :cy="leaf.y"
            :r="leaf.node ? 7 : 5"
            :fill="leaf.node ? 'var(--card)' : presenceColor[leaf.presence]"
            :stroke="leaf.node ? presenceColor[leaf.presence] : 'transparent'"
            stroke-width="2.2"
            class="transition-[filter] hover:brightness-125"
          />
          <text
            v-if="leaf.label"
            :x="leaf.label.x"
            :y="leaf.label.y"
            :text-anchor="leaf.label.anchor"
            class="stroke-surface text-[11px] font-semibold [paint-order:stroke] [stroke-width:4px]"
            :class="leaf.presence === 'down' ? 'fill-status-down' : leaf.presence === 'pending' ? 'fill-status-pending' : 'fill-muted'"
          >{{ leaf.name }}</text>
        </a>
        <text
          v-if="satellite.more"
          :x="satellite.more.x"
          :y="satellite.more.y"
          text-anchor="middle"
          class="fill-muted text-[11px]"
          aria-hidden="true"
        >{{ t('observe.orbit.more', { count: satellite.more.count }) }}</text>
      </template>

      <g
        role="img"
        :aria-label="t('observe.orbit.center', { healthy: layout.healthy, total: layout.active })"
      >
        <circle
          :cx="CX"
          :cy="CY"
          :r="ORB + 20"
          :fill="orb"
          opacity="0.3"
          :filter="`url(#${id}-glow)`"
        />
        <circle
          :cx="CX"
          :cy="CY"
          :r="ORB"
          :fill="`url(#${id}-orb)`"
        />
        <circle
          :cx="CX"
          :cy="CY"
          :r="ORB - 19"
          class="fill-surface"
        />
        <circle
          :cx="CX"
          :cy="CY"
          :r="ORB - 12"
          fill="none"
          class="stroke-white/20"
          stroke-width="4"
        />
        <circle
          v-if="!layout.progress"
          :cx="CX"
          :cy="CY"
          :r="ORB - 12"
          fill="none"
          class="stroke-white/90"
          stroke-width="4"
        />
        <path
          v-else
          :d="layout.progress"
          fill="none"
          class="stroke-white/90"
          stroke-width="4"
          stroke-linecap="round"
        />
        <text
          :x="CX"
          :y="CY + 3"
          text-anchor="middle"
          class="fill-primary text-[21px] font-bold tabular-nums"
        >{{ layout.healthy }}/{{ layout.active }}</text>
        <text
          :x="CX"
          :y="CY + 17"
          text-anchor="middle"
          class="fill-muted text-[10px]"
        >{{ t('observe.orbit.healthy') }}</text>
      </g>

      <template
        v-for="satellite in layout.satellites"
        :key="satellite.key"
      >
        <a
          :href="href(satellite.to)"
          :aria-label="`${label(satellite.name, satellite.presence)}, ${satellite.sub.text}`"
          class="group/sat cursor-pointer focus-visible:outline-none"
          @click="follow($event, satellite.to)"
        >
          <circle
            v-if="satellite.presence === 'down'"
            :cx="satellite.x"
            :cy="satellite.y"
            :r="satellite.radius + 4"
            fill="none"
            :stroke="presenceColor.down"
            stroke-width="2"
            class="origin-center [transform-box:fill-box] motion-reduce:hidden motion-safe:animate-orbit-halo"
          />
          <circle
            :cx="satellite.x"
            :cy="satellite.y"
            :r="satellite.radius"
            class="fill-card stroke-hairline transition-colors group-hover/sat:fill-fill group-focus-visible/sat:stroke-ring group-focus-visible/sat:[stroke-width:3px]"
          />
          <template
            v-for="segment in satellite.ring"
            :key="segment.presence"
          >
            <circle
              v-if="segment.whole"
              :cx="satellite.x"
              :cy="satellite.y"
              :r="satellite.radius + 4"
              fill="none"
              :stroke="presenceColor[segment.presence]"
              stroke-width="3"
            />
            <path
              v-else
              :d="segment.d"
              fill="none"
              :stroke="presenceColor[segment.presence]"
              stroke-width="3"
              stroke-linecap="round"
            />
          </template>
          <Icon
            v-if="satellite.icon"
            :name="satellite.icon"
            size="16"
            :x="satellite.x - 8"
            :y="satellite.y - 8"
            :style="{ color: presenceColor[satellite.presence] }"
          />
        </a>
        <a
          :href="href(satellite.to)"
          tabindex="-1"
          aria-hidden="true"
          class="cursor-pointer"
          @click="follow($event, satellite.to)"
        >
          <text
            v-if="!satellite.icon"
            :x="satellite.x"
            :y="satellite.y + 4"
            text-anchor="middle"
            class="fill-primary text-xs font-semibold tabular-nums"
          >{{ satellite.total }}</text>
          <text
            :x="satellite.x"
            :y="satellite.y + satellite.radius + 18"
            text-anchor="middle"
            class="fill-primary stroke-surface text-[12.5px] font-semibold [paint-order:stroke] [stroke-linejoin:round] [stroke-width:5px]"
          >
            {{ satellite.name }}
            <tspan
              v-if="satellite.sub.text"
              :x="satellite.x"
              dy="14"
              class="text-[11px] font-medium"
              :class="satellite.sub.tone"
            >{{ satellite.sub.text }}</tspan>
          </text>
        </a>
      </template>

      <text
        v-if="layout.hidden > 0"
        :x="CX"
        :y="CY + layout.outer + 48"
        text-anchor="middle"
        class="fill-muted text-[11px]"
      >{{ t('observe.orbit.hidden', { count: layout.hidden }) }}</text>
    </svg>
  </div>
</template>
