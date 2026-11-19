module Contract exposing (opt, options, readings, widthHolds)

{-| The fixed-width contract, as a test: a component rendered at many
readings keeps the one width its `Config` gives it.
-}

import Expect exposing (Expectation)
import Html exposing (Html)
import Protocol exposing (OptIndex)
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Option, Reading, Width)


{-| Five readings: the shortest text, the longest (longer than any box), a
negative, a pending one, and the ends of the travel.
-}
readings : List Reading
readings =
    [ Control.reading 0 ""
    , Control.reading 1 "20000.00 Hz and then some"
    , Control.reading 0.5 "-100"
    , Control.pending (Control.reading 0.25 "1.20 kHz")
    , Control.reading 0.75 "C#-1"
    ]


widthHolds : Width -> List (Html msg) -> Expectation
widthHolds w renders =
    Expect.all
        (List.map (\h _ -> Query.fromHtml h |> Query.has [ Sel.style "width" (Control.widthText w) ]) renders)
        ()


opt : Int -> OptIndex
opt i =
    Protocol.optIndex i |> Maybe.withDefault Protocol.firstOption


options : List String -> List Option
options =
    List.indexedMap (\i n -> ( opt i, n ))
