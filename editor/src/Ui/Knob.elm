module Ui.Knob exposing (Config, Fill(..), bipolar, chars, face, hero, label, new, view, width)

{-| A rotary control: ticks, a pointer and a readout. `hero` is the one big
knob of a page; `bipolar` fills from the centre.
-}

import Html exposing (Html, div)
import Html.Attributes exposing (class, classList, style)
import Protocol exposing (Addr)
import Svg exposing (Svg)
import Svg.Attributes as S
import Ui.Control as Control exposing (Msg, Reading, Travel(..), Width)
import Ui.Tokens as Tokens exposing (Token(..))


type Size
    = Regular
    | Big


type Polarity
    = Unipolar
    | Bipolar


type Config
    = Config
        { addr : Addr
        , label : String
        , size : Size
        , polarity : Polarity
        , chars : Int
        }


new : Addr -> Config
new a =
    Config { addr = a, label = "", size = Regular, polarity = Unipolar, chars = 8 }


label : String -> Config -> Config
label l (Config c) =
    Config { c | label = l }


hero : Config -> Config
hero (Config c) =
    Config { c | size = Big }


bipolar : Config -> Config
bipolar (Config c) =
    Config { c | polarity = Bipolar }


{-| The readout's width in characters (default 8, for `1.20 kHz`).
-}
chars : Int -> Config -> Config
chars n (Config c) =
    Config { c | chars = max 1 n }


diameter : Size -> Int
diameter s =
    case s of
        Regular ->
            44

        Big ->
            66


width : Config -> Width
width (Config c) =
    Control.width
        { px = Control.snap16 (max (diameter c.size + 8) (Control.labelPx c.label))
        , ch = c.chars + 1
        }


view : Config -> Reading -> Html Msg
view ((Config c) as config) r =
    let
        d : String
        d =
            String.fromInt (diameter c.size) ++ "px"

        travel : Travel
        travel =
            case c.size of
                Regular ->
                    Knob

                Big ->
                    Hero
    in
    Control.cell
        [ classList [ ( "hero", c.size == Big ), ( "bip", c.polarity == Bipolar ) ]
        , Control.syncClass (Control.sync r)
        , Control.widthStyle (width config)
        ]
        c.label
        [ div (class "kn" :: style "width" d :: style "height" d :: Control.slider c.label r ++ Control.grab c.addr travel)
            [ face (polarityFill c.polarity) (Control.norm r) ]
        , Control.readout c.chars r
        ]


type Fill
    = NoFill
    | FromCentre


polarityFill : Polarity -> Fill
polarityFill p =
    case p of
        Unipolar ->
            NoFill

        Bipolar ->
            FromCentre


{-| The drawn knob, 100 units square: eleven ticks, the body, the pointer at
`t` (0 to 1 over 270 degrees). Shared with the selector's small knob.
-}
face : Fill -> Float -> Svg msg
face fill t =
    Svg.svg [ S.viewBox "0 0 100 100" ]
        (List.map tick (List.range 0 10)
            ++ [ Svg.circle [ S.cx "50", S.cy "50", S.r "33", S.style ("fill:" ++ Tokens.var KnobBody) ] []
               , Svg.circle [ S.cx "50", S.cy "50", S.r "26", S.style ("fill:" ++ Tokens.var KnobFace) ] []
               ]
            ++ (case fill of
                    NoFill ->
                        []

                    FromCentre ->
                        [ Svg.path [ S.d (arc 0.5 t), S.fill "none", S.strokeWidth "6", S.style ("stroke:" ++ Tokens.var Acc) ] [] ]
               )
            ++ [ Svg.line
                    [ S.x1 "50"
                    , S.y1 "50"
                    , S.x2 "50"
                    , S.y2 "20"
                    , S.strokeWidth "6"
                    , S.strokeLinecap "round"
                    , S.style ("stroke:" ++ Tokens.var KnobPointer)
                    , S.transform ("rotate(" ++ String.fromFloat (-135 + 270 * t) ++ " 50 50)")
                    ]
                    []
               ]
        )


tick : Int -> Svg msg
tick i =
    let
        a : Float
        a =
            degrees (-135 + 27 * toFloat i)

        inner : Float
        inner =
            if modBy 5 i == 0 then
                38

            else
                41
    in
    Svg.line
        [ S.x1 (String.fromFloat (50 + 47 * sin a))
        , S.y1 (String.fromFloat (50 - 47 * cos a))
        , S.x2 (String.fromFloat (50 + inner * sin a))
        , S.y2 (String.fromFloat (50 - inner * cos a))
        , S.strokeWidth
            (if modBy 5 i == 0 then
                "3.5"

             else
                "2"
            )
        , S.style ("stroke:" ++ Tokens.var Ink)
        ]
        []


{-| The arc on the tick ring from `t0` to `t1`.
-}
arc : Float -> Float -> String
arc t0 t1 =
    let
        point : Float -> ( Float, Float )
        point t =
            ( 50 + 44 * sin (degrees (-135 + 270 * t)), 50 - 44 * cos (degrees (-135 + 270 * t)) )

        ( x0, y0 ) =
            point t0

        ( x1, y1 ) =
            point t1

        n : Float -> String
        n =
            String.fromFloat
    in
    "M"
        ++ n x0
        ++ " "
        ++ n y0
        ++ "A44 44 0 0 "
        ++ (if t1 > t0 then
                "1"

            else
                "0"
           )
        ++ " "
        ++ n x1
        ++ " "
        ++ n y1
