module Addr exposing
    ( Addr
    , Block(..)
    , Env
    , Field
    , Lfo
    , Operator
    , Osc(..)
    , PadBlock(..)
    , RouteSlot
    , at
    , envNumber
    , envs
    , field
    , fromWire
    , key
    , lfoNumber
    , lfos
    , onWire
    , operatorNumber
    , operators
    , oscNumber
    , routeSlots
    , vaOsc
    )

{-| A parameter's address, `OP_C.CRSE`. Only two things make one: the unit's
own text, read once off the wire (`fromWire`), and the typed builders a
layout uses (`at`). A layout never writes an address as a string.
-}

import Ui.Pad as Pad exposing (Pad)


type Addr
    = Addr String


{-| The unit's text for an address: one word with a dot in it, and never a
space or a control character, so it can never split a command in two.
-}
fromWire : String -> Maybe Addr
fromWire s =
    if String.contains "." s && not (String.startsWith "." s) && not (String.endsWith "." s) && String.all plain s then
        Just (Addr s)

    else
        Nothing


plain : Char -> Bool
plain c =
    Char.toCode c > 32 && Char.toCode c /= 127 && c /= '"'


{-| The address is one the unit could write: a builder given a word the
unit has none of makes one that is not.
-}
onWire : Addr -> Bool
onWire (Addr a) =
    fromWire a /= Nothing


{-| The address as the unit writes it.
-}
key : Addr -> String
key (Addr a) =
    a



-- THE BUILDERS


{-| A field of a block: `CUTOFF`, `E.DPT`. The unit's own word.
-}
type Field
    = Field String


{-| The unit's words are capitals, digits, `_` and inner dots. Anything else
is a field the unit has none of: its address is never a wire address, so
`Protocol.encode` sends nothing for it.
-}
field : String -> Field
field w =
    if w /= "" && not (String.startsWith "." w) && not (String.endsWith "." w) && String.all unitChar w then
        Field w

    else
        Field " "


unitChar : Char -> Bool
unitChar c =
    Char.isUpper c || Char.isDigit c || c == '_' || c == '.'


type Env
    = Env1
    | Env2
    | Env3


envs : List Env
envs =
    [ Env1, Env2, Env3 ]


envNumber : Env -> Int
envNumber e =
    case e of
        Env1 ->
            1

        Env2 ->
            2

        Env3 ->
            3


type Lfo
    = Lfo1
    | Lfo2
    | Lfo3


lfos : List Lfo
lfos =
    [ Lfo1, Lfo2, Lfo3 ]


lfoNumber : Lfo -> Int
lfoNumber l =
    case l of
        Lfo1 ->
            1

        Lfo2 ->
            2

        Lfo3 ->
            3


{-| ALGO's six operators, the unit's OP\_A to OP\_F.
-}
type Operator
    = OpA
    | OpB
    | OpC
    | OpD
    | OpE
    | OpF


operators : List Operator
operators =
    [ OpA, OpB, OpC, OpD, OpE, OpF ]


operatorNumber : Operator -> Int
operatorNumber o =
    case o of
        OpA ->
            1

        OpB ->
            2

        OpC ->
            3

        OpD ->
            4

        OpE ->
            5

        OpF ->
            6


operatorLetter : Operator -> String
operatorLetter o =
    case o of
        OpA ->
            "A"

        OpB ->
            "B"

        OpC ->
            "C"

        OpD ->
            "D"

        OpE ->
            "E"

        OpF ->
            "F"


{-| VA's two oscillators.
-}
type Osc
    = Osc1
    | Osc2


oscNumber : Osc -> Int
oscNumber o =
    case o of
        Osc1 ->
            1

        Osc2 ->
            2


{-| A DRUM pad's four routes.
-}
type RouteSlot
    = Rt1
    | Rt2
    | Rt3
    | Rt4


routeSlots : List RouteSlot
routeSlots =
    [ Rt1, Rt2, Rt3, Rt4 ]


routeNumber : RouteSlot -> Int
routeNumber r =
    case r of
        Rt1 ->
            1

        Rt2 ->
            2

        Rt3 ->
            3

        Rt4 ->
            4


{-| The parts of a DRUM pad, each its own block on the unit.
-}
type PadBlock
    = Voice
    | Amp
    | PitchEnv
    | PitchLfo
    | PadFilter
    | PadRoute RouteSlot


{-| A block of addresses, as the unit names them.
-}
type Block
    = Pitch
    | Filter
    | VoiceFx
    | Out
    | EnvBlock Env
    | LfoBlock Lfo
    | Algo
    | Op Operator
    | Va
    | Modal
    | Tape
    | Kit
    | Pad Pad PadBlock


blockName : Block -> String
blockName b =
    case b of
        Pitch ->
            "PITCH"

        Filter ->
            "FILTER"

        VoiceFx ->
            "VOICEFX"

        Out ->
            "OUT"

        EnvBlock e ->
            "ENV" ++ String.fromInt (envNumber e)

        LfoBlock l ->
            "LFO" ++ String.fromInt (lfoNumber l)

        Algo ->
            "ALGO"

        Op o ->
            "OP_" ++ operatorLetter o

        Va ->
            "VA"

        Modal ->
            "MODAL"

        Tape ->
            "TAPE_ENG"

        Kit ->
            "KIT"

        Pad p part ->
            "PAD" ++ String.fromInt (Pad.number p) ++ padSuffix part


padSuffix : PadBlock -> String
padSuffix part =
    case part of
        Voice ->
            ""

        Amp ->
            "_AMP"

        PitchEnv ->
            "_ENV"

        PitchLfo ->
            "_LFO"

        PadFilter ->
            "_FLT"

        PadRoute r ->
            "_RT" ++ String.fromInt (routeNumber r)


at : Block -> Field -> Addr
at b (Field f) =
    Addr (blockName b ++ "." ++ f)


{-| A VA oscillator's own field: the unit numbers the field, `VA.WAVE1`.
-}
vaOsc : Osc -> Field -> Addr
vaOsc o (Field f) =
    Addr ("VA." ++ f ++ String.fromInt (oscNumber o))
