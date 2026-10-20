# Design rules

V1Per's admin app follows holocron's Kit and v2 design documents (see [References](README.md#references)). This file holds the rules Mike settled for V1Per on top of them. Where they differ, this file wins.

## Forms

Settled on 2026-10-20 after a design review found the forms cramped and wordy.

- One body size. Labels, values, hints, option metas and errors are 14px. Muted color sets a hint apart, not a smaller font. `FieldMessages` renders the hint and the error for every field component.
- Few hints. A limit is enforced with `maxlength` and reported by the error, not announced under the field. A sentence that explains a whole section goes in the `SettingsSection` description, in the left column. A field keeps its own hint only when it is unusable without one.
- Units sit inside the field with `FormField`'s `suffix` ("Interval", suffix "s"), never in the label.
- A list of options is a `CheckboxGroup`: one line per option, a one or two word meta on the right, "Select all" only from five options.

## Selection

The admin app selects like a desktop app: chrome, labels, headings and buttons don't highlight. Fields, code, monospace data and detail values (`dd`) stay selectable, and so does text people copy, such as check messages, delivery errors, alerts and incident updates (`select-text`). The public status page keeps normal document selection.

## Tinted surfaces

Settled on 2026-10-20 in the monitor page review.

- An alert, the monitor status block and the open incident card sit on an 8% tint of their tone with a hairline of the same tone at 30%. No glow, no colored bar.
- Text on a tint is the strongest ink (`primary`), and secondary text is that ink at 72% opacity, not the gray `muted` token. These surfaces redefine `--muted` for their children, so `text-muted` inside them follows.

## Figures

Settled on 2026-10-20 in the monitor page review.

- A figure's value carries the weight, not its label. Values are semibold in the strongest ink and a size larger than the text around them (`StatFigure`: `md`, `lg`, `xl`). Labels are 13px and muted.
- On a status card, the label sits below the value, so a row of figures reads as numbers first (`label-below`). Elsewhere, such as the History panel, the label stays above.
- Labels are short when the section already names the measure: "24 h" under an "Uptime" heading, not "Uptime 24 hours".
- A marker that ties a figure to a chart, such as the ring icon, goes in the label, never next to the value.

## Motion and stability

Settled on 2026-10-20 after a review of perceived speed.

- A view arrives with a 90ms opacity fade and no movement. Data animations, such as the uptime rings, take 300ms at most; hover feedback 100ms.
- Hovering never changes a box's size. A readout that replaces text on hover sits on one line of fixed height and truncates.
- A loading block has the height of what it stands for, and the labels of a panel's figures show while their values load.
- Live snapshots that views reopen pass a `cache` name to `useLiveSnapshot`, so the last data shows at once while the fresh read runs. Sign-out clears them.

## Status page

Settled on 2026-10-20 from three variants, loosely after status.betterstack.com.

- Three views at their own paths below the page's base: Status (`/`), Maintenance (`/maintenance`) and Incidents (`/incidents`). A segmented nav in the header switches them in place with the History API, so the live snapshot stays and links can be shared. Past incidents load only on the Incidents view.
- The Status view leads with a centered hero: the status icon at 48px, the headline at 24px and "Updated" muted below. Then a row of three figures (90-day uptime, incidents in 90 days, next maintenance), open incidents and maintenance under way, then "System status".
- Figures follow the [Figures](#figures) rule: the value semibold in the strongest ink, the label 13px and muted below it. A component's 90-day uptime is the bold value on its name row, with "uptime" as the label; phones hide the label.
- Healthy is quiet. An operational component or group shows only the green check, the word stays for screen readers. Every other state shows icon and word.
- The bars' time axis ("90 days ago", "Today") is labeled once per card, not per component.
- An open incident uses the [Tinted surfaces](#tinted-surfaces) rule.
- The footer reads "Powered by V1Per", with V1Per linking to the repository.
- Rejected: tabs below the banner with one line per component (the bars got too small on phones), and components always shown with tabs only for incidents and maintenance (the page stayed long; its figures row was kept).
