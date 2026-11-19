module Ui.Control exposing
    ( Chosen
    , Direction(..)
    , Drag
    , Intent(..)
    , Msg(..)
    , Option
    , Reading
    , Sync(..)
    , Target(..)
    , Travel(..)
    , Want(..)
    , Wanted
    , Width
    , ariaChecked
    , ariaLabel
    , ariaRole
    , ariaValueText
    , atNorm
    , cell
    , chosen
    , chosenSync
    , current
    , drag
    , dragIndex
    , dragNorm
    , empty
    , flush
    , grab
    , idle
    , isEmpty
    , labelPx
    , norm
    , pending
    , pendingChoice
    , reading
    , readout
    , scaleOf
    , slider
    , snap16
    , subscriptions
    , sync
    , syncClass
    , text
    , update
    , want
    , wants
    , wheelNorm
    , width
    , widthStyle
    , widthText
    )

{-| What a hand does to a control, shared by every control of the UI.

A control is handed a `Reading` (where it points, the unit's own text, and
whether an edit is waiting on the unit) or a `Chosen` option, and gives back
one `Msg` type, so a page has one `update` for every control. `update` turns
pointer travel into a normalised value and says it as an `Intent`.

The arithmetic underneath is slice 1's, unchanged: a drag or a wheel makes
many wants a frame; `Wanted` keeps the last one for each control, so at most
one `ed set` leaves per animation frame per control.

-}

import Browser.Events
import Dict exposing (Dict)
import Html exposing (Html, div)
import Html.Attributes exposing (attribute, class, classList, style, tabindex, title)
import Html.Events as Events
import Json.Decode as Decode exposing (Decoder)
import Protocol exposing (Addr, OptIndex, Scale(..), addrKey)
import Schema exposing (Param)



-- WHAT A CONTROL SHOWS


{-| Whether the value shown is the unit's, or an edit the unit has not yet
confirmed.
-}
type Sync
    = Confirmed
    | Pending


{-| A continuous control's reading: its position (0 to 1) and the unit's text.
-}
type Reading
    = Reading { norm : Float, text : String, sync : Sync }


reading : Float -> String -> Reading
reading n t =
    Reading { norm = clamp 0 1 n, text = t, sync = Confirmed }


pending : Reading -> Reading
pending (Reading r) =
    Reading { r | sync = Pending }


norm : Reading -> Float
norm (Reading r) =
    r.norm


text : Reading -> String
text (Reading r) =
    r.text


sync : Reading -> Sync
sync (Reading r) =
    r.sync


{-| A choice's reading: the option it is on.
-}
type Chosen
    = Chosen { index : OptIndex, sync : Sync }


chosen : OptIndex -> Chosen
chosen i =
    Chosen { index = i, sync = Confirmed }


pendingChoice : Chosen -> Chosen
pendingChoice (Chosen c) =
    Chosen { c | sync = Pending }


current : Chosen -> OptIndex
current (Chosen c) =
    c.index


chosenSync : Chosen -> Sync
chosenSync (Chosen c) =
    c.sync


{-| An option as the unit lists it: its number and its name.
-}
type alias Option =
    ( OptIndex, String )


syncClass : Sync -> Html.Attribute msg
syncClass s =
    classList
        [ ( "pending"
          , case s of
                Pending ->
                    True

                Confirmed ->
                    False
          )
        ]



-- THE FIXED-WIDTH CONTRACT


{-| A box's width: at least `px` pixels and `ch` characters of its own font.
Every component computes it from its `Config` alone; no `Reading` reaches it.
-}
type Width
    = Width { px : Int, ch : Int }


width : { px : Int, ch : Int } -> Width
width =
    Width


widthText : Width -> String
widthText (Width w) =
    "max(" ++ String.fromInt w.px ++ "px, " ++ String.fromInt w.ch ++ "ch)"


widthStyle : Width -> Html.Attribute msg
widthStyle w =
    style "width" (widthText w)


{-| The value's text in a box `chars` wide: tabular numerals, and text that
is longer is cut with an ellipsis, never grown.
-}
readout : Int -> Reading -> Html msg
readout chars r =
    div [ class "v", style "width" (String.fromInt chars ++ "ch"), title (text r) ] [ Html.text (text r) ]


{-| A control's box: its label above what it holds. The label is cut with
an ellipsis rather than widening the box.
-}
cell : List (Html.Attribute msg) -> String -> List (Html msg) -> Html msg
cell attrs label body =
    div (class "c" :: attrs) (div [ class "k", title label ] [ Html.text label ] :: body)


{-| Pixels a label takes in the label face (12px condensed capitals).
-}
labelPx : String -> Int
labelPx label =
    ceiling (toFloat (String.length label) * 7.2) + 4


{-| Widths step in 16px, so a module's controls sit on a grid.
-}
snap16 : Int -> Int
snap16 px =
    16 * ceiling (toFloat px / 16)


ariaRole : String -> Html.Attribute msg
ariaRole =
    attribute "role"


ariaLabel : String -> Html.Attribute msg
ariaLabel =
    attribute "aria-label"


ariaValueText : String -> Html.Attribute msg
ariaValueText =
    attribute "aria-valuetext"


ariaChecked : Bool -> Html.Attribute msg
ariaChecked on =
    attribute "aria-checked"
        (if on then
            "true"

         else
            "false"
        )


{-| What a screen reader hears of a continuous control.
-}
slider : String -> Reading -> List (Html.Attribute msg)
slider label r =
    [ ariaRole "slider"
    , ariaLabel label
    , ariaValueText (text r)
    , attribute "aria-valuemin" "0"
    , attribute "aria-valuemax" "100"
    , attribute "aria-valuenow" (String.fromInt (round (norm r * 100)))
    ]



-- WHAT A HAND DOES


{-| How far a pointer travels for the whole range: the hero knob is the one
to noodle, so it travels furthest.
-}
type Travel
    = Knob
    | Hero
    | Fader
    | Attenuverter


travelPx : Travel -> Float
travelPx t =
    case t of
        Knob ->
            150

        Hero ->
            240

        Fader ->
            60

        Attenuverter ->
            240


type Direction
    = Up
    | Down


type Msg
    = Grab Addr Travel Float
    | Moved Float
    | Released
    | Step Addr Direction
    | Pick Addr OptIndex
    | Reset Addr


{-| What a page knows of a control when a hand reaches it.
-}
type Target
    = Continuous { norm : Float, wheel : Float }
    | Discrete { options : List OptIndex, current : OptIndex }


{-| What a hand asks for. A page turns `SetNorm` into the stored value
(`atNorm`); `Restore` is the param's default.
-}
type Intent
    = SetNorm Addr Float
    | SetOption Addr OptIndex
    | Restore Addr


type Drag
    = Idle
    | Dragging { addr : Addr, travel : Travel, startY : Float, from : Target }


idle : Drag
idle =
    Idle


update : (Addr -> Maybe Target) -> Msg -> Drag -> ( Drag, List Intent )
update lookup msg held =
    case msg of
        Grab a t y ->
            case lookup a of
                Just from ->
                    ( Dragging { addr = a, travel = t, startY = y, from = from }, [] )

                Nothing ->
                    ( held, [] )

        Moved y ->
            case held of
                Dragging d ->
                    ( held
                    , case d.from of
                        Continuous c ->
                            [ SetNorm d.addr (dragNorm { px = travelPx d.travel, from = c.norm } d.startY y) ]

                        Discrete o ->
                            positionOf o.current o.options
                                |> Maybe.map (\from -> dragIndex { n = List.length o.options, from = from } d.startY y)
                                |> Maybe.andThen (\i -> at i o.options)
                                |> Maybe.map (\i -> [ SetOption d.addr i ])
                                |> Maybe.withDefault []
                    )

                Idle ->
                    ( held, [] )

        Released ->
            ( Idle, [] )

        Step a dir ->
            let
                sign : Int
                sign =
                    case dir of
                        Up ->
                            1

                        Down ->
                            -1
            in
            ( held
            , case lookup a of
                Just (Continuous c) ->
                    [ SetNorm a (clamp 0 1 (c.norm + toFloat sign * c.wheel)) ]

                Just (Discrete o) ->
                    positionOf o.current o.options
                        |> Maybe.andThen (\p -> at (clamp 0 (List.length o.options - 1) (p + sign)) o.options)
                        |> Maybe.map (\i -> [ SetOption a i ])
                        |> Maybe.withDefault []

                Nothing ->
                    []
            )

        Pick a i ->
            ( held, [ SetOption a i ] )

        Reset a ->
            ( held, [ Restore a ] )


positionOf : a -> List a -> Maybe Int
positionOf x xs =
    List.indexedMap Tuple.pair xs
        |> List.filter (\( _, y ) -> y == x)
        |> List.head
        |> Maybe.map Tuple.first


at : Int -> List a -> Maybe a
at i xs =
    List.head (List.drop i xs)


subscriptions : Drag -> Sub Msg
subscriptions held =
    case held of
        Dragging _ ->
            Sub.batch
                [ Browser.Events.onMouseMove (Decode.map Moved (Decode.field "clientY" Decode.float))
                , Browser.Events.onMouseUp (Decode.succeed Released)
                ]

        Idle ->
            Sub.none


{-| The hands on a draggable control: mouse down grabs, the wheel and the
arrow keys step, a double click resets.
-}
grab : Addr -> Travel -> List (Html.Attribute Msg)
grab a t =
    Events.onDoubleClick (Reset a) :: drag a t


{-| A drag, the wheel and the arrows, without the reset (a cable's
attenuverter unpatches on a double click instead).
-}
drag : Addr -> Travel -> List (Html.Attribute Msg)
drag a t =
    [ tabindex 0
    , Events.preventDefaultOn "mousedown" (Decode.map (\y -> ( Grab a t y, True )) (Decode.field "clientY" Decode.float))
    , Events.preventDefaultOn "wheel" (Decode.andThen (wheel a) (Decode.field "deltaY" Decode.float))
    , Events.preventDefaultOn "keydown" (Decode.andThen (arrow a) (Decode.field "key" Decode.string))
    ]


wheel : Addr -> Float -> Decoder ( Msg, Bool )
wheel a dy =
    if dy < 0 then
        Decode.succeed ( Step a Up, True )

    else if dy > 0 then
        Decode.succeed ( Step a Down, True )

    else
        Decode.fail "no roll"


arrow : Addr -> String -> Decoder ( Msg, Bool )
arrow a key =
    case key of
        "ArrowUp" ->
            Decode.succeed ( Step a Up, True )

        "ArrowRight" ->
            Decode.succeed ( Step a Up, True )

        "ArrowDown" ->
            Decode.succeed ( Step a Down, True )

        "ArrowLeft" ->
            Decode.succeed ( Step a Down, True )

        _ ->
            Decode.fail "not an arrow"



-- SLICE 1'S ARITHMETIC


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
