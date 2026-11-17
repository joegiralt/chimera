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


{-| The editor edits ALGO; any other engine is named, so it can be said.
-}
type Engine
    = Algo
    | Other String


engineFromName : String -> Engine
engineFromName s =
    if s == "ALGO" then
        Algo

    else
        Other s


engineName : Engine -> String
engineName e =
    case e of
        Algo ->
            "ALGO"

        Other s ->
            s
