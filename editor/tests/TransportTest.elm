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
                    (List.map (Transport.parseLink >> Transport.linkText) [ "busy", "lost", "unsupported", "absent" ])
                    [ Just "THE PORT IS IN USE (CLOSE chimera-usb.py)"
                    , Just "THE UNIT WAS DISCONNECTED"
                    , Just "USE CHROME OR EDGE"
                    , Just "THE SIMULATOR IS NOT RUNNING (just editor-sim)"
                    ]
        , test "link states parse" <|
            \_ ->
                Expect.equal
                    (List.map Transport.parseLink [ "open", "busy", "lost", "unsupported", "absent", "huh" ])
                    [ Open, Busy, Lost, Unsupported, Absent, Lost ]
        , test "open has no text" <|
            \_ -> Expect.equal (Transport.linkText Open) Nothing
        ]
