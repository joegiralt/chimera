module Main exposing (main)

import Browser
import Html exposing (Html, button, div, text)
import Html.Attributes exposing (class)
import Html.Events exposing (onClick)
import Model
import Unit exposing (Engine(..))
import View.Algo as Algo


{-| The unit's state, and the hands on the face.
-}
type alias App =
    { unit : Model.Model
    , panel : Algo.State
    }


type Msg
    = Unit Model.Msg
    | Panel Algo.Msg


main : Program () App Msg
main =
    Browser.element
        { init = \_ -> ( { unit = Model.init, panel = Algo.init }, Cmd.none )
        , update = update
        , view = view
        , subscriptions = \app -> Sub.batch [ Sub.map Unit (Model.subscriptions app.unit), Sub.map Panel (Algo.subscriptions app.panel) ]
        }


update : Msg -> App -> ( App, Cmd Msg )
update msg app =
    case msg of
        Unit m ->
            let
                ( unit, cmd ) =
                    Model.update m app.unit
            in
            ( { app | unit = unit }, Cmd.map Unit cmd )

        Panel m ->
            let
                ( panel, edits ) =
                    Algo.update m app.unit app.panel

                ( unit, cmds ) =
                    List.foldl
                        (\e ( u, cs ) -> Model.update e u |> Tuple.mapSecond (\c -> Cmd.map Unit c :: cs))
                        ( app.unit, [] )
                        edits
            in
            ( { app | unit = unit, panel = panel }, Cmd.batch cmds )


view : App -> Html Msg
view app =
    let
        notice =
            Model.notice app.unit
    in
    div [ class "editor" ]
        [ div [ class "top" ]
            [ button [ class "part", onClick (Unit Model.ConnectSerial) ] [ text "CONNECT UNIT" ]
            , button [ class "part", onClick (Unit Model.ConnectSocket) ] [ text "CONNECT SIMULATOR" ]
            , div [ class "notice" ] [ text (Maybe.withDefault "" notice) ]
            , div [ class "status" ] [ text (Maybe.withDefault "" (Model.status app.unit)) ]
            ]
        , if notice == Nothing && app.unit.engine == Just Algo then
            Html.map Panel (Algo.view app.unit app.panel)

          else
            text ""
        ]
