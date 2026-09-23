# 005 — FM chain routes the LFO but has no page to edit it

Found by the engine-refactor final review (2026-09-23).

Since Task 22 every Part chain declares mod sources `["ENV", "LFO"]`, matching
what `Voice` produces. The Pizza and Modal chains have Envelope and LFO pages
under MOD; the FM chain's MOD sub-pages are FM_ENV1..4 (operator envelopes),
so on FM the LFO (and amp envelope) can be routed but not edited.

Kept as-is by the engine-refactor spec. Follow-up: add LFO (and amp env) pages
to the FM chain's MOD block, or decide where they live when chains become
composable (sub-project 2/3).
