module View.Parts exposing (view)

{-| The Part keys: one a Part, the one the box follows marked.
-}

import Html exposing (Html, button, div, text)
import Html.Attributes exposing (class, classList)
import Html.Events exposing (onClick)
import Unit exposing (Part)


view : Maybe Part -> (Part -> msg) -> Html msg
view followed ask =
    div [ class "parts" ]
        (Unit.allParts
            |> List.map
                (\p ->
                    button
                        [ classList [ ( "part", True ), ( "on", followed == Just p ) ]
                        , onClick (ask p)
                        ]
                        [ text (String.fromInt (Unit.partNumber p)) ]
                )
        )
