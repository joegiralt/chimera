module Unit exposing
    ( Engine(..)
    , Gen
    , Part
    , allParts
    , engineFromName
    , engineName
    , genFromInt
    , genToInt
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


{-| A Part as the box numbers it, 1 to 6.
-}
type Part
    = Part Int


{-| The box's Parts (`MAX_PARTS`), one key each.
-}
partCount : Int
partCount =
    6


partFromInt : Int -> Maybe Part
partFromInt n =
    if n >= 1 && n <= partCount then
        Just (Part n)

    else
        Nothing


allParts : List Part
allParts =
    List.range 1 partCount |> List.map Part


partNumber : Part -> Int
partNumber (Part n) =
    n


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
