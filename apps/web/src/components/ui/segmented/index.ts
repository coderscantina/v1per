import { cva, type VariantProps } from 'class-variance-authority'

/**
 * The one segmented control look: a bordered track a step off its background, with the chosen
 * option raised onto it in the fill colour. Shared by `ChoiceGroup`, `Tabs`, the toggle groups and
 * the mode switch, which mark the chosen option with `aria-checked`, `data-state=on` or
 * `data-state=active`.
 */
export const segmentedTrack = 'inline-flex max-w-full flex-wrap gap-0.5 rounded-[11px] border border-hairline bg-surface p-[3px]'

const raised = [
  'aria-checked:bg-fill-strong aria-checked:text-primary aria-checked:shadow-[inset_0_1px_0_color-mix(in_oklch,white_7%,transparent)]',
  'data-[state=on]:bg-fill-strong data-[state=on]:text-primary data-[state=on]:shadow-[inset_0_1px_0_color-mix(in_oklch,white_7%,transparent)]',
  'data-[state=active]:bg-fill-strong data-[state=active]:text-primary data-[state=active]:shadow-[inset_0_1px_0_color-mix(in_oklch,white_7%,transparent)]',
].join(' ')

export const segmentedItem = cva(
  `inline-flex cursor-pointer items-center justify-center gap-1.5 font-medium whitespace-nowrap text-muted transition-colors hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 ${raised}`,
  {
    variants: {
      size: {
        sm: 'h-[26px] rounded-[7px] px-2.5 text-xs max-sm:min-h-8',
        md: 'h-[30px] rounded-lg px-3.5 text-[0.8125rem] max-sm:px-3',
      },
    },
    defaultVariants: { size: 'sm' },
  }
)

export type SegmentedItemVariants = VariantProps<typeof segmentedItem>
