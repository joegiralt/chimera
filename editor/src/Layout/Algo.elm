module Layout.Algo exposing (modules)

{-| ALGO's page, in v6's order: ALGORITHM, PITCH, OP 1–6, then the voice
(FILTER, FILTER MOD, VOICE FX, AMP · OUT, ENV 1–3, LFO 1–3) and MOD.
-}

import Layout.Shared as Shared exposing (Module, auto, fader, group, hero, heroFader, knob, menu, module_, strip)
import Protocol exposing (Addr(..))
import Ui.Screen exposing (Glyph(..))


modules : List Module
modules =
    [ algorithm, Shared.pitch ]
        ++ List.map operator (List.range 1 6)
        ++ [ Shared.filter, Shared.filterMod, Shared.voiceFx, Shared.ampOut ]
        ++ List.map Shared.envelope (List.range 1 3)
        ++ List.map Shared.lfo (List.range 1 3)
        ++ [ Shared.mod ]


algorithm : Module
algorithm =
    let
        m : Module
        m =
            module_ "ALGORITHM" "ALGORITHM"
    in
    { m
        | screen = Maybe.map (\s -> { s | fallback = Just AlgoChart }) (strip "ALG")
        , groups =
            [ group "" [ menu "ALGO.ALG_A", menu "ALGO.ALG_B" ]
            , group "" [ hero "ALGO.MORPH", knob "ALGO.TRNSP", knob "ALGO.VOWEL" ]
            ]
    }


{-| Operator `n` (the unit's OP\_A to OP\_F): its WAVE in the title bar, then
the kind's controls. TABLE has a ratio; FORMANT and NOISE a band.
-}
operator : Int -> Module
operator n =
    let
        m : Module
        m =
            module_ ("OP " ++ String.fromInt n) "OPERATORS"

        a : String -> String
        a f =
            "OP_" ++ String.fromChar (Char.fromCode (64 + n)) ++ "." ++ f
    in
    { m
        | header = [ Addr (a "WAVE") ]
        , screen = Maybe.map (\s -> { s | fallback = Just OpWave }) (strip ("OP" ++ String.fromInt n))
        , groups =
            [ group "PITCH" [ hero (a "CRSE"), knob (a "FINE"), knob (a "DETUN"), fader (a "FDBK") ]
            , group "OUTPUT" [ heroFader (a "LEVEL"), fader (a "VEL") ]
            , group "ENVELOPE" [ fader (a "AR"), fader (a "D1R"), fader (a "D1L"), fader (a "D2R"), fader (a "RR"), fader (a "RS") ]
            , group "BAND" [ hero (a "FRMT"), knob (a "BW"), fader (a "SKIRT"), fader (a "KEY"), fader (a "RES"), auto (a "VSLOT") ]
            ]
    }
