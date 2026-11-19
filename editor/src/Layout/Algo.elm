module Layout.Algo exposing (chartFocus, modules)

{-| ALGO's page, in v6's order: ALGORITHM, PITCH, OP 1–6, then the voice
(FILTER, FILTER MOD, VOICE FX, AMP · OUT, ENV 1–3, LFO 1–3) and MOD.
-}

import Addr exposing (Addr, Block(..), Operator)
import Layout.Page exposing (UnitPage(..))
import Layout.Shared as Shared exposing (Module, auto, fader, group, hero, heroFader, knob, menu, module_, strip)
import Ui.Chip as Chip
import Ui.Screen exposing (Focus(..), Glyph(..))


modules : List Module
modules =
    [ algorithm, Shared.pitch ]
        ++ List.map operator Addr.operators
        ++ [ Shared.filter, Shared.filterMod, Shared.voiceFx Fx, Shared.ampOut ]
        ++ List.map Shared.envelope Addr.envs
        ++ List.map Shared.lfo Addr.lfos
        ++ [ Shared.mod ]


{-| The cell the ALG page's focus band is on while the chart draws: MORPH's
(knob C). On ALG A or B the unit draws that algorithm alone; on any other
cell it draws the chart morphing between them, as MORPH moves.
-}
chartFocus : Focus
chartFocus =
    CellC


algorithm : Module
algorithm =
    let
        a : String -> Addr
        a =
            Addr.field >> Addr.at Algo
    in
    module_ Chip.Algorithm "ALGORITHM"
        |> Shared.withScreen (strip Alg |> Shared.withFallback AlgoChart |> Shared.withFocus chartFocus)
        |> Shared.withGroups
            [ group "" [ menu (a "ALG_A"), menu (a "ALG_B") ]
            , group "" [ hero (a "MORPH"), knob (a "TRNSP"), knob (a "VOWEL") ]
            ]


{-| An operator (the unit's OP\_A to OP\_F): its WAVE in the title bar, then
the kind's controls. TABLE has a ratio; FORMANT and NOISE a band.
-}
operator : Operator -> Module
operator o =
    let
        a : String -> Addr
        a =
            Addr.field >> Addr.at (Op o)
    in
    module_ Chip.Operators ("OP " ++ String.fromInt (Addr.operatorNumber o))
        |> Shared.withHeader [ a "WAVE" ]
        |> Shared.withScreen (strip (OpPage o) |> Shared.withFallback OpWave)
        |> Shared.withGroups
            [ group "PITCH" [ hero (a "CRSE"), knob (a "FINE"), knob (a "DETUN"), fader (a "FDBK") ]
            , group "OUTPUT" [ heroFader (a "LEVEL"), fader (a "VEL") ]
            , group "ENVELOPE" [ fader (a "AR"), fader (a "D1R"), fader (a "D1L"), fader (a "D2R"), fader (a "RR"), fader (a "RS") ]
            , group "BAND" [ hero (a "FRMT"), knob (a "BW"), fader (a "SKIRT"), fader (a "KEY"), fader (a "RES"), auto (a "VSLOT") ]
            ]
