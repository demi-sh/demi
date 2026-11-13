<script setup lang="ts">
import { onMounted, onUnmounted, useTemplateRef } from 'vue'

const props = defineProps<{
  size?: number
  strokeWidth?: number
  trackClass?: string
  arcClass?: string
}>()

const svgSize = props.size ?? 14
const stroke = props.strokeWidth ?? 2.5
const radius = (svgSize - stroke) / 2
const circumference = 2 * Math.PI * radius
const cx = svgSize / 2
const cy = svgSize / 2

/**
 * The arc grows from a tenth of the ring to 45% and back as it goes round, its
 * tail moving a fifth of a turn while its head moves more than half.
 *
 * Drawn by animating the stroke's dashes, it would move only on the main thread,
 * and stall while the page works, which is when it shows. So the arc is laid of
 * `SEGMENTS` arcs a tenth of the ring long, each in a layer of its own that only
 * turns, which the compositor moves: the first at the tail, the last at the head,
 * the others evenly between, overlapping, so together they draw the one arc, as
 * long as the tail and the head are apart.
 */
const SEGMENTS = 5
const SEGMENT = 0.1
/** Where the tail and the head stand at the start, the middle and the end of a turn, in turns from the ring's right. */
const TAIL = [0, 0.2, 1]
const HEAD = [SEGMENT, 0.65, 1 + SEGMENT]
const segmentDash = `${circumference * SEGMENT} ${circumference * (1 - SEGMENT)}`

const root = useTemplateRef<HTMLElement>('root')
const segments = useTemplateRef<HTMLElement[]>('segments')

/**
 * The segments' turns run in the compositor while the spinner shows. One
 * whose element has no box, hidden under `display: none` or gone from the
 * page, the browser ticks on the main thread in every frame instead, which
 * keeps the page drawing for nothing. So the turns pause while the spinner
 * has no box, and end once it has left the page: after its parent's leave
 * transition, if it fades out, since the spinner shows to its last frame.
 */
let turns: Animation[] = []
let boxes: ResizeObserver | null = null
// Held apart from the template ref, which Vue clears before the leave transition ends.
let element: HTMLElement | null = null

function end(): void {
  boxes?.disconnect()
  boxes = null
  for (const turn of turns)
    turn.cancel()
  turns = []
}

onMounted(() => {
  turns = (segments.value ?? []).map((segment, index) => {
    const share = index / (SEGMENTS - 1)
    return segment.animate(TAIL.map((tail, at) => ({
      // The segment's own start, from the tail's toward the head's last tenth.
      transform: `rotate(${(tail + share * (HEAD[at]! - SEGMENT - tail)) * 360}deg)`,
    })), {
      duration: 1500,
      easing: 'ease-in-out',
      iterations: Infinity,
    })
  })
  element = root.value
  boxes = new ResizeObserver(([entry]) => {
    if (!element?.isConnected) {
      end()
      return
    }
    const box = entry?.borderBoxSize[0]
    const shown = (box?.inlineSize ?? 0) > 0 && (box?.blockSize ?? 0) > 0
    for (const turn of turns) {
      if (shown && turn.playState === 'paused')
        turn.play()
      else if (!shown && turn.playState === 'running')
        turn.pause()
    }
  })
  if (element)
    boxes.observe(element)
})

// A spinner that fades out with its parent is still on the page here, and
// ends when the observer sees it leave.
onUnmounted(() => {
  if (!element?.isConnected)
    end()
})
</script>

<template>
  <span
    ref="root"
    class="spinner-rotate relative inline-block"
    :style="{ width: `${svgSize}px`, height: `${svgSize}px` }"
  >
    <svg
      class="absolute inset-0"
      :width="svgSize" :height="svgSize"
      :viewBox="`0 0 ${svgSize} ${svgSize}`"
    >
      <circle
        :cx="cx" :cy="cy" :r="radius"
        fill="none"
        stroke="currentColor"
        :stroke-width="stroke"
        :class="trackClass ?? 'opacity-20'"
      />
    </svg>
    <!-- One group, so overlapping segments draw as one arc under an arc's opacity. -->
    <span class="absolute inset-0" :class="arcClass">
      <span
        v-for="index in SEGMENTS"
        :key="index"
        ref="segments"
        class="absolute inset-0 will-change-transform"
      >
        <svg
          class="block"
          :width="svgSize" :height="svgSize"
          :viewBox="`0 0 ${svgSize} ${svgSize}`"
        >
          <circle
            :cx="cx" :cy="cy" :r="radius"
            fill="none"
            stroke="currentColor"
            :stroke-width="stroke"
            stroke-linecap="round"
            :stroke-dasharray="segmentDash"
          />
        </svg>
      </span>
    </span>
  </span>
</template>

<style scoped>
.spinner-rotate {
  animation: spinner-rotate 2s linear infinite;
}

@keyframes spinner-rotate {
  0% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
}
</style>
