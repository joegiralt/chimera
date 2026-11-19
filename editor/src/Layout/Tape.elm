module Layout.Tape exposing (modules)

{-| TAPE's page, in v6's order: SOURCE (read-only: the sample lives on the
card, #1015), DECK (its MODEL in the title bar), MACHINE, PITCH, then the
voice and MOD. The machine picks what DECK and MACHINE hold.
-}

import Addr exposing (Addr, Block(..))
import Layout.Page exposing (UnitPage(..))
import Layout.Shared as Shared exposing (Extra(..), Module, auto, fader, group, hero, knob, module_, strip)
import Ui.Chip as Chip


modules : List Module
modules =
    [ source, deck, machine, Shared.pitch, Shared.filter, Shared.filterMod, Shared.voiceFx Fx, Shared.ampOut ]
        ++ List.map Shared.envelope Addr.envs
        ++ List.map Shared.lfo Addr.lfos
        ++ [ Shared.mod ]


a : String -> Addr
a =
    Addr.field >> Addr.at Tape


source : Module
source =
    module_ Chip.Tape "SOURCE"
        |> Shared.withNote "READ-ONLY"
        |> Shared.withScreen (strip Src)
        |> Shared.withGroups [ group "" [ knob (a "START"), knob (a "END") ] ]
        |> Shared.withExtras [ SourceText, Note "THE SAMPLE LIVES ON THE CARD: CHANGE IT ON THE UNIT, AND THE EDITOR SHOWS IT" ]


deck : Module
deck =
    module_ Chip.Tape "DECK"
        |> Shared.withHeader [ a "MODEL" ]
        |> Shared.withScreen (strip Deck)
        |> Shared.withGroups
            [ group "" [ hero (a "POSITION") ]
            , group "" [ knob (a "MOTION"), auto (a "TYPE") ]
            , group "" [ fader (a "SIZE"), fader (a "DRAG"), fader (a "STRETCH_LENGTH"), fader (a "XFADE"), fader (a "DRIFT"), auto (a "MODE") ]
            ]


machine : Module
machine =
    module_ Chip.Tape "MACHINE"
        |> Shared.withScreen (strip Mch)
        |> Shared.withGroups
            [ group "TAPE" [ fader (a "TAPE"), fader (a "AGE"), auto (a "WOW") ]
            , group "" [ auto (a "TRON"), auto (a "RETRIG"), fader (a "GRAIN"), fader (a "SPRAY"), auto (a "STRETCH_END"), auto (a "KEY") ]
            ]
