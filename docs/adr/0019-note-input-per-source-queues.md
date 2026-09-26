# 0019. One parser and one single-producer queue per note source

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
Notes arrive from several contexts: the chip's DIN USART interrupt (and USB
in sub-project 2), the desktop's midir callback thread and its UI thread
(computer keyboard). `NoteQueue` is single-producer/single-consumer: two
producers sharing one queue would race on its tail.

## Decision
Each source owns one `MidiParser` (where it parses bytes) and one
`NoteQueue` inside `NoteSources<N>`, and is that queue's only producer.
The audio side drains every queue once per block in a fixed order (source 0
first) and passes each `NoteEvent` to `Instrument::handle`, which keeps the
routing: every Part on the event's channel plays it, and note-offs release
by the recorded note-on channel. There is no cross-source timestamp: within
one block a note-off from one source may be applied before a note-on from
another. Each queue counts its own drops (shown per source on the AUDIO page).

## Alternatives considered
- **One multi-producer queue** — needs a CAS loop or a lock in the interrupt.
- **Timestamps and a merge** — no source has a shared clock yet (tempo is
  sub-project 4); ordering inside 1.33 ms is inaudible.
- **Routing per source** — duplicates `Instrument::handle`'s channel logic.

## Consequences
Adding a source is a queue, a `SourceId` and a producer; `MAX_NOTE_SOURCES`
bounds the stats array. Sub-project 2 adds USB as a second chip source.

## Sources
`docs/superpowers/specs/2026-09-26-instrument-on-chip-design.md` § Module
layout, § MIDI parsing; `chimera-core/src/note_queue.rs`.
