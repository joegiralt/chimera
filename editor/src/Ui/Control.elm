module Ui.Control exposing
    ( Want(..)
    , Wanted
    , atNorm
    , dragIndex
    , dragNorm
    , empty
    , flush
    , isEmpty
    , scaleOf
    , want
    , wants
    , wheelNorm
    )

{-| What a hand does to a control, as pure arithmetic. A drag or a wheel
makes many wants a frame; `Wanted` keeps the last one for each control, so
at most one `ed set` leaves per animation frame per control.
-}

import Dict exposing (Dict)
import Protocol exposing (Addr, Scale(..), addrKey)
import Schema exposing (Param)


type Want
    = Number Float
    | Option Int


type Wanted
    = Wanted (Dict String ( Addr, Want ))


empty : Wanted
empty =
    Wanted Dict.empty


want : Addr -> Want -> Wanted -> Wanted
want a w (Wanted d) =
    Wanted (Dict.insert (addrKey a) ( a, w ) d)


{-| The wants to send this frame, one per control in address order, and what
is left (nothing).
-}
flush : Wanted -> ( List ( Addr, Want ), Wanted )
flush (Wanted d) =
    ( Dict.values d, empty )



-- WHERE A TURN LANDS


{-| How the param moves under a hand: the unit says (`describe`'s scale).
-}
scaleOf : Param -> Scale
scaleOf p =
    p.scale


{-| The stored value at knob position `t`, 0 to 1, kept to four decimals so
it is always a plain number the unit reads. The unit quantises it.
-}
atNorm : Param -> Float -> Float
atNorm p t0 =
    let
        t : Float
        t =
            clamp 0 1 t0

        v : Float
        v =
            case scaleOf p of
                Linear ->
                    p.min + t * (p.max - p.min)

                Octaves ->
                    p.min * (2 ^ (t * logBase 2 (p.max / p.min)))
    in
    toFloat (round (v * 10000)) / 10000


{-| A position moved by dragging from `startY` to `y`: up raises it, `px`
pixels for the whole range.
-}
dragNorm : { px : Float, from : Float } -> Float -> Float -> Float
dragNorm { px, from } startY y =
    clamp 0 1 (from + (startY - y) / px)


{-| A choice of `n` options steps as the pointer travels, `180 / n` pixels
each, at least five.
-}
dragIndex : { n : Int, from : Int } -> Float -> Float -> Int
dragIndex { n, from } startY y =
    clamp 0 (max 0 (n - 1)) (from + round ((startY - y) / max 5 (180 / toFloat n)))


{-| One wheel notch: a step of the param, but never under 1/64 of the travel.
-}
wheelNorm : Param -> Float
wheelNorm p =
    let
        travel : Float
        travel =
            case scaleOf p of
                Linear ->
                    p.max - p.min

                Octaves ->
                    logBase 2 (p.max / p.min)
    in
    if travel <= 0 then
        1 / 64

    else
        max (1 / 64) (p.step / travel)


isEmpty : Wanted -> Bool
isEmpty (Wanted d) =
    Dict.isEmpty d


wants : Addr -> Wanted -> Bool
wants a (Wanted d) =
    Dict.member (addrKey a) d
