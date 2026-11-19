module Ui.WordRow exposing (Config, label, new, view, width)

{-| A row of words with the active one lit: the settings sheet, and header
type menus.
-}

import Addr exposing (Addr)
import Html exposing (Html, button, div, text)
import Html.Attributes exposing (class, classList, style)
import Html.Events exposing (onClick)
import Ui.Control as Control exposing (Chosen, Msg(..), Option, Options, Width)
import Ui.Tokens as Tokens exposing (Face(..))


type Config
    = Config { addr : Addr, label : String, options : List Option }


{-| A row from the unit's options, of which there is always one.
-}
new : Addr -> Options -> Config
new a opts =
    Config { addr = a, label = "", options = Control.optionList opts }


label : String -> Config -> Config
label l (Config c) =
    Config { c | label = l }


wordPx : String -> Int
wordPx w =
    Tokens.textPx WordFace w + 24


width : Config -> Width
width (Config c) =
    let
        words : Int
        words =
            List.sum (List.map (Tuple.second >> wordPx) c.options) + 4 * (List.length c.options - 1)
    in
    Control.pxWidth (max (Control.labelPx c.label) words)


view : Config -> Chosen -> Html Msg
view ((Config c) as config) ch =
    Control.cell (class "wrc" :: Control.widthStyle (width config) :: Control.markAttrs (Control.chosenMark ch))
        c.label
        [ div [ class "words", Control.ariaRole "radiogroup", Control.ariaLabel c.label ]
            (List.map
                (\( i, w ) ->
                    button
                        [ classList [ ( "on", Just i == Control.current ch ) ]
                        , Control.ariaRole "radio"
                        , Control.ariaChecked (Just i == Control.current ch)
                        , style "width" (String.fromInt (wordPx w) ++ "px")
                        , onClick (Pick c.addr i)
                        ]
                        [ text w ]
                )
                c.options
            )
        ]
