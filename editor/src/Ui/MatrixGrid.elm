module Ui.MatrixGrid exposing (view)

{-| The routes as a signed amount grid: a row per source, a column per
destination. A patched cell drags its amount; an empty one patches.
-}

import Html exposing (Html, table, td, text, th, tr)
import Html.Attributes as Attr exposing (class, classList, style, title)
import Html.Events exposing (onClick)
import Ui.Control as Control exposing (Travel(..))
import Ui.Routes as Routes exposing (Cable, Dest(..), Routes, Sign(..), Source(..))


view : { connect : Source -> Dest -> msg, hand : Control.Msg -> msg } -> Routes -> Html msg
view { connect, hand } routes =
    table [ class "mx" ]
        (tr [] (th [ class "rh" ] [] :: List.map (\(Dest d) -> th [ title d ] [ text d ]) (Routes.dests routes))
            :: List.map
                (\((Source s) as src) ->
                    tr []
                        (th [ class "rh" ] [ text s ]
                            :: List.map (cell connect hand routes src) (Routes.dests routes)
                        )
                )
                (Routes.sources routes)
        )


cell : (Source -> Dest -> msg) -> (Control.Msg -> msg) -> Routes -> Source -> Dest -> Html msg
cell connect hand routes ((Source s) as src) ((Dest d) as dst) =
    case List.filter (\c -> c.from == src && c.to == dst) (Routes.cables routes) of
        c :: _ ->
            patched hand c

        [] ->
            td [ title ("patch " ++ s ++ " → " ++ d), onClick (connect src dst) ] [ text "·" ]


patched : (Control.Msg -> msg) -> Cable -> Html msg
patched hand c =
    let
        a : Int
        a =
            abs (Routes.amountValue c.amount)

        ( cls, token ) =
            case Routes.sign c.amount of
                Adds ->
                    ( "adds", "var(--mod)" )

                Subtracts ->
                    ( "subtracts", "var(--modneg)" )
    in
    td
        ([ classList [ ( cls, True ), ( "strong", a > 60 ) ]
         , style "background" ("color-mix(in srgb, " ++ token ++ " " ++ String.fromFloat (toFloat a * 0.6) ++ "%, var(--pn2))")
         ]
            ++ List.map (Attr.map hand) (Control.drag c.addr Attenuverter)
        )
        [ text (Routes.amountText c.amount) ]
