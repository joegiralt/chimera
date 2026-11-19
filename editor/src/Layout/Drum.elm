module Layout.Drum exposing (every, kit, pad)

{-| DRUM as the box has it, Analog Rytm Overbridge-style: the KIT view
(the twelve pads at a glance, then the whole kit's voice: VOICE FX, OUT,
FILTER MOD, ENV 1–3, LFO 1–3) and a PAD view per pad (PAD, MACHINE, FILTER,
AMP, PITCH ENV, PITCH LFO, its four routes as a patch bay, then MOD). What
is per pad on the box is per pad here.
-}

import Layout.Shared as Shared exposing (Extra(..), Module, auto, fader, group, hero, heroFader, knob, menu, module_, strip, tall)
import Protocol exposing (Addr(..))
import Ui.Pad as Pad exposing (Pad)


{-| Every module of every view.
-}
every : List Module
every =
    kit ++ List.concatMap pad Pad.all


kit : List Module
kit =
    [ pads 1, pads 7, wholeKit Shared.voiceFx, out, wholeKit filterMod ]
        ++ List.map (Shared.envelope >> wholeKit) (List.range 1 3)
        ++ List.map (Shared.lfo >> wholeKit) (List.range 1 3)


wholeKit : Module -> Module
wholeKit m =
    { m | note = "WHOLE KIT" }


{-| Six pads at a glance: level, pan, choke and retrig each. The first
holds the kit's TONAL.
-}
pads : Int -> Module
pads from =
    let
        m : Module
        m =
            module_ ("PADS " ++ String.fromInt from ++ "–" ++ String.fromInt (from + 5)) "KIT"

        row : Int -> Shared.Group
        row n =
            let
                a : String -> String
                a f =
                    "PAD" ++ String.fromInt n ++ "." ++ f
            in
            group ("PAD " ++ String.padLeft 2 '0' (String.fromInt n)) [ knob (a "LEVEL"), knob (a "PAN"), menu (a "CHOKE"), auto (a "RETRIG") ]
    in
    { m
        | screen = strip ("PAD" ++ String.fromInt from ++ "_KIT")
        , groups =
            (if from == 1 then
                [ group "" [ menu "KIT.TONAL" ] ]

             else
                []
            )
                ++ List.map row (List.range from (from + 5))
    }


out : Module
out =
    let
        m : Module
        m =
            module_ "OUT" "OUT"
    in
    { m | note = "SENDS · WHOLE KIT", screen = strip "SND", groups = [ group "" [ fader "OUT.SEND1", fader "OUT.SEND2", fader "OUT.SEND3" ] ] }


{-| The kit's filter modulation, and the STABILITY its pads' filters share.
-}
filterMod : Module
filterMod =
    let
        m : Module
        m =
            Shared.filterMod
    in
    { m | groups = m.groups ++ [ group "" [ fader "FILTER.STAB" ] ] }


pad : Pad -> List Module
pad p =
    let
        n : String
        n =
            String.fromInt (Pad.number p)

        a : String -> String -> String
        a block f =
            "PAD" ++ n ++ block ++ "." ++ f

        this : Module -> Module
        this m =
            { m | note = "PAD " ++ Pad.label p, chip = "PAD" }

        voice : Module
        voice =
            module_ "PAD" "PAD"

        machine : Module
        machine =
            module_ "MACHINE" "PAD"

        filter : Module
        filter =
            Shared.filterCore ("PAD" ++ n ++ "_FLT") ("PAD" ++ n ++ "_FLT")

        amp : Module
        amp =
            module_ "AMP" "PAD"

        penv : Module
        penv =
            module_ "PITCH ENV" "PAD"

        plfo : Module
        plfo =
            module_ "PITCH LFO" "PAD"

        routes : Module
        routes =
            module_ "ROUTE 1–4" "PAD"
    in
    List.map this
        [ { voice
            | screen = strip ("PAD" ++ n ++ "_KIT")
            , groups = [ group "" [ heroFader (a "" "LEVEL"), knob (a "" "PAN") ], group "KEYS" [ auto (a "" "CHOKE"), auto (a "" "RETRIG") ] ]
          }
        , { machine
            | header = [ Addr (a "" "MACHINE") ]
            , screen = strip ("PAD" ++ n ++ "_MCH")
            , groups =
                [ group "A–F" [ hero (a "" "KNOB_A"), knob (a "" "KNOB_B"), knob (a "" "KNOB_C"), knob (a "" "KNOB_D"), knob (a "" "KNOB_E"), knob (a "" "KNOB_F") ]
                , group "MORE" [ knob (a "" "KNOB_G"), knob (a "" "KNOB_H") ]
                ]
          }
        , filter
        , { amp
            | screen = tall ("PAD" ++ n ++ "_AMP")
            , groups =
                [ group "" [ fader (a "_AMP" "ATTACK"), fader (a "_AMP" "HOLD"), fader (a "_AMP" "DECAY") ]
                , group "" [ auto (a "_AMP" "SPEED"), auto (a "_AMP" "MODE"), fader (a "_AMP" "VEL") ]
                , group "GATE" [ fader (a "_AMP" "LPG"), fader (a "_AMP" "GATE_DECAY"), fader (a "_AMP" "STRIKE") ]
                ]
          }
        , { penv
            | screen = tall ("PAD" ++ n ++ "_PENV")
            , groups = [ group "" [ fader (a "_ENV" "ATTACK"), fader (a "_ENV" "HOLD"), fader (a "_ENV" "DECAY") ], group "" [ auto (a "_ENV" "SPEED") ] ]
          }
        , { plfo
            | screen = strip ("PAD" ++ n ++ "_PLFO")
            , groups = [ group "" [ hero (a "_LFO" "SPEED"), auto (a "_LFO" "WAVE") ], group "" [ fader (a "_LFO" "DEPTH"), knob (a "_LFO" "PHASE"), auto (a "_LFO" "MODE") ] ]
          }
        , { routes | full = True, extras = [ PadRoutes p ] }
        ]
        ++ [ Shared.mod ]
