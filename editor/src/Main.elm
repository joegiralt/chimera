port module Main exposing (main)

import Browser
import Html exposing (Html)
import Json.Decode as Decode
import Model
import Ui.Tokens as Tokens exposing (Os(..))
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
    let
        preference : Tokens.Preference
        preference =
            Decode.decodeValue
                (Decode.map2 (\s d -> { stored = s, os = os d })
                    (Decode.field "stored" (Decode.nullable Decode.string))
                    (Decode.field "dark" Decode.bool)
                )
                flags
                |> Result.withDefault { stored = Nothing, os = OsLight }
    in
    ( { unit = Model.init, page = EnginePage.init (Tokens.fromPreference preference) }, Cmd.none )


os : Bool -> Os
os dark =
    if dark then
        OsDark

    else
        OsLight


update : Msg -> App -> ( App, Cmd Msg )
update msg app =
    case msg of
        Unit m ->
            let
                ( unit, cmd ) =
                    Model.update m app.unit
            in
            ( { app | unit = unit }, Cmd.map Unit cmd )

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
            ( { app | unit = unit, page = page }
            , Cmd.batch (Maybe.withDefault Cmd.none (Maybe.map (Tokens.name >> storeTheme) store) :: cmds)
            )


view : App -> Html Msg
view app =
    Html.map Page (EnginePage.view app.unit app.page)
