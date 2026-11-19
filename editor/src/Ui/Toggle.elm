module Ui.Toggle exposing (Config, fromOptions, label, three, two, view, width)

{-| A printed mini toggle for two or three choices: a lever beside the
words. A click on a word picks it; a click on the lever moves to the next.
-}

import Addr exposing (Addr)
import Html exposing (Html, button, div, text)
import Html.Attributes exposing (attribute, class, classList, style)
import Html.Events exposing (onClick)
import Ui.Control as Control exposing (Chosen, Msg(..), Option, Width)
import Ui.Tokens as Tokens exposing (Face(..))


type Positions
    = Two Option Option
    | Three Option Option Option


type Config
    = Config { addr : Addr, label : String, positions : Positions }


two : Addr -> Option -> Option -> Config
two a x y =
    Config { addr = a, label = "", positions = Two x y }


three : Addr -> Option -> Option -> Option -> Config
three a x y z =
    Config { addr = a, label = "", positions = Three x y z }


{-| A toggle from the unit's options: only two or three make one.
-}
fromOptions : Addr -> List Option -> Maybe Config
fromOptions a opts =
    case opts of
        [ x, y ] ->
            Just (two a x y)

        [ x, y, z ] ->
            Just (three a x y z)

        [] ->
            Nothing

        [ _ ] ->
            Nothing

        _ :: _ :: _ :: _ :: _ ->
            Nothing


label : String -> Config -> Config
label l (Config c) =
    Config { c | label = l }


options : Positions -> List Option
options p =
    case p of
        Two x y ->
            [ x, y ]

        Three x y z ->
            [ x, y, z ]


wordPx : String -> Int
wordPx w =
    Tokens.textPx ToggleFace w + 4


width : Config -> Width
width (Config c) =
    let
        widest : Int
        widest =
            options c.positions |> List.map (Tuple.second >> wordPx) |> List.maximum |> Maybe.withDefault 0
    in
    Control.pxWidth (Control.snap16 (max (Control.labelPx c.label) (14 + 6 + widest)))


view : Config -> Chosen -> Html Msg
view ((Config c) as config) ch =
    let
        opts : List Option
        opts =
            options c.positions

        n : Int
        n =
            List.length opts

        here : Maybe Int
        here =
            opts
                |> List.indexedMap Tuple.pair
                |> List.filter (\( _, ( i, _ ) ) -> Just i == Control.current ch)
                |> List.head
                |> Maybe.map Tuple.first

        -- Off every position (not reported, or not in the list), the lever
        -- moves to the first.
        next : Maybe Option
        next =
            case here of
                Just k ->
                    List.head (List.drop (modBy n (k + 1)) opts)

                Nothing ->
                    List.head opts

        ball : String
        ball =
            case ( c.positions, here ) of
                ( _, Nothing ) ->
                    "none"

                ( Three _ _ _, Just 1 ) ->
                    "m"

                ( _, Just 0 ) ->
                    "t"

                ( Two _ _, Just _ ) ->
                    "b"

                ( Three _ _ _, Just _ ) ->
                    "b"
    in
    Control.cell (class "tgc" :: Control.widthStyle (width config) :: Control.markAttrs (Control.chosenMark ch))
        c.label
        [ div [ classList [ ( "tg", True ), ( "n3", n == 3 ) ], Control.ariaRole "radiogroup", Control.ariaLabel c.label ]
            [ button
                ([ class "lv", attribute "data-p" ball, Control.ariaLabel (c.label ++ ": next") ]
                    ++ (case next of
                            Just ( i, _ ) ->
                                [ onClick (Pick c.addr i) ]

                            Nothing ->
                                []
                       )
                )
                []
            , div [ class "tw" ]
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
                    opts
                )
            ]
        ]
