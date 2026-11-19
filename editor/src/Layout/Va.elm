module Layout.Va exposing (modules)

{-| VA's page, in v6's order: OSC 1, OSC 2, MODE, MIXER, the voice, then
PITCH and MOD. The wave is the continuous variable wave, so it is a knob.
-}

import Addr exposing (Addr, Block(..), Osc(..))
import Layout.Page exposing (UnitPage(..))
import Layout.Shared as Shared exposing (Module, auto, fader, group, hero, knob, menu, module_, strip)
import Ui.Chip as Chip
import Ui.Screen exposing (Glyph(..))


modules : List Module
modules =
    [ osc Osc1, osc Osc2, mode, mixer, Shared.filter, Shared.filterMod, Shared.voiceFx Fx, Shared.ampOut ]
        ++ List.map Shared.envelope Addr.envs
        ++ List.map Shared.lfo Addr.lfos
        ++ [ Shared.pitch, Shared.mod ]


va : String -> Addr
va =
    Addr.field >> Addr.at Va


osc : Osc -> Module
osc o =
    let
        a : String -> Addr
        a =
            Addr.field >> Addr.vaOsc o
    in
    module_ Chip.Oscillators ("OSC " ++ String.fromInt (Addr.oscNumber o))
        |> Shared.withScreen (strip (OscPage o) |> Shared.withFallback VaWave)
        |> Shared.withGroups
            [ group "" [ hero (a "WAVE"), knob (a "SHAPE") ]
            , group "PITCH" [ knob (a "CRSE"), knob (a "FINE") ]
            ]


mode : Module
mode =
    module_ Chip.Oscillators "MODE"
        |> Shared.withScreen (strip VaPage)
        |> Shared.withGroups
            [ group "" [ menu (va "MODE") ]
            , group "" [ hero (va "AMOUNT") ]
            , group "" [ fader (va "STAB") ]
            ]


mixer : Module
mixer =
    module_ Chip.Oscillators "MIXER"
        |> Shared.withScreen (strip Noise)
        |> Shared.withGroups
            [ group "" [ fader (va "SUB") ]
            , group "NOISE" [ auto (va "NOISEKIND"), fader (va "NOISE") ]
            ]
