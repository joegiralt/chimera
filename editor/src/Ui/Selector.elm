module Ui.Selector exposing (Config, fromOptions, label, view, width)

{-| Four to eight choices printed around a small selector knob, as on a
panel. A printed word picks its position; the knob drags and wheels.
-}

import Addr exposing (Addr)
import Html exposing (Html, button, div, text)
import Html.Attributes exposing (class, classList, style)
import Html.Events exposing (onClick)
import Ui.Control as Control exposing (Chosen, Msg(..), Option, Travel(..), Width)
import Ui.Knob as Knob
import Ui.Tokens as Tokens exposing (Face(..))


type Config
    = Config { addr : Addr, label : String, options : List Option }


{-| A selector from the unit's options: only four to eight make one.
-}
fromOptions : Addr -> List Option -> Maybe Config
fromOptions a opts =
    if List.length opts >= 4 && List.length opts <= 8 then
        Just (Config { addr = a, label = "", options = opts })

    else
        Nothing


label : String -> Config -> Config
label l (Config c) =
    Config { c | label = l }


type alias Geometry =
    { radius : Float, w : Float, h : Float, span : Float }


geometry : List Option -> Geometry
geometry opts =
    let
        n : Int
        n =
            List.length opts

        radius : Float
        radius =
            if n > 5 then
                50

            else
                40

        widest : Float
        widest =
            opts |> List.map (Tuple.second >> Tokens.textPx SelectorFace) |> List.maximum |> Maybe.withDefault 0 |> toFloat
    in
    { radius = radius
    , w = 2 * (radius + (widest + 8) / 2) + 4
    , h = 2 * radius + 18
    , span =
        if n > 5 then
            270

        else
            min 240 (60 * toFloat (n - 1))
    }


width : Config -> Width
width (Config c) =
    Control.pxWidth (Control.snap16 (max (Control.labelPx c.label) (ceiling (geometry c.options).w)))


view : Config -> Chosen -> Html Msg
view ((Config c) as config) ch =
    let
        g : Geometry
        g =
            geometry c.options

        n : Int
        n =
            List.length c.options

        angle : Int -> Float
        angle i =
            -g.span / 2 + g.span * toFloat i / toFloat (max 1 (n - 1))

        -- Not on any position (not reported, or a value the list lacks):
        -- the knob has no pointer, and the cell says which.
        here : Maybe Int
        here =
            c.options
                |> List.indexedMap Tuple.pair
                |> List.filter (\( _, ( i, _ ) ) -> Just i == Control.current ch)
                |> List.head
                |> Maybe.map Tuple.first

        cx : Float
        cx =
            g.w / 2

        cy : Float
        cy =
            g.radius + 12

        px : Float -> String
        px v =
            String.fromFloat v ++ "px"
    in
    Control.cell (classList [ ( "slc", True ), ( "unlisted", Control.choice ch == Control.NotInList ), ( "missing", Control.choice ch == Control.NoValue ) ] :: Control.widthStyle (width config) :: Control.markAttrs (Control.chosenMark ch))
        c.label
        [ div [ class "sl", style "width" (px g.w), style "height" (px g.h), Control.ariaRole "radiogroup", Control.ariaLabel c.label ]
            (div
                ([ class "kn", style "width" "34px", style "height" "34px", style "left" (px (cx - 17)), style "top" (px (cy - 17)) ]
                    ++ Control.grab c.addr Knob
                )
                [ Knob.face Knob.NoFill (here |> Maybe.map (\k -> (angle k + 135) / 270)) ]
                :: List.indexedMap
                    (\k ( i, w ) ->
                        button
                            [ classList [ ( "on", Just i == Control.current ch ) ]
                            , Control.ariaRole "radio"
                            , Control.ariaChecked (Just i == Control.current ch)
                            , style "left" (px (cx + (g.radius + 4) * sin (degrees (angle k))))
                            , style "top" (px (cy - (g.radius - 4) * cos (degrees (angle k))))
                            , onClick (Pick c.addr i)
                            ]
                            [ text w ]
                    )
                    c.options
            )
        ]
