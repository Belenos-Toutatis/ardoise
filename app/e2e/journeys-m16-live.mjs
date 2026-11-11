// M16 journey, the live control window's side panel (docs/design/side-panel.md, §10 "End-to-end journeys"): four tabs
// (no Page, no Fond), collapse, the view remembered, the badges, the selection above the open tab, the share and the
// projection in Projection. Package 0 registers the empty list in `run.mjs`; package D writes the journey (driven as the
// others, in French and in English, with `--simulated-tablet`) with the helpers of `side-panel.mjs`.
export const sidePanelLiveJourneys = [];
