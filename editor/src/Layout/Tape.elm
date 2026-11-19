module Layout.Tape exposing (modules)

{-| TAPE's page, in v6's order: SOURCE (read-only: the sample lives on the
card, #1015), DECK (its MODEL in the title bar), MACHINE, PITCH, then the
voice and MOD. The machine picks what DECK and MACHINE hold.
-}

import Layout.Shared as Shared exposing (Extra(..), Module, auto, fader, group, hero, knob, module_, strip)
import Protocol exposing (Addr(..))


modules : List Module
modules =
    [ source, deck, machine, Shared.pitch, Shared.filter, Shared.filterMod, Shared.voiceFx, Shared.ampOut ]
        ++ List.map Shared.envelope (List.range 1 3)
        ++ List.map Shared.lfo (List.range 1 3)
        ++ [ Shared.mod ]


source : Module
source =
    let
        m : Module
        m =
            module_ "SOURCE" "TAPE"
    in
    { m
        | note = "READ-ONLY"
        , screen = strip "SRC"
        , groups = [ group "" [ knob "TAPE_ENG.START", knob "TAPE_ENG.END" ] ]
        , extras = [ SourceText, Note "THE SAMPLE LIVES ON THE CARD: CHANGE IT ON THE UNIT, AND THE EDITOR SHOWS IT" ]
    }


deck : Module
deck =
    let
        m : Module
        m =
            module_ "DECK" "TAPE"
    in
    { m
        | header = [ Addr "TAPE_ENG.MODEL" ]
        , screen = strip "DECK"
        , groups =
            [ group "" [ hero "TAPE_ENG.POSITION" ]
            , group "" [ knob "TAPE_ENG.MOTION", auto "TAPE_ENG.TYPE" ]
            , group "" [ fader "TAPE_ENG.SIZE", fader "TAPE_ENG.DRAG", fader "TAPE_ENG.STRETCH_LENGTH", fader "TAPE_ENG.XFADE", fader "TAPE_ENG.DRIFT", auto "TAPE_ENG.MODE" ]
            ]
    }


machine : Module
machine =
    let
        m : Module
        m =
            module_ "MACHINE" "TAPE"
    in
    { m
        | screen = strip "MCH"
        , groups =
            [ group "TAPE" [ fader "TAPE_ENG.TAPE", fader "TAPE_ENG.AGE", auto "TAPE_ENG.WOW" ]
            , group "" [ auto "TAPE_ENG.TRON", auto "TAPE_ENG.RETRIG", fader "TAPE_ENG.GRAIN", fader "TAPE_ENG.SPRAY", auto "TAPE_ENG.STRETCH_END", auto "TAPE_ENG.KEY" ]
            ]
    }
