# Responsive Listing Layout

## Summary

The directory listing renders a four-column table that cannot fit phone viewports. Below 640px the grid's minimum track widths exceed the available space, the actions column is pushed outside the panel, and panels end up with inconsistent widths. This spec defines a stacked layout for narrow viewports and removes the `NaN KB` artifact on directory rows.

## Problem Statement

On a 390px viewport the listing grid requires roughly 470px: the name track has a 10rem floor that `1fr` cannot shrink below, the size and modified tracks are fixed, and gaps and padding add the rest. The overflow pushes the actions cell outside the panel border and makes the listing panel wider than the breadcrumb panel above it. Directory rows display `NaN KB` because the frontend formats a missing `sizeBytes` as `NaN` instead of rendering nothing.

## User Story

As a visitor browsing the file manager from a phone, I want the directory listing to fit the viewport with all actions reachable, so that I can read names and download files without horizontal scrolling.

## Scope

- In scope:
  - Stacked listing layout below 640px: hidden header row, name and actions on the first line, size and modified on the second line.
  - Empty size cell for entries without a `sizeBytes` value.
- In scope:
  - Screenshot-based verification at 320px, 390px, 768px and desktop widths.

## Non-Goals

- Not in scope: changes to the API contract, the generated client, or backend behavior.
- Not in scope: new listing features such as sorting, selection or upload.

## UX / Flow

Below 640px each row renders as two stacked lines inside the existing panel. The first line carries the entry name (truncating with an ellipsis) and the action button right-aligned. The second line carries the size left-aligned and the modified timestamp right-aligned, both in tabular numerals. The column header row is hidden because the stacked lines are self-describing. Desktop rendering above 640px is unchanged.

Entries without a size value (directories) render an empty size cell, matching the existing behavior of the modified column for missing timestamps.

## Technical Notes

- The stacked layout is a scoped CSS change in `frontend/src/App.vue` inside the existing `max-width: 640px` media query, using grid template areas over the existing cell classes.
- `formatSize(undefined)` in `frontend/src/lib/format.ts` currently coalesces to `NaN` (asserted in `tests/frontend-helpers.spec.ts`). It now returns an empty string for both `null` and `undefined`, symmetric with `formatModified`.
- No trust boundaries are affected: all path handling and validation stay on the server.

## Security Considerations

None. Presentation-only change; no user-controlled input reaches new code paths.

## Acceptance Criteria

- [ ] At 320px and 390px the listing panel fits the viewport with no horizontal overflow, and the actions button is fully visible inside the panel.
- [ ] The listing panel and the breadcrumb panel share the same width at all viewports.
- [ ] Directory rows render an empty size cell instead of `NaN KB`.
- [ ] Desktop rendering above 640px is visually unchanged.
- [ ] `npm run check` and `npm run lint` pass.
