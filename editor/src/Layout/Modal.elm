module Layout.Modal exposing (modules)

{-| MODAL's page, in v6's order: RESONATOR (its MODEL in the title bar),
EXCITER, MODAL-2, PITCH, then the voice and MOD. The model picks what the
exciter and MODAL-2 hold.
-}

import Addr exposing (Addr, Block(..))
import Layout.Page exposing (UnitPage(..))
import Layout.Shared as Shared exposing (Module, auto, fader, group, hero, knob, module_, strip)
import Ui.Chip as Chip


modules : List Module
modules =
    [ resonator, exciter, modal2, Shared.pitch, Shared.filter, Shared.filterMod, Shared.voiceFx Fx, Shared.ampOut ]
        ++ List.map Shared.envelope Addr.envs
        ++ List.map Shared.lfo Addr.lfos
        ++ [ Shared.mod ]


a : String -> Addr
a =
    Addr.field >> Addr.at Modal


resonator : Module
resonator =
    module_ Chip.Resonator "RESONATOR"
        |> Shared.withHeader [ a "MODE" ]
        |> Shared.withScreen (strip Res)
        |> Shared.withGroups
            [ group "" [ hero (a "BRIGHT"), knob (a "STRUCTURE") ]
            , group "" [ fader (a "DAMP"), fader (a "POS") ]
            ]


exciter : Module
exciter =
    module_ Chip.Resonator "EXCITER"
        |> Shared.withScreen (strip Exc)
        |> Shared.withGroups [ group "" [ hero (a "EXCITE"), hero (a "FORCE"), fader (a "COLOR"), fader (a "BURST"), fader (a "SPEED") ] ]


modal2 : Module
modal2 =
    module_ Chip.Resonator "MODAL-2"
        |> Shared.withScreen (strip Mdl2)
        |> Shared.withGroups
            [ group "" [ auto (a "MODES") ]
            , group "BODY" [ knob (a "COUPLE"), fader (a "HALO"), fader (a "BODY") ]
            , group "ENSEMBLE" [ fader (a "E.DPT"), fader (a "E.RAT"), fader (a "E.MIX") ]
            ]
