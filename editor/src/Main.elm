module Main exposing (main)

import Browser
import Html exposing (Html, button, div, text)
import Html.Events exposing (onClick)
import Protocol exposing (Command(..), Line(..), encode, parse)
import Transport


type alias Model =
    { link : Maybe Transport.Link
    , lines : List String
    }


type Msg
    = Serial
    | Socket
    | Line String
    | Link Transport.Link


main : Program () Model Msg
main =
    Browser.element
        { init = \_ -> ( { link = Nothing, lines = [] }, Cmd.none )
        , update = update
        , view = view
        , subscriptions = \_ -> Sub.batch [ Transport.lineIn Line, Transport.linkState (Transport.parseLink >> Link) ]
        }


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        Serial ->
            ( model, Transport.connectSerial () )

        Socket ->
            ( model, Transport.connectSocket "ws://127.0.0.1:7342" )

        Link l ->
            ( { model | link = Just l }
            , if l == Transport.Open then
                sendCmd Hello

              else
                Cmd.none
            )

        Line raw ->
            case parse raw of
                Unknown _ ->
                    ( model, Cmd.none )

                _ ->
                    ( { model | lines = raw :: List.take 20 model.lines }, Cmd.none )


view : Model -> Html Msg
view model =
    div []
        (button [ onClick Serial ] [ text "CONNECT UNIT" ]
            :: button [ onClick Socket ] [ text "CONNECT SIMULATOR" ]
            :: div [] [ text (model.link |> Maybe.andThen Transport.linkText |> Maybe.withDefault "") ]
            :: List.map (\l -> div [] [ text l ]) (List.reverse model.lines)
        )


{-| A command that could not be a single line is never sent.
-}
sendCmd : Command -> Cmd msg
sendCmd c =
    encode c |> Maybe.map Transport.send |> Maybe.withDefault Cmd.none
