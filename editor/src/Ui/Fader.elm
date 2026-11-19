module Ui.Fader exposing (Config, bipolar, chars, hero, label, new, view, width)

{-| A vertical fader for levels and envelope stages: a slot, a cap and a
readout. `bipolar` marks the centre and fills from it.
-}

import Html exposing (Html, div, i)
import Html.Attributes exposing (class, classList, style)
import Protocol exposing (Addr)
import Ui.Control as Control exposing (Msg, Reading, Travel(..), Width)


type Size
    = Regular
    | Tall


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


{-| The taller fader with the bigger readout, for the level that matters.
-}
hero : Config -> Config
hero (Config c) =
    Config { c | size = Tall }


bipolar : Config -> Config
bipolar (Config c) =
    Config { c | polarity = Bipolar }


chars : Int -> Config -> Config
chars n (Config c) =
    Config { c | chars = max 1 n }


width : Config -> Width
width (Config c) =
    Control.width { px = Control.snap16 (max 44 (Control.labelPx c.label)), ch = c.chars + 1 }


view : Config -> Reading -> Html Msg
view ((Config c) as config) r =
    let
        t : Float
        t =
            Control.norm r

        at : Float -> String
        at x =
            "calc(4px + (100% - 21px) * " ++ String.fromFloat x ++ ")"
    in
    Control.cell
        [ classList [ ( "hero", c.size == Tall ), ( "bip", c.polarity == Bipolar ) ]
        , Control.syncClass (Control.sync r)
        , Control.widthStyle (width config)
        ]
        c.label
        [ div (class "fd" :: Control.slider c.label r ++ Control.grab c.addr Fader)
            (i [ class "tk" ] []
                :: (case c.polarity of
                        Unipolar ->
                            []

                        Bipolar ->
                            [ i [ class "fill", style "bottom" (at (min t 0.5)), style "top" ("calc(100% - " ++ at (max t 0.5) ++ ")") ] [] ]
                   )
                ++ [ Html.b [ class "cap", style "bottom" (at t) ] [] ]
            )
        , Control.readout c.chars r
        ]
