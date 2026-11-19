module Ui.Page exposing
    ( Config
    , Current(..)
    , Effect(..)
    , Link(..)
    , Msg(..)
    , State
    , Tab
    , init
    , link
    , meta
    , modules
    , new
    , parts
    , strip
    , theme
    , units
    , update
    , view
    )

{-| A page: the top bar (Parts, the unit's pages, the theme switch, the
link), the chip bar (show or hide a group, fold all), and the modules in
rows of two below.
-}

import Html exposing (Html, button, div, h1, main_, span, text)
import Html.Attributes exposing (attribute, class, classList)
import Html.Events exposing (onClick)
import Ui.Chip as Chip exposing (Chip, Chips)
import Ui.Module as Module exposing (Fold(..), Folds)
import Ui.Row as Row
import Ui.Tokens as Tokens exposing (Theme)


type Current
    = Here
    | Elsewhere


{-| A tab in the top bar: what it is (`P2 · ALGO`), its name, and where it goes.
-}
type alias Tab msg =
    { kicker : String, name : String, current : Current, go : msg }


type Link
    = Offline
    | Linked String


type Config msg
    = Config
        { title : String
        , meta : String
        , parts : List (Tab msg)
        , units : List (Tab msg)
        , link : Link
        , strip : List (Html msg)
        , modules : List (Module.Config msg)
        }


new : String -> Config msg
new title =
    Config { title = title, meta = "", parts = [], units = [], link = Offline, strip = [], modules = [] }


meta : String -> Config msg -> Config msg
meta s (Config c) =
    Config { c | meta = s }


parts : List (Tab msg) -> Config msg -> Config msg
parts ts (Config c) =
    Config { c | parts = ts }


{-| The unit's own pages: MIXER, ORBIT, SETTINGS, MIDI MAP.
-}
units : List (Tab msg) -> Config msg -> Config msg
units ts (Config c) =
    Config { c | units = ts }


link : Link -> Config msg -> Config msg
link l (Config c) =
    Config { c | link = l }


{-| A strip under the chips (DRUM's pads).
-}
strip : Html msg -> Config msg -> Config msg
strip s (Config c) =
    Config { c | strip = [ s ] }


modules : List (Module.Config msg) -> Config msg -> Config msg
modules ms (Config c) =
    Config { c | modules = ms }


type State
    = State { theme : Theme, hidden : Chips, folds : Folds }


init : Theme -> State
init t =
    State { theme = t, hidden = Chip.none, folds = Module.noFolds }


theme : State -> Theme
theme (State s) =
    s.theme


type Msg
    = ChoseTheme Theme
    | ToggleChip Chip
    | Folding Module.Msg


{-| What the page asks of the world: the theme is kept by the shim.
-}
type Effect
    = NoEffect
    | StoreTheme Theme


update : Msg -> State -> ( State, Effect )
update msg (State s) =
    case msg of
        ChoseTheme t ->
            ( State { s | theme = t }, StoreTheme t )

        ToggleChip chip ->
            ( State { s | hidden = Chip.toggle chip s.hidden }, NoEffect )

        Folding m ->
            ( State { s | folds = Module.update m s.folds }, NoEffect )


view : (Msg -> msg) -> State -> Config msg -> Html msg
view toMsg (State s) (Config c) =
    let
        shown : List (Module.Config msg)
        shown =
            List.filter (\m -> not (Chip.member (Module.chipOf m) s.hidden)) c.modules

        chips : List Chip
        chips =
            List.foldl
                (\m acc ->
                    if List.member (Module.chipOf m) acc then
                        acc

                    else
                        acc ++ [ Module.chipOf m ]
                )
                []
                c.modules

        foldAll : Fold -> msg
        foldAll f =
            toMsg (Folding (Module.SetFold (List.map Module.idOf shown) f))
    in
    div [ class "ui page", Tokens.themed s.theme ]
        [ div [ class "ptop" ]
            ([ div [ class "row" ]
                (span [ class "brand" ] [ text "CHIMERA" ]
                    :: List.map tab c.parts
                    ++ (if List.isEmpty c.units then
                            []

                        else
                            span [ class "sep" ] [] :: List.map tab c.units
                       )
                    ++ [ div [ class "themesw" ]
                            (List.map
                                (\t ->
                                    button
                                        [ classList [ ( "on", t == s.theme ) ]
                                        , attribute "aria-pressed" (ariaBool (t == s.theme))
                                        , onClick (toMsg (ChoseTheme t))
                                        ]
                                        [ text (Tokens.label t) ]
                                )
                                Tokens.themes
                            )
                       , span [ class "unit" ]
                            (case c.link of
                                Linked l ->
                                    [ Html.i [ class "live" ] [], text l ]

                                Offline ->
                                    [ Html.i [] [], text "NO UNIT" ]
                            )
                       ]
                )
             , div [ class "row chips" ]
                (span [ class "k" ] [ text "SHOW" ]
                    :: List.map
                        (\chip ->
                            button
                                [ classList [ ( "chip", True ), ( "off", Chip.member chip s.hidden ) ]
                                , attribute "aria-pressed" (ariaBool (not (Chip.member chip s.hidden)))
                                , onClick (toMsg (ToggleChip chip))
                                ]
                                [ text (Chip.label chip) ]
                        )
                        chips
                    ++ [ button [ class "chip push", onClick (foldAll Folded) ] [ text "FOLD ALL" ]
                       , button [ class "chip", onClick (foldAll Open) ] [ text "OPEN ALL" ]
                       ]
                )
             ]
                ++ List.map (\x -> div [ class "row" ] [ x ]) c.strip
            )
        , main_ []
            [ div [ class "pagehead" ] [ h1 [] [ text c.title ], span [ class "meta" ] [ text c.meta ] ]
            , div [ class "prows" ] (List.map (Row.view (Folding >> toMsg) s.folds) (Row.pairUp shown))
            ]
        ]


tab : Tab msg -> Html msg
tab t =
    button [ classList [ ( "tab", True ), ( "on", t.current == Here ) ], onClick t.go ]
        [ Html.b [] [ text t.kicker ], span [] [ text t.name ] ]


ariaBool : Bool -> String
ariaBool on =
    if on then
        "true"

    else
        "false"
