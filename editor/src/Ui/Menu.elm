module Ui.Menu exposing (Config, Msg(..), State, fromOptions, glyphs, inHeader, label, shut, update, view, width)

{-| More than eight choices: a button with the value that opens the list,
optionally with the unit's glyph on each option. One menu is open at a time.
-}

import Html exposing (Html, b, button, div, span, text)
import Html.Attributes exposing (attribute, class, classList, style)
import Html.Events as Events exposing (onClick)
import Json.Decode as Decode
import Protocol exposing (Addr, OptIndex)
import Ui.Control as Control exposing (Chosen, Option, Width)
import Ui.Screen as Screen


type Place
    = InPanel
    | InHeader


type Config
    = Config
        { addr : Addr
        , label : String
        , options : List Option
        , glyph : OptIndex -> Maybe Screen.Config
        , place : Place
        }


{-| A menu from the unit's options; there must be some.
-}
fromOptions : Addr -> List Option -> Maybe Config
fromOptions a opts =
    if List.isEmpty opts then
        Nothing

    else
        Just (Config { addr = a, label = "", options = opts, glyph = \_ -> Nothing, place = InPanel })


label : String -> Config -> Config
label l (Config c) =
    Config { c | label = l }


glyphs : (OptIndex -> Maybe Screen.Config) -> Config -> Config
glyphs g (Config c) =
    Config { c | glyph = g }


{-| The title bar's type menu (TYPE ▾, WAVE ▾): it swaps a module's body.
-}
inHeader : Config -> Config
inHeader (Config c) =
    Config { c | place = InHeader }


type State
    = Shut
    | Showing Addr


shut : State
shut =
    Shut


type Msg
    = Open Addr
    | Close
    | Choose Addr OptIndex


{-| A choice shuts the menu and says the pick, for the page's one update.
-}
update : Msg -> ( State, Maybe Control.Msg )
update msg =
    case msg of
        Open a ->
            ( Showing a, Nothing )

        Close ->
            ( Shut, Nothing )

        Choose a i ->
            ( Shut, Just (Control.Pick a i) )


widest : List Option -> Int
widest opts =
    opts |> List.map (Tuple.second >> String.length) |> List.maximum |> Maybe.withDefault 1


{-| The value's box holds the widest option name, in its own face.
-}
width : Config -> Width
width (Config c) =
    case c.place of
        InPanel ->
            Control.width { px = Control.snap16 (max 56 (Control.labelPx c.label)), ch = widest c.options + 2 }

        InHeader ->
            Control.width { px = Control.labelPx c.label + 34 + 9 * widest c.options, ch = 0 }


view : Config -> State -> Chosen -> Html Msg
view ((Config c) as config) state ch =
    let
        open : Bool
        open =
            state == Showing c.addr

        name : String
        name =
            c.options
                |> List.filter (\( i, _ ) -> i == Control.current ch)
                |> List.head
                |> Maybe.map Tuple.second
                |> Maybe.withDefault "--"

        toggle : Html.Attribute Msg
        toggle =
            onClick
                (if open then
                    Close

                 else
                    Open c.addr
                )

        popup : Html Msg
        popup =
            if open then
                div
                    [ class "pop"
                    , Control.ariaRole "listbox"
                    , Control.ariaLabel c.label
                    , Events.on "keydown" (Decode.field "key" Decode.string |> Decode.andThen escape)
                    ]
                    (List.map
                        (\( i, w ) ->
                            button
                                [ classList [ ( "on", i == Control.current ch ) ]
                                , Control.ariaRole "option"
                                , attribute "aria-selected" (ariaBool (i == Control.current ch))
                                , onClick (Choose c.addr i)
                                ]
                                (case c.glyph i of
                                    Just g ->
                                        [ Screen.view g, text w ]

                                    Nothing ->
                                        [ text w ]
                                )
                        )
                        c.options
                    )

            else
                text ""

        popupAttrs : List (Html.Attribute Msg)
        popupAttrs =
            [ attribute "aria-haspopup" "listbox", attribute "aria-expanded" (ariaBool open), toggle ]
    in
    case c.place of
        InPanel ->
            Control.cell [ class "mnc", Control.syncClass (Control.chosenSync ch), Control.widthStyle (width config) ]
                c.label
                [ button (class "mn" :: popupAttrs) [ text name ], popup ]

        InHeader ->
            div [ class "hmn", Control.syncClass (Control.chosenSync ch), Control.widthStyle (width config) ]
                [ button (class "hbtn" :: popupAttrs)
                    [ b [] [ text c.label ], span [ style "width" (String.fromInt (widest c.options) ++ "ch") ] [ text name ] ]
                , popup
                ]


escape : String -> Decode.Decoder Msg
escape key =
    if key == "Escape" then
        Decode.succeed Close

    else
        Decode.fail "not escape"


ariaBool : Bool -> String
ariaBool on =
    if on then
        "true"

    else
        "false"
