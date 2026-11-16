module Main exposing (main)

import Browser
import Dict
import Html exposing (Html, button, div, text)
import Html.Events exposing (onClick)
import Model exposing (Model, Msg(..))
import Transport


{-| Throwaway view: connect buttons, the notice, the unit's values.
-}
main : Program () Model Msg
main =
    Browser.element
        { init = \_ -> ( Model.init, Cmd.none )
        , update = Model.update
        , view = view
        , subscriptions = Model.subscriptions
        }


view : Model -> Html Msg
view model =
    div []
        (button [ onClick ConnectSerial ] [ text "CONNECT UNIT" ]
            :: button [ onClick ConnectSocket ] [ text "CONNECT SIMULATOR" ]
            :: div [] [ text (Model.notice model |> Maybe.withDefault "") ]
            :: List.map (\( a, r ) -> div [] [ text (a ++ " " ++ r.text) ]) (Dict.toList model.values)
        )
