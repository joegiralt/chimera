module Unit exposing
    ( Engine(..)
    , Gen
    , Part
    , allParts
    , engineFromName
    , engineName
    , genFromInt
    , genToInt
    , partCount
    , partFromInt
    , partNumber
    )

{-| Small types for what the unit says, so a bare Int is never a Part, a
generation or anything else.
-}


{-| The unit's number for its last `since` answer. Never 0: the firmware
answers 1 and up, and 0 is the editor's "I hold nothing", which is `Nothing`.
-}
type Gen
    = Gen Int


genFromInt : Int -> Maybe Gen
genFromInt n =
    if n >= 1 then
        Just (Gen n)

    else
        Nothing


genToInt : Gen -> Int
genToInt (Gen n) =
    n


{-| One of the box's Parts, B1 to B6: no other value exists.
-}
type Part
    = P1
    | P2
    | P3
    | P4
    | P5
    | P6


{-| Every Part, in the box's order. The unit's own count is pinned against
it (`UnitTest`, from `console_ed_test`'s fixture).
-}
allParts : List Part
allParts =
    [ P1, P2, P3, P4, P5, P6 ]


partCount : Int
partCount =
    List.length allParts


{-| As the box numbers it, from 1.
-}
partNumber : Part -> Int
partNumber p =
    case p of
        P1 ->
            1

        P2 ->
            2

        P3 ->
            3

        P4 ->
            4

        P5 ->
            5

        P6 ->
            6


partFromInt : Int -> Maybe Part
partFromInt n =
    allParts |> List.filter (\p -> partNumber p == n) |> List.head


{-| The unit's engines, by the names `ed` gives them. A name the editor
doesn't know is a newer unit's: `Nothing`, and the editor says so.
-}
type Engine
    = Algo
    | Modal
    | Va
    | Tape
    | Drum


engineFromName : String -> Maybe Engine
engineFromName s =
    case s of
        "ALGO" ->
            Just Algo

        "MODAL" ->
            Just Modal

        "VA" ->
            Just Va

        "TAPE" ->
            Just Tape

        "DRUM" ->
            Just Drum

        _ ->
            Nothing


engineName : Engine -> String
engineName e =
    case e of
        Algo ->
            "ALGO"

        Modal ->
            "MODAL"

        Va ->
            "VA"

        Tape ->
            "TAPE"

        Drum ->
            "DRUM"
