port module Transport exposing
    ( Link(..)
    , connectSerial
    , connectSocket
    , lineIn
    , linkState
    , linkText
    , parseLink
    , send
    )

{-| The one link to the unit, over Web Serial or a WebSocket. The JavaScript
side is `ports.js`; Elm only sees lines and a link state.
-}


type Link
    = Open
    | Busy
    | Lost
    | Unsupported


port connectSerial : () -> Cmd msg


port connectSocket : String -> Cmd msg


port send : String -> Cmd msg


port lineIn : (String -> msg) -> Sub msg


port linkState : (String -> msg) -> Sub msg


{-| An unknown state is a lost link: never leave the editor thinking it is open.
-}
parseLink : String -> Link
parseLink s =
    case s of
        "open" ->
            Open

        "busy" ->
            Busy

        "unsupported" ->
            Unsupported

        _ ->
            Lost


linkText : String -> Maybe String
linkText s =
    case parseLink s of
        Open ->
            Nothing

        Busy ->
            Just "THE PORT IS IN USE (CLOSE chimera-usb.py)"

        Lost ->
            Just "THE UNIT WAS DISCONNECTED"

        Unsupported ->
            Just "USE CHROME OR EDGE"
