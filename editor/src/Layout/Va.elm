module Layout.Va exposing (modules)

{-| VA's page, in v6's order: OSC 1, OSC 2, MODE, MIXER, the voice, then
PITCH and MOD. The wave is the continuous variable wave, so it is a knob.
-}

import Layout.Shared as Shared exposing (Module, auto, fader, group, hero, knob, menu, module_, strip)
import Ui.Screen exposing (Glyph(..))


modules : List Module
modules =
    [ osc 1, osc 2, mode, mixer, Shared.filter, Shared.filterMod, Shared.voiceFx, Shared.ampOut ]
        ++ List.map Shared.envelope (List.range 1 3)
        ++ List.map Shared.lfo (List.range 1 3)
        ++ [ Shared.pitch, Shared.mod ]


osc : Int -> Module
osc n =
    let
        m : Module
        m =
            module_ ("OSC " ++ String.fromInt n) "OSCILLATORS"

        a : String -> String
        a f =
            "VA." ++ f ++ String.fromInt n
    in
    { m
        | screen = Maybe.map (\s -> { s | fallback = Just VaWave }) (strip ("OSC" ++ String.fromInt n))
        , groups =
            [ group "" [ hero (a "WAVE"), knob (a "SHAPE") ]
            , group "PITCH" [ knob (a "CRSE"), knob (a "FINE") ]
            ]
    }


mode : Module
mode =
    let
        m : Module
        m =
            module_ "MODE" "OSCILLATORS"
    in
    { m
        | screen = strip "VA"
        , groups =
            [ group "" [ menu "VA.MODE" ]
            , group "" [ hero "VA.AMOUNT" ]
            , group "" [ fader "VA.STAB" ]
            ]
    }


mixer : Module
mixer =
    let
        m : Module
        m =
            module_ "MIXER" "OSCILLATORS"
    in
    { m
        | screen = strip "NOISE"
        , groups =
            [ group "" [ fader "VA.SUB" ]
            , group "NOISE" [ auto "VA.NOISEKIND", fader "VA.NOISE" ]
            ]
    }
