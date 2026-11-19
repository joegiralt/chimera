module Layout.Modal exposing (modules)

{-| MODAL's page, in v6's order: RESONATOR (its MODEL in the title bar),
EXCITER, MODAL-2, PITCH, then the voice and MOD. The model picks what the
exciter and MODAL-2 hold.
-}

import Layout.Shared as Shared exposing (Module, auto, fader, group, hero, knob, module_, strip)
import Protocol exposing (Addr(..))


modules : List Module
modules =
    [ resonator, exciter, modal2, Shared.pitch, Shared.filter, Shared.filterMod, Shared.voiceFx, Shared.ampOut ]
        ++ List.map Shared.envelope (List.range 1 3)
        ++ List.map Shared.lfo (List.range 1 3)
        ++ [ Shared.mod ]


resonator : Module
resonator =
    let
        m : Module
        m =
            module_ "RESONATOR" "RESONATOR"
    in
    { m
        | header = [ Addr "MODAL.MODE" ]
        , screen = strip "RES"
        , groups =
            [ group "" [ hero "MODAL.BRIGHT", knob "MODAL.STRUCTURE" ]
            , group "" [ fader "MODAL.DAMP", fader "MODAL.POS" ]
            ]
    }


exciter : Module
exciter =
    let
        m : Module
        m =
            module_ "EXCITER" "RESONATOR"
    in
    { m
        | screen = strip "EXC"
        , groups = [ group "" [ hero "MODAL.EXCITE", hero "MODAL.FORCE", fader "MODAL.COLOR", fader "MODAL.BURST", fader "MODAL.SPEED" ] ]
    }


modal2 : Module
modal2 =
    let
        m : Module
        m =
            module_ "MODAL-2" "RESONATOR"
    in
    { m
        | screen = strip "MDL2"
        , groups =
            [ group "" [ auto "MODAL.MODES" ]
            , group "BODY" [ knob "MODAL.COUPLE", fader "MODAL.HALO", fader "MODAL.BODY" ]
            , group "ENSEMBLE" [ fader "MODAL.E.DPT", fader "MODAL.E.RAT", fader "MODAL.E.MIX" ]
            ]
    }
