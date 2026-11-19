module Ui.Chip exposing (Chip(..), Chips, all, label, member, none, toggle)

{-| The show/hide chips of a page: which group of modules a module belongs
to. Several modules share a chip (the three envelopes are ENVELOPES).
-}

import Set exposing (Set)


type Chip
    = Algorithm
    | Pitch
    | Operators
    | Oscillators
    | Resonator
    | Tape
    | Filter
    | Fx
    | Amp
    | Envelopes
    | Lfos
    | Mod
    | Kit
    | Out
    | Pad
    | Other
    | Unit
    | Controls
    | Choices
    | Screens
    | Folding


{-| Every chip, so a test can check each has its own word.
-}
all : List Chip
all =
    [ Algorithm, Pitch, Operators, Oscillators, Resonator, Tape, Filter, Fx, Amp, Envelopes, Lfos, Mod, Kit, Out, Pad, Other, Unit, Controls, Choices, Screens, Folding ]


label : Chip -> String
label c =
    case c of
        Algorithm ->
            "ALGORITHM"

        Pitch ->
            "PITCH"

        Operators ->
            "OPERATORS"

        Oscillators ->
            "OSCILLATORS"

        Resonator ->
            "RESONATOR"

        Tape ->
            "TAPE"

        Filter ->
            "FILTER"

        Fx ->
            "FX"

        Amp ->
            "AMP"

        Envelopes ->
            "ENVELOPES"

        Lfos ->
            "LFOS"

        Mod ->
            "MOD"

        Kit ->
            "KIT"

        Out ->
            "OUT"

        Pad ->
            "PAD"

        Other ->
            "OTHER"

        Unit ->
            "UNIT"

        Controls ->
            "CONTROLS"

        Choices ->
            "CHOICES"

        Screens ->
            "SCREENS"

        Folding ->
            "FOLDING"


{-| A set of chips (the hidden ones), keyed by each chip's own word.
-}
type Chips
    = Chips (Set String)


none : Chips
none =
    Chips Set.empty


member : Chip -> Chips -> Bool
member c (Chips s) =
    Set.member (label c) s


toggle : Chip -> Chips -> Chips
toggle c (Chips s) =
    Chips
        (if Set.member (label c) s then
            Set.remove (label c) s

         else
            Set.insert (label c) s
        )
