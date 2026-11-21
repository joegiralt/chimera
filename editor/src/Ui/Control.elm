module Ui.Control exposing
    ( Choice(..)
    , Chosen
    , Direction(..)
    , Drag
    , Intent(..)
    , Mark(..)
    , Msg(..)
    , Option
    , Options
    , Polarity
    , Reading
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
    , choice
    , choiceText
    , chosen
    , chosenMark
    , current
    , drag
    , dragIndex
    , dragNorm
    , empty
    , flush
    , grab
    , idle
    , isBipolar
    , isEmpty
    , labelPx
    , mark
    , markAttrs
    , missing
    , noValue
    , nonEmpty
    , norm
    , notInList
    , nudged
    , optionList
    , options
    , polarity
    , pxWidth
    , reading
    , readout
    , refused
    , refusedChoice
    , scaleOf
    , slider
    , snap16
    , subscriptions
    , text
    , update
    , want
    , wheelNorm
    , width
    , widthStyle
    , widthText
    )

{-| What a hand does to a control, shared by every control of the UI.

A control is handed a `Reading` (where the unit's value points and its own
text, or that the unit has not reported it, and any refusal with the unit's
reason) or a `Chosen` option, and gives back
one `Msg` type, so a page has one `update` for every control. `update` turns
pointer travel into a normalised value and says it as an `Intent`.

The arithmetic underneath is slice 1's, unchanged: a drag or a wheel makes
many wants a frame; `Wanted` keeps the last one for each control, so at most
one `ed set` leaves per animation frame per control.

-}

import Addr exposing (Addr)
import Browser.Events
import Dict exposing (Dict)
import Html exposing (Html, div)
import Html.Attributes exposing (attribute, class, classList, style, tabindex, title)
import Html.Events as Events
import Json.Decode as Decode exposing (Decoder)
import Protocol exposing (Kind(..), OptIndex, Scale(..))
import Schema exposing (Param)
import Ui.Tokens as Tokens exposing (Chars, Face(..))



-- WHAT A CONTROL SHOWS


{-| Whether the unit took the last edit: a refusal carries the unit's own
reason, until the unit next reports the value.
-}
type Mark
    = Clear
    | Refused String


{-| A continuous control's reading: where it points (0 to 1) and the unit's
text, or that the unit has not reported it yet.
-}
type Reading
    = Reading { at : At, mark : Mark }


type At
    = Known { norm : Float, text : String }
    | Missing


reading : Float -> String -> Reading
reading n t =
    Reading { at = Known { norm = clamp 0 1 n, text = t }, mark = Clear }


{-| A value the unit has not reported: the control says so, never a guess.
-}
missing : Reading
missing =
    Reading { at = Missing, mark = Clear }


refused : String -> Reading -> Reading
refused why (Reading r) =
    Reading { r | mark = Refused why }


norm : Reading -> Maybe Float
norm (Reading r) =
    case r.at of
        Known k ->
            Just k.norm

        Missing ->
            Nothing


{-| The text a readout prints: the unit's, or a dash for a value not yet
reported.
-}
text : Reading -> String
text (Reading r) =
    case r.at of
        Known k ->
            k.text

        Missing ->
            "--"


mark : Reading -> Mark
mark (Reading r) =
    r.mark


{-| Where a choice is: on one of its options, on a value the unit's list
doesn't hold, or not reported yet.
-}
type Choice
    = On OptIndex
    | NotInList
    | NoValue


{-| A choice's reading.
-}
type Chosen
    = Chosen { choice : Choice, mark : Mark }


chosen : OptIndex -> Chosen
chosen i =
    Chosen { choice = On i, mark = Clear }


notInList : Chosen
notInList =
    Chosen { choice = NotInList, mark = Clear }


noValue : Chosen
noValue =
    Chosen { choice = NoValue, mark = Clear }


refusedChoice : String -> Chosen -> Chosen
refusedChoice why (Chosen c) =
    Chosen { c | mark = Refused why }


choice : Chosen -> Choice
choice (Chosen c) =
    c.choice


{-| The option it is on, if it is on one.
-}
current : Chosen -> Maybe OptIndex
current (Chosen c) =
    case c.choice of
        On i ->
            Just i

        NotInList ->
            Nothing

        NoValue ->
            Nothing


chosenMark : Chosen -> Mark
chosenMark (Chosen c) =
    c.mark


{-| The name a choice shows: its option's, or what the unit said instead.
-}
choiceText : List Option -> Chosen -> String
choiceText opts ch =
    case choice ch of
        On i ->
            opts |> List.filter (\( k, _ ) -> k == i) |> List.head |> Maybe.map Tuple.second |> Maybe.withDefault "--"

        NotInList ->
            "??"

        NoValue ->
            "--"


{-| An option as the unit lists it: its number and its name.
-}
type alias Option =
    ( OptIndex, String )


{-| A choice's options: never none, so a list can always show one.
-}
type Options
    = Options Option (List Option)


nonEmpty : Option -> List Option -> Options
nonEmpty =
    Options


options : List Option -> Maybe Options
options opts =
    case opts of
        first :: rest ->
            Just (Options first rest)

        [] ->
            Nothing


optionList : Options -> List Option
optionList (Options first rest) =
    first :: rest


{-| A control's marks: a refusal wears its reason, which a hover and a
screen reader both get, and a small badge that takes no room.
-}
markAttrs : Mark -> List (Html.Attribute msg)
markAttrs m =
    case m of
        Clear ->
            []

        Refused why ->
            [ class "refused", title ("THE UNIT REFUSED: " ++ why), attribute "aria-description" ("the unit refused: " ++ why) ]


{-| Whether a control's range runs either side of nothing: it follows from
the unit's range alone (min below 0, max above), never a hand flag.
-}
type Polarity
    = Unipolar
    | Bipolar


polarity : { a | min : Float, max : Float } -> Polarity
polarity r =
    if r.min < 0 && r.max > 0 then
        Bipolar

    else
        Unipolar


isBipolar : Polarity -> Bool
isBipolar p =
    p == Bipolar



-- THE FIXED-WIDTH CONTRACT


{-| A box's width: at least `px` pixels and `ch` characters of its own font.
Every component computes it from its `Config` alone; no `Reading` reaches it.
-}
type Width
    = Width { px : Int, ch : Int }


{-| At least `px` pixels (never negative) and `ch` characters.
-}
width : { px : Int, ch : Chars } -> Width
width w =
    Width { px = max 0 w.px, ch = Tokens.charsCount w.ch }


{-| Pixels alone.
-}
pxWidth : Int -> Width
pxWidth px =
    Width { px = max 0 px, ch = 0 }


widthText : Width -> String
widthText (Width w) =
    "max(" ++ String.fromInt w.px ++ "px, " ++ String.fromInt w.ch ++ "ch)"


widthStyle : Width -> Html.Attribute msg
widthStyle w =
    style "width" (widthText w)


{-| The value's text in a box `chars` wide: tabular numerals, and text that
is longer is cut with an ellipsis, never grown.
-}
readout : Chars -> Reading -> Html msg
readout chars r =
    div [ classList [ ( "v", True ), ( "missing", norm r == Nothing ) ], style "width" (String.fromInt (Tokens.charsCount chars) ++ "ch"), title (text r) ] [ Html.text (text r) ]


{-| A control's box: its label above what it holds. The label is cut with
an ellipsis rather than widening the box.
-}
cell : List (Html.Attribute msg) -> String -> List (Html msg) -> Html msg
cell attrs label body =
    div (class "c" :: attrs) (div [ class "k", title label ] [ Html.text label ] :: body)


{-| Pixels a label takes in the label face.
-}
labelPx : String -> Int
labelPx label =
    Tokens.textPx LabelFace label + 4


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
    ]
        ++ (case norm r of
                Just n ->
                    [ attribute "aria-valuenow" (String.fromInt (round (n * 100))) ]

                Nothing ->
                    []
           )



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
(`atNorm`) and a wheel's `Nudge` too (`nudged`); `Restore` is the
param's default.
-}
type Intent
    = SetNorm Addr Float
    | Nudge Addr { from : Float, by : Int, wheel : Float }
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
                    [ Nudge a { from = c.norm, by = sign, wheel = c.wheel } ]

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
    | Option OptIndex


type Wanted
    = Wanted (Dict String ( Addr, Want ))


empty : Wanted
empty =
    Wanted Dict.empty


want : Addr -> Want -> Wanted -> Wanted
want a w (Wanted d) =
    Wanted (Dict.insert (Addr.key a) ( a, w ) d)


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


{-| The value at knob position `t`, 0 to 1, along the param's travel.
-}
valueAt : Param -> Float -> Float
valueAt p t =
    case scaleOf p of
        Linear ->
            p.min + t * (p.max - p.min)

        Octaves ->
            p.min * (2 ^ (t * logBase 2 (p.max / p.min)))


{-| The stored value at knob position `t`, 0 to 1, in range, kept to four
decimals so it is always a plain number the unit reads. A stepped param
lands on the multiple of its step the unit's `quantize` would store
(rounded half away from zero, from zero, not from its bottom: #1055); a
continuous one is never snapped (the box turns it by its step from
wherever it is).
-}
atNorm : Param -> Float -> Float
atNorm p t =
    let
        v : Float
        v =
            valueAt p (clamp 0 1 t)
    in
    stored p
        (case p.kind of
            Stepped ->
                onStep p v

            Cont ->
                v

            Choice ->
                v

            Toggle ->
                v
        )


{-| `v` on `p`'s nearest step, as `quantize` rounds (`roundf`); a step of
0 leaves it.
-}
onStep : Param -> Float -> Float
onStep p v =
    if p.step > 0 then
        toFloat (roundAway (v / p.step)) * p.step

    else
        v


roundAway : Float -> Int
roundAway x =
    if x >= 0 then
        floor (x + 0.5)

    else
        -(floor (-x + 0.5))


{-| `v` in range, to four decimals.
-}
stored : Param -> Float -> Float
stored p v =
    toFloat (round (clamp p.min p.max v * 10000)) / 10000


{-| One wheel notch `by` from knob position `from`: a stepped param a
step from the step it is on (the unit's norm, printed to 7 decimals, is
rounded to it first, so a notch never lands back where it was), within
range; a continuous one by `wheel` of its travel.
-}
nudged : Param -> { from : Float, by : Int, wheel : Float } -> Float
nudged p n =
    let
        notch : () -> Float
        notch () =
            atNorm p (n.from + toFloat n.by * n.wheel)
    in
    case ( p.kind, p.step > 0 ) of
        ( Stepped, True ) ->
            stored p (toFloat (roundAway (valueAt p (clamp 0 1 n.from) / p.step) + n.by) * p.step)

        ( Stepped, False ) ->
            notch ()

        ( Cont, _ ) ->
            notch ()

        ( Choice, _ ) ->
            notch ()

        ( Toggle, _ ) ->
            notch ()


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
