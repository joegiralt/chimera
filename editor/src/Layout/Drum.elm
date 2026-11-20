module Layout.Drum exposing (every, kit, pad)

{-| DRUM as the box has it, Analog Rytm Overbridge-style: the KIT view
(the twelve pads at a glance, then the whole kit's voice: VOICE FX, OUT,
FILTER MOD, ENV 1–3, LFO 1–3) and a PAD view per pad (PAD, MACHINE, FILTER,
AMP, PITCH ENV, PITCH LFO, its four routes as a patch bay, then MOD). What
is per pad on the box is per pad here.
-}

import Addr exposing (Addr, Block(..), PadBlock(..))
import Layout.Page exposing (PadPage(..), UnitPage(..))
import Layout.Shared as Shared exposing (Extra(..), Module, auto, fader, group, hero, heroFader, knob, menu, module_, strip, tall)
import Ui.Chip as Chip
import Ui.Pad as Pad exposing (Pad(..))


{-| Every module of every view.
-}
every : List Module
every =
    kit ++ List.concatMap pad Pad.all


kit : List Module
kit =
    [ pads ( Pad1, [ Pad2, Pad3, Pad4, Pad5, Pad6 ] ), pads ( Pad7, [ Pad8, Pad9, Pad10, Pad11, Pad12 ] ), wholeKit (Shared.voiceFx (PadPage Pad1 PadFx)), out, wholeKit filterMod ]
        ++ List.map (Shared.envelope >> wholeKit) Addr.envs
        ++ List.map (Shared.lfo >> wholeKit) Addr.lfos


wholeKit : Module -> Module
wholeKit =
    Shared.withNote "WHOLE KIT"


{-| Six pads at a glance: level, pan, choke and retrig each, the first's
kit band on top. The first six hold the kit's TONAL.
-}
pads : ( Pad, List Pad ) -> Module
pads ( first, rest ) =
    let
        row : Pad -> Shared.Group
        row p =
            let
                a : String -> Addr
                a =
                    Addr.field >> Addr.at (Pad p Voice)
            in
            group ("PAD " ++ Pad.label p) [ knob (a "LEVEL"), knob (a "PAN"), menu (a "CHOKE"), auto (a "RETRIG") ]

        last : Pad
        last =
            List.foldl always first rest
    in
    module_ Chip.Kit ("PADS " ++ String.fromInt (Pad.number first) ++ "–" ++ String.fromInt (Pad.number last))
        |> Shared.withScreen (strip (PadPage first PadKit))
        |> Shared.withGroups
            ((if first == Pad1 then
                [ group "" [ menu (Addr.at Kit (Addr.field "TONAL")) ] ]

              else
                []
             )
                ++ List.map row (first :: rest)
            )


out : Module
out =
    let
        a : String -> Addr
        a =
            Addr.field >> Addr.at Out
    in
    module_ Chip.Out "OUT"
        |> Shared.withNote "SENDS · WIDTH · WHOLE KIT"
        |> Shared.withScreen (strip Snd)
        |> Shared.withGroups [ group "" [ fader (a "SEND1"), fader (a "SEND2"), fader (a "SEND3") ], group "" [ fader (a "WIDTH") ] ]


{-| The kit's filter modulation, and the STABILITY its pads' filters share.
-}
filterMod : Module
filterMod =
    Shared.filterMod |> Shared.addGroups [ group "" [ fader (Addr.at Filter (Addr.field "STAB")) ] ]


pad : Pad -> List Module
pad p =
    let
        a : PadBlock -> String -> Addr
        a block =
            Addr.field >> Addr.at (Pad p block)

        this : Module -> Module
        this =
            Shared.withNote ("PAD " ++ Pad.label p) >> Shared.withChip Chip.Pad

        on : PadPage -> UnitPage
        on =
            PadPage p
    in
    List.map this
        [ module_ Chip.Pad "PAD"
            |> Shared.withScreen (strip (on PadKit))
            |> Shared.withGroups [ group "" [ heroFader (a Voice "LEVEL"), knob (a Voice "PAN") ], group "KEYS" [ auto (a Voice "CHOKE"), auto (a Voice "RETRIG") ] ]
        , module_ Chip.Pad "MACHINE"
            |> Shared.withHeader [ a Voice "MACHINE" ]
            |> Shared.withScreen (strip (on PadMch))
            |> Shared.withGroups
                [ group "A–F" [ hero (a Voice "KNOB_A"), knob (a Voice "KNOB_B"), knob (a Voice "KNOB_C"), knob (a Voice "KNOB_D"), knob (a Voice "KNOB_E"), knob (a Voice "KNOB_F") ]
                , group "MORE" [ knob (a Voice "KNOB_G"), knob (a Voice "KNOB_H") ]
                ]
        , Shared.filterCore (Pad p PadFilter) (on PadFlt)
        , module_ Chip.Pad "AMP"
            |> Shared.withScreen (tall (on PadAmp))
            |> Shared.withGroups
                [ group "" [ fader (a Amp "ATTACK"), fader (a Amp "HOLD"), fader (a Amp "DECAY") ]
                , group "" [ auto (a Amp "SPEED"), auto (a Amp "MODE"), fader (a Amp "VEL") ]
                , group "GATE" [ fader (a Amp "LPG"), fader (a Amp "GATE_DECAY"), fader (a Amp "STRIKE") ]
                ]
        , module_ Chip.Pad "PITCH ENV"
            |> Shared.withScreen (tall (on PadPenv))
            |> Shared.withGroups [ group "" [ fader (a PitchEnv "ATTACK"), fader (a PitchEnv "HOLD"), fader (a PitchEnv "DECAY") ], group "" [ auto (a PitchEnv "SPEED") ] ]
        , module_ Chip.Pad "PITCH LFO"
            |> Shared.withScreen (strip (on PadPlfo))
            |> Shared.withGroups [ group "" [ hero (a PitchLfo "SPEED"), auto (a PitchLfo "WAVE") ], group "" [ fader (a PitchLfo "DEPTH"), knob (a PitchLfo "PHASE"), auto (a PitchLfo "MODE") ] ]

        -- Half a row, beside nothing: whether it takes the whole row waits on the owner.
        , module_ Chip.Pad "ROUTE 1–4" |> Shared.withExtras [ PadRoutes p ]
        ]
        ++ [ Shared.mod ]
