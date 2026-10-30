# The panel language

Date: 2026-10-30. Owner's call: "some panels are ugly". Every panel
speaks one grammar, Pond's composition with GNOME's colours, type and
close button. This spec wins over `docs/DESIGN.md` where the two disagree
about a panel; the theme-boundary spec still decides what a theme owns,
and the token table (`crates/fs-theme/formalshell.json`) still owns every
colour, face and radius. Pond's numbers are read off its BSD-3 source in
`artifacts/pond/src/shell/` (PanelStyle.qml, Toggle.qml, Slider.qml,
DeviceList.qml, battery/Card.qml); its captures are in
`artifacts/pond/{liked,vm}/`.

## 1. What stays GNOME

- Colours: the table's `card`, `border`, `separator`, `accent` (hover),
  `muted`, `primary` and the role boxes. No Pond hex value lands in code.
- Type: the fontconfig `sans-serif` face for words and `monospace` for
  numbers that tick. Pond's serif titles become our sans in the
  `display` role at 600 weight.
- Close: a flat ghost icon button in the header row's trailing corner,
  `controlHeight` square, the `x` glyph. Never a floating square outside
  the card. A panel that is a page of tiles (Settings) has no header and
  no close: Escape and a click outside close it.
- Focus: the table's one `ring`, drawn by the keyboard only, travelling
  between rows (`halo`). Pond has no focus rings; we keep ours.

## 2. The grid

Panels lay out on a 4px grid. Every gap, inset and height below is a
multiple of 4, read from the panel tokens in fs-theme's spacing table
(§8), never a literal in surface code:

| name | px | Pond's |
| --- | --- | --- |
| `panelInset` | 20 | the card's text inset, 19.5 |
| `panelRowInset` | 8 | rows start 8 in, their fill reaching past the text column |
| `panelRow` | 44 | `rowHeight` |
| `panelRowCompact` | 40 | audio's rows, the slider's control |
| `panelGap` | 8 | icon to text, button to button |
| `panelSection` | 16 | between sections, and a heading to the rows above it |
| `panelIcon` | 20 | a row's leading glyph |
| `panelMark` | 16 | a trailing lock, check circle or status glyph |
| `panelWidth` | 320 | `PanelStyle.width` 316, on the grid |
| `panelWidthWide` | 400 | a panel with a second column of values (Information, Earbuds) |

Radii are the table's libadwaita ladder: rows `lg` (12, Pond's 12),
buttons and segment squares `md` (9, Pond's 8 and 10), the card `xl`
(15, Pond's 16).

## 3. Header

- One row `panelRow` tall: the title in `display`/600, then the panel's
  own controls (at most one toggle pill and one icon button), then the
  close button.
- A hairline under it, inset `panelInset` both sides, one `border` width
  in `separator`. Nothing else in the card is a full-bleed rule.
- The body starts `panelGap` under the hairline.
- No glyph before the title. A panel whose title is a device name (an
  earbud, the iPhone) keeps the title and moves the device glyph into the
  body's first row.

## 4. Rows

- `panelRow` tall, `lg` radius, inset `panelRowInset` from the card's
  sides, so the row fill reaches 12 past the text column.
- Leading glyph `panelIcon` at the row's 12, text `panelGap` after it.
  Primary text `body` 500, a secondary line `caption` in `muted`. One line
  each; long text elides, the marquee only where it already runs.
- Trailing, right-aligned at the text column's end, exactly one of: a
  value in `mono` (`muted`), a `panelMark` glyph (lock, check circle,
  chevron), a toggle pill, or one text button (`md` radius, `outline`).
- Rest: no fill. Hover and the keyboard cursor: `accent`. The row a list
  is on now (the connected network, the default sink, the active
  profile) carries the `ghostOpen` soft fill and its trailing action
  (Disconnect) or a filled check circle. `selected` is never used for a
  list's current row.
- No dividers between rows. Information's numbered list is the one
  exception: hairlines between its facts, inset to the text column.

## 5. Sections

- A heading is `caption` 600 in `muted`, sentence case, at the text
  column, `panelSection` above it and `panelGap` under it. No counts in
  headings; a count goes trailing on the section's last row or not at all.
- A disclosure (Known networks) is a row with a trailing chevron that
  turns, never a heading that toggles.
- Tabs (audio's Output and Input) are a segmented choice (§6), not text.

## 6. Controls

- Toggle: Pond's pill, 52 x 28 (on the grid), a round knob 24 with 2 in
  inset, travelling on `effects`. Off: `border` outline, no fill. On:
  `primary` fill, the knob in `primaryForeground`. One per header at
  most; in a row it sits trailing.
- Slider: a `panelRowCompact` control, the track 12 tall at `md`
  radius in `muted`, the fill in `primary`, a knob 24 round in
  `primary` with a `border` ring. The value sits trailing in `mono`, not
  in a bubble. A muted channel draws the track outlined and the fill 2
  tall.
- Segmented choice: N equal squares in one row, `panelRow` tall, `md`
  radius, `panelGap` apart, a 20 glyph centred and the name under it
  only when every option has room. Rest `muted`, hover `accent`, the
  chosen one `ghostOpen` with the glyph in `primary`. Over it, a label
  row: the name at the left in `muted`, the current choice at the right.
  Power profiles, audio's Output/Input, the visualizer styles and the
  theme mode all use it.
- Buttons in a footer: `outline`, `md` radius, `panelRow` tall, equal
  widths filling the text column, `panelGap` apart.

## 7. States, scrolling, footers

- Empty or unavailable: one line of `body` in `muted`, centred in a
  `panelRow` band under the header, saying what is missing
  ("No Wi-Fi adapter", "No power sources"). No glyph, no illustration, no
  invented device (CLAUDE.md's honest unavailable rule).
- An error: one `caption` line in `destructive` under the row it belongs
  to.
- Lists scroll inside the card past the output's room; the card never
  grows past it. A list that scrolls fades its top 16px once it is off
  the start; nothing fades at the bottom.
- A footer row (Add a network, Search nearby devices) is a row (§4) with
  a `panelIcon` glyph and `muted` text, the last thing in the card.
- A footer of facts (Information's version) is `caption` in `muted`,
  `panelSection` under the body.

## 8. Tokens

§2's numbers join `SEMANTIC_SPACING_BASE` in
`crates/fs-theme/src/tokens.rs`, beside the other semantic spacing, and
reach surfaces as `Theme.space.panel_*`. `ui::w`
gets one constructor per §3 to §7 element (`panel_header`, `panel_row`,
`panel_heading`, `pill`, `segmented`, `panel_empty`, `panel_footer`), and
panels build from those alone. A panel reaching for a raw `Cell` or a
spacing literal is a bug against this spec.

## 9. Proof

`artifacts/panels-before.png` and `artifacts/panels-after.png`: every
panel on one sheet each, dark, in the same order. Each redesign commit
keeps its legs green and updates the probes the new geometry moves.
