port module Main exposing (main)

import Browser
import Html exposing (Html)
import Json.Decode as Decode
import Model
import Ui.Tokens as Tokens
import View.EnginePage as EnginePage


{-| The theme a choice in the top bar keeps (theme.js, behind try/catch).
-}
port storeTheme : String -> Cmd msg


{-| The unit's state, and the hands on the page.
-}
type alias App =
    { unit : Model.Model
    , page : EnginePage.State
    }


type Msg
    = Unit Model.Msg
    | Page EnginePage.Msg


main : Program Decode.Value App Msg
main =
    Browser.element
        { init = init
        , update = update
        , view = view
        , subscriptions = \app -> Sub.batch [ Sub.map Unit Model.subscriptions, Sub.map Page (EnginePage.subscriptions app.page) ]
        }


{-| The stored theme, else the OS's: what theme.js read at start.
-}
init : Decode.Value -> ( App, Cmd Msg )
init flags =
    ( { unit = Model.init, page = EnginePage.init (Tokens.fromPreference (Tokens.preference flags)) }, Cmd.none )


update : Msg -> App -> ( App, Cmd Msg )
update msg app =
    case msg of
        Unit m ->
            let
                ( unit, cmd ) =
                    Model.update m app.unit
            in
            ( { app | unit = unit, page = EnginePage.follow unit app.page }, Cmd.map Unit cmd )

        Page m ->
            let
                ( page, asks, store ) =
                    EnginePage.update m app.unit app.page

                ( unit, cmds ) =
                    List.foldl
                        (\e ( u, cs ) -> Model.update e u |> Tuple.mapSecond (\c -> Cmd.map Unit c :: cs))
                        ( app.unit, [] )
                        asks
            in
            ( { app | unit = unit, page = EnginePage.follow unit page }
            , Cmd.batch (Maybe.withDefault Cmd.none (Maybe.map (Tokens.name >> storeTheme) store) :: cmds)
            )


view : App -> Html Msg
view app =
    Html.map Page (EnginePage.view app.unit app.page)
