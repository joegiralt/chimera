module Ui.Pad exposing (Pad(..), all, label, number)

{-| DRUM's twelve pads, as the unit numbers them: never a bare Int.
-}


type Pad
    = Pad1
    | Pad2
    | Pad3
    | Pad4
    | Pad5
    | Pad6
    | Pad7
    | Pad8
    | Pad9
    | Pad10
    | Pad11
    | Pad12


all : List Pad
all =
    [ Pad1, Pad2, Pad3, Pad4, Pad5, Pad6, Pad7, Pad8, Pad9, Pad10, Pad11, Pad12 ]


number : Pad -> Int
number p =
    case p of
        Pad1 ->
            1

        Pad2 ->
            2

        Pad3 ->
            3

        Pad4 ->
            4

        Pad5 ->
            5

        Pad6 ->
            6

        Pad7 ->
            7

        Pad8 ->
            8

        Pad9 ->
            9

        Pad10 ->
            10

        Pad11 ->
            11

        Pad12 ->
            12


{-| The pad's number as the unit prints it: `01`.
-}
label : Pad -> String
label p =
    String.padLeft 2 '0' (String.fromInt (number p))
