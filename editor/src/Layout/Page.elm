module Layout.Page exposing (PadPage(..), UnitPage(..), label, pad, resolve, variantSelector)

{-| The unit's pages a layout draws from, by name: a layout never writes a
page's label as a string, and `resolve` finds its number in the schema the
unit described (a test checks every layout's pages resolve).
-}

import Addr exposing (Addr, Block(..), Env, Lfo, Operator, Osc, PadBlock(..))
import Protocol exposing (PageNo)
import Schema exposing (Schema)
import Ui.Pad as Pad exposing (Pad)


type UnitPage
    = Alg
    | Pit
    | OpPage Operator
    | Flt
    | Fx
    | Snd
    | EnvPage Env
    | LfoPage Lfo
    | VaPage
    | OscPage Osc
    | Noise
    | Res
    | Exc
    | Mdl2
    | Src
    | Deck
    | Mch
    | PadPage Pad PadPage


{-| A DRUM pad's own pages.
-}
type PadPage
    = PadKit
    | PadMch
    | PadFlt
    | PadAmp
    | PadPenv
    | PadPlfo
    | PadFx


{-| The page's label as `ed describe` gives it.
-}
label : UnitPage -> String
label p =
    case p of
        Alg ->
            "ALG"

        Pit ->
            "PIT"

        OpPage o ->
            "OP" ++ String.fromInt (Addr.operatorNumber o)

        Flt ->
            "FLT"

        Fx ->
            "FX"

        Snd ->
            "SND"

        EnvPage e ->
            "E" ++ String.fromInt (Addr.envNumber e)

        LfoPage l ->
            "L" ++ String.fromInt (Addr.lfoNumber l)

        VaPage ->
            "VA"

        OscPage o ->
            "OSC" ++ String.fromInt (Addr.oscNumber o)

        Noise ->
            "NOISE"

        Res ->
            "RES"

        Exc ->
            "EXC"

        Mdl2 ->
            "MDL2"

        Src ->
            "SRC"

        Deck ->
            "DECK"

        Mch ->
            "MCH"

        PadPage n pp ->
            "PAD" ++ String.fromInt (Pad.number n) ++ padSuffix pp


padSuffix : PadPage -> String
padSuffix pp =
    case pp of
        PadKit ->
            "_KIT"

        PadMch ->
            "_MCH"

        PadFlt ->
            "_FLT"

        PadAmp ->
            "_AMP"

        PadPenv ->
            "_PENV"

        PadPlfo ->
            "_PLFO"

        PadFx ->
            "_FX"


{-| The page's number in the schema the unit described, if it has the page.
-}
resolve : UnitPage -> Schema -> Maybe PageNo
resolve p =
    Schema.pageNumber (label p)


{-| The DRUM pad a page draws for: the unit selects it before drawing.
-}
pad : UnitPage -> Maybe Pad
pad p =
    case p of
        PadPage n _ ->
            Just n

        Alg ->
            Nothing

        Pit ->
            Nothing

        OpPage _ ->
            Nothing

        Flt ->
            Nothing

        Fx ->
            Nothing

        Snd ->
            Nothing

        EnvPage _ ->
            Nothing

        LfoPage _ ->
            Nothing

        VaPage ->
            Nothing

        OscPage _ ->
            Nothing

        Noise ->
            Nothing

        Res ->
            Nothing

        Exc ->
            Nothing

        Mdl2 ->
            Nothing

        Src ->
            Nothing

        Deck ->
            Nothing

        Mch ->
            Nothing


{-| The address whose value picks a panel's variant (`panel FILTER LADDER`
is FILTER.KIND's doing), by the panel's name as the unit gives it.
-}
variantSelector : String -> Maybe Addr
variantSelector name =
    let
        numbered : String -> List a -> Maybe a
        numbered prefix xs =
            if String.startsWith prefix name then
                String.dropLeft (String.length prefix) name
                    |> String.toInt
                    |> Maybe.andThen
                        (\n ->
                            if n >= 1 then
                                List.head (List.drop (n - 1) xs)

                            else
                                Nothing
                        )

            else
                Nothing

        padOf : String -> Maybe Pad
        padOf suffix =
            if String.startsWith "PAD" name && String.endsWith suffix name then
                name |> String.dropLeft 3 |> String.dropRight (String.length suffix) |> String.toInt |> Maybe.andThen (\n -> Pad.all |> List.filter (\q -> Pad.number q == n) |> List.head)

            else
                Nothing

        f : String -> Addr.Field
        f =
            Addr.field
    in
    case name of
        "FILTER" ->
            Just (Addr.at Filter (f "KIND"))

        "FX" ->
            Just (Addr.at VoiceFx (f "TYPE"))

        "RESONATOR" ->
            Just (Addr.at Modal (f "MODE"))

        "MACHINE" ->
            Just (Addr.at Tape (f "MODEL"))

        _ ->
            [ numbered "ENV" Addr.envs |> Maybe.map (\e -> Addr.at (EnvBlock e) (f "TYPE"))
            , numbered "LFO" Addr.lfos |> Maybe.map (\l -> Addr.at (LfoBlock l) (f "TYPE"))
            , numbered "OP" Addr.operators |> Maybe.map (\o -> Addr.at (Op o) (f "WAVE"))
            , padOf "_MCH" |> Maybe.map (\p -> Addr.at (Pad p Voice) (f "MACHINE"))
            , padOf "_FLT" |> Maybe.map (\p -> Addr.at (Pad p PadFilter) (f "KIND"))
            ]
                |> List.filterMap identity
                |> List.head
