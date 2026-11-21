port module Cache exposing (got, keep, recall)

{-| The editor's schema cache: IndexedDB through `ports.js`, keyed by the
protocol, the schema and its hash (ADR 0289). A miss, or any failure of
the browser's store, answers `Nothing`: the editor describes the schema
again.
-}


port recall : String -> Cmd msg


port keep : { key : String, lines : List String } -> Cmd msg


port got : ({ key : String, lines : Maybe (List String) } -> msg) -> Sub msg
