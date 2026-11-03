# antislop Delivery Gate

Scope: the generated documentation site and the diff page. Read-only verification,
built from `site/` on 2026-11-02.

Design Read: developer documentation for defense mission-systems engineers, in a
utilitarian field-manual style, dial ENERGY 1 / RHYTHM 2 / MOTION 1.

## Block 1: Hard Gate

- R-02 PASS: zero em dash characters in all 10,702 generated pages, including schema-quoted text.
- R-03 PASS: no horizontal overflow at 390 px on five representative pages; 44 px minimum controls.
- R-17 PASS: only computed schema counts are shown; no third-party statistics.
- R-18 PASS: no testimonials or avatars.
- R-23 PASS: no generated logos, avatars, or people. The brand is text.
- R-24 PASS: every navigation link resolves to an existing page; `scripts/check_site.py` checked all pages.
- R-25 PASS: eight text/background pairs pass WCAG AA; lowest is 5.49:1.
- R-26 PASS: search filters, theme toggles, and all navigation links work; no dead controls.
- R-27 PASS: the site is static. Search has an explicit empty state. No data fetch means no loading or error view is needed.
- R-28 PASS: no FAQ.
- R-32 PASS: keyboard focus reaches a link with a 3 px visible outline; controls are native elements.
- R-33 PASS: no patch scripts. All output is generated from source.
- R-34 PASS: light and dark themes both render; both were clicked through.
- R-35 PASS: 30 Playwright checks, including six pages at 390 px, search, theme, navigation, focus, and console errors.
- R-36 PASS: no security, compliance, or performance claims.
- R-37 PASS: direction declared above; the build follows it.
- R-38 PASS: no fabricated content. Examples are generated from the public schema and marked as generated.
- R-39 PASS: `lang`, unique title, landmarks, one h1 per page, labeled search, `aria-pressed` theme button, no images.
- R-40 PASS: no images, video, or iframes; no third-party scripts; no unmeasured performance number.

## Block 2: Purpose-Gate

- R-01 PASS: no gradients or glows.
- R-04 PASS: no icons.
- R-06 PASS: serif body for manual reading, mono for identifiers; no uppercase tracking.
- R-07 PASS: no background grid or texture.
- R-08 PASS: no decorative arrows.
- R-09 PASS: no capsule badges. Diff class labels are functional text.
- R-10 PASS: no glassmorphism.
- R-12 PASS: no shadows.
- R-13 PASS: no glow.
- R-14 PASS: no feature cards.
- R-19 PASS: motion is limited to hover and focus; reduced motion is honored.
- R-22 PASS: no illustrations.

## Block 3: Liveliness

- PASS: dials declared and held: ENERGY 1, RHYTHM 2, MOTION 1.
- PASS: reference pages are uniform; the message index and diff report break the rhythm.
- PASS: one focal point per page, the page heading.
- PASS: whitespace separates the header strip, fields table, and example.
- PASS: one accent, rust in light mode and warm amber in dark mode, reserved for links, required markers, and diff classes.
- PASS: identity motif is the message header strip with name, primitive, version, and field count.
- PASS: Design Read declared before CSS.

## Block 4: Craftsmanship

- C-1 PASS: every choice has a stated reason in this file or `README.md`.
- C-2 PASS: no dead controls.
- C-3 PASS: every section serves reference reading: index, fields, example, diff.
- C-4 PASS: both themes, five breakpoints, keyboard, and no-JS content all hold.
- C-5 PASS: no fabricated claims.
- R-05 PASS: no landing-page template; the site is a reference manual.
- R-11 PASS: no pill shapes; the site uses square corners.
- R-15 PASS: links are specific ("Messages", "Types", "Diff"), not generic CTAs.
- R-16 PASS: no marketing buzzwords.
- R-20 PASS: identity comes from typography, the header strip, and the rust accent.
- R-21 PASS: theme toggle exists and works in both modes.
- R-29 PASS: two core colors plus one accent, with neutrals.
- R-30 PASS: not a clone of a product site; it reads as a technical manual.
- R-31 PASS: color, layout, typography, spacing, tables, and motif each have a one-line reason.

Result: PASS. No failing item.
