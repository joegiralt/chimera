module Wire exposing (addr, page)

{-| Addresses and page numbers as a test writes them: the unit's text, read
the way the protocol reads it.
-}

import Addr exposing (Addr)
import Protocol exposing (PageNo)


{-| The address the unit writes `s` for. Text that is no address is one no
fixture holds, so a test using it fails on it.
-}
addr : String -> Addr
addr s =
    Addr.fromWire s |> Maybe.withDefault (Addr.at Addr.Kit (Addr.field "NOT_AN_ADDRESS"))


page : Int -> PageNo
page n =
    Protocol.pageNo n |> Maybe.withDefault Protocol.firstPage
