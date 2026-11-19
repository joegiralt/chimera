module Ui.Module exposing
    ( Config
    , Fold(..)
    , Folds
    , Id
    , Jack(..)
    , Msg(..)
    , body
    , chip
    , chipOf
    , foldOf
    , full
    , group
    , idOf
    , isFull
    , jacks
    , menus
    , new
    , noFolds
    , note
    , panel
    , summary
    , titleOf
    , update
    , view
    )

{-| A module of the panel: a black title bar (title, a small note, header
menus, and a summary while folded) that folds the body, and jacks along the
bottom edge.
-}

import Html exposing (Html, div, h2, header, small, span, text)
import Html.Attributes exposing (attribute, class, classList)
import Html.Events as Events
import Set exposing (Set)


{-| A module on a page, by its title unless told otherwise.
-}
type Id
    = Id String


type Fold
    = Open
    | Folded


{-| Which modules are folded.
-}
type Folds
    = Folds (Set String)


noFolds : Folds
noFolds =
    Folds Set.empty


foldOf : Id -> Folds -> Fold
foldOf (Id i) (Folds s) =
    if Set.member i s then
        Folded

    else
        Open


type Msg
    = SetFold (List Id) Fold


update : Msg -> Folds -> Folds
update (SetFold ids fold) (Folds s) =
    let
        keys : Set String
        keys =
            Set.fromList (List.map (\(Id i) -> i) ids)
    in
    Folds
        (case fold of
            Folded ->
                Set.union keys s

            Open ->
                Set.diff s keys
        )


type Span
    = Half
    | Whole


{-| A jack: inputs are destinations, outputs are sources.
-}
type Jack
    = In String
    | Out String


type Config msg
    = Config
        { id : Id
        , title : String
        , note : String
        , chip : String
        , menus : List (Html msg)
        , summary : List ( String, String )
        , span : Span
        , jacks : List Jack
        , body : List (Html msg)
        }


new : String -> Config msg
new title =
    Config { id = Id title, title = title, note = "", chip = title, menus = [], summary = [], span = Half, jacks = [], body = [] }


note : String -> Config msg -> Config msg
note s (Config c) =
    Config { c | note = s }


{-| The show/hide chip the module answers to (several modules can share one).
-}
chip : String -> Config msg -> Config msg
chip s (Config c) =
    Config { c | chip = s }


{-| The title bar's type menus (`Ui.Menu.inHeader`).
-}
menus : List (Html msg) -> Config msg -> Config msg
menus ms (Config c) =
    Config { c | menus = ms }


{-| What the title bar says while the module is folded.
-}
summary : List ( String, String ) -> Config msg -> Config msg
summary s (Config c) =
    Config { c | summary = s }


{-| A module that takes a whole row (MOD).
-}
full : Config msg -> Config msg
full (Config c) =
    Config { c | span = Whole }


jacks : List Jack -> Config msg -> Config msg
jacks js (Config c) =
    Config { c | jacks = js }


body : List (Html msg) -> Config msg -> Config msg
body b (Config c) =
    Config { c | body = b }


idOf : Config msg -> Id
idOf (Config c) =
    c.id


titleOf : Config msg -> String
titleOf (Config c) =
    c.title


chipOf : Config msg -> String
chipOf (Config c) =
    c.chip


isFull : Config msg -> Bool
isFull (Config c) =
    c.span == Whole


{-| The body's layout: groups side by side, wrapping as the module narrows.
-}
panel : List (Html msg) -> Html msg
panel =
    div [ class "pnl" ]


{-| A titled group of controls, ruled off from its neighbour.
-}
group : String -> List (Html msg) -> Html msg
group title items =
    div [ class "grp" ]
        ((if title == "" then
            text ""

          else
            div [ class "gt" ] [ text title ]
         )
            :: items
        )


view : { fold : Fold, toggle : msg } -> Config msg -> Html msg
view { fold, toggle } (Config c) =
    let
        folded : Bool
        folded =
            fold == Folded
    in
    Html.section [ classList [ ( "sec", True ), ( "folded", folded ) ] ]
        (header []
            [ Html.button
                [ class "handle"
                , Events.onClick toggle
                , attribute "aria-expanded"
                    (if folded then
                        "false"

                     else
                        "true"
                    )
                ]
                [ span [ class "fold" ] [ text "▾" ]
                , h2 [] [ text c.title, small [] [ text c.note ] ]
                , div [ class "sum" ] (List.map (\( k, v ) -> span [] [ text (k ++ " "), Html.b [] [ text v ] ]) c.summary)
                ]
            , div [ class "hmenu" ] c.menus
            ]
            :: (if folded then
                    []

                else
                    [ div [ class "body" ] c.body, jackRow c.jacks ]
               )
        )


jackRow : List Jack -> Html msg
jackRow js =
    if List.isEmpty js then
        text ""

    else
        jacksOf js


jacksOf : List Jack -> Html msg
jacksOf js =
    let
        ins : List String
        ins =
            List.filterMap
                (\j ->
                    case j of
                        In l ->
                            Just l

                        Out _ ->
                            Nothing
                )
                js

        outs : List String
        outs =
            List.filterMap
                (\j ->
                    case j of
                        Out l ->
                            Just l

                        In _ ->
                            Nothing
                )
                js

        jack : String -> String -> Html msg
        jack dir l =
            div [ class ("jk " ++ dir) ] [ span [] [ text l ], span [ class "hole" ] [] ]
    in
    div [ class "jacks" ]
        ((if List.isEmpty ins then
            []

          else
            span [ class "jt" ] [ text "IN" ] :: List.map (jack "in") ins
         )
            ++ span [ class "sp" ] []
            :: List.map (jack "out") outs
        )
