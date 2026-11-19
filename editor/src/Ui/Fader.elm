module Ui.Fader exposing (Config, chars, hero, label, new, view, width)

{-| A vertical fader for levels and envelope stages: a slot, a cap and a
readout. `bipolar` marks the centre and fills from it.
-}

import Addr exposing (Addr)
import Html exposing (Html, div, i)
import Html.Attributes exposing (class, classList, style)
import Ui.Control as Control exposing (Msg, Polarity, Reading, Travel(..), Width)
import Ui.Tokens as Tokens exposing (Chars)


type Size
    = Regular
    | Tall


type Config
    = Config
        { addr : Addr
        , label : String
        , size : Size
        , polarity : Polarity
        , chars : Chars
        }


{-| A control for the param at `a`: its polarity is the param's range's
(`Control.polarity`), so a bipolar one fills from its centre.
-}
new : Addr -> Polarity -> Config
new a p =
    Config { addr = a, label = "", size = Regular, polarity = p, chars = Tokens.readoutChars }


label : String -> Config -> Config
label l (Config c) =
    Config { c | label = l }


{-| The taller fader with the bigger readout, for the level that matters.
-}
hero : Config -> Config
hero (Config c) =
    Config { c | size = Tall }


chars : Chars -> Config -> Config
chars n (Config c) =
    Config { c | chars = n }


width : Config -> Width
width (Config c) =
    Control.width { px = Control.snap16 (max 44 (Control.labelPx c.label)), ch = Tokens.oneMore c.chars }


view : Config -> Reading -> Html Msg
view ((Config c) as config) r =
    let
        at : Float -> String
        at x =
            "calc(4px + (100% - 21px) * " ++ String.fromFloat x ++ ")"
    in
    Control.cell
        ([ classList [ ( "hero", c.size == Tall ), ( "bip", Control.isBipolar c.polarity ) ]
         , classList [ ( "missing", Control.norm r == Nothing ) ]
         , Control.widthStyle (width config)
         ]
            ++ Control.markAttrs (Control.mark r)
        )
        c.label
        [ div (class "fd" :: Control.slider c.label r ++ Control.grab c.addr Fader)
            (i [ class "tk" ] []
                -- A value not yet reported draws no cap (the cell says `missing`).
                :: (case Control.norm r of
                        Just t ->
                            (if Control.isBipolar c.polarity then
                                [ i [ class "fill", style "bottom" (at (min t 0.5)), style "top" ("calc(100% - " ++ at (max t 0.5) ++ ")") ] [] ]

                             else
                                []
                            )
                                ++ [ Html.b [ class "cap", style "bottom" (at t) ] [] ]

                        Nothing ->
                            []
                   )
            )
        , Control.readout c.chars r
        ]
