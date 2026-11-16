module TransportTest exposing (suite)

import Expect
import Test exposing (Test, describe, test)
import Transport exposing (Link(..))


suite : Test
suite =
    describe "Transport"
        [ test "port_busy_and_lost_are_reported" <|
            \_ ->
                Expect.equal
                    [ Transport.linkText "busy"
                    , Transport.linkText "lost"
                    , Transport.linkText "unsupported"
                    ]
                    [ Just "THE PORT IS IN USE (CLOSE chimera-usb.py)"
                    , Just "THE UNIT WAS DISCONNECTED"
                    , Just "USE CHROME OR EDGE"
                    ]
        , test "link states parse" <|
            \_ ->
                Expect.equal
                    (List.map Transport.parseLink [ "open", "busy", "lost", "unsupported", "huh" ])
                    [ Open, Busy, Lost, Unsupported, Lost ]
        , test "open has no text" <|
            \_ -> Expect.equal (Transport.linkText "open") Nothing
        ]
