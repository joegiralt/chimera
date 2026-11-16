module Protocol exposing
    ( Addr(..)
    , AnswerLine(..)
    , Command(..)
    , Kind(..)
    , Line(..)
    , Value(..)
    , encode
    , parse
    )

{-| The unit's text console, `ed` namespace: one command out, answer lines in.
Parsing never fails: a line that isn't understood is `Unknown` and is ignored.
-}


{-| A parameter address, `OP3.CRSE`. The unit's own text, opaque here.
-}
type Addr
    = Addr String


type Value
    = Num Float
    | Name String


type Kind
    = Cont
    | Choice
    | Toggle


type Command
    = Hello
    | Describe String Int
    | Get Addr
    | Set Addr Value
    | Since Int


type AnswerLine
    = HelloIs { protocol : Int, build : String, part : Int, engine : String }
    | Page { n : Int, label : String, count : Int }
    | Panel { name : String, variant : String }
    | Param { addr : Addr, label : String, kind : Kind, min : Float, max : Float, step : Float, default : Float }
    | Opt { addr : Addr, index : Int, name : String }
    | Route String
    | Val { addr : Addr, norm : Float, stored : Float, text : String }
    | PartIs Int String
    | Queued


type Line
    = Answer AnswerLine
    | Ok_ (Maybe Int)
    | Err_ String
    | Unknown String


encode : Command -> String
encode c =
    case c of
        Hello ->
            "ed hello"

        Describe engine page ->
            "ed describe " ++ engine ++ " " ++ String.fromInt page

        Get (Addr a) ->
            "ed get " ++ a

        Set (Addr a) (Num n) ->
            "ed set " ++ a ++ " " ++ String.fromFloat n

        Set (Addr a) (Name n) ->
            "ed set " ++ a ++ " " ++ n

        Since n ->
            "ed since " ++ String.fromInt n


parse : String -> Line
parse raw =
    let
        line =
            stripCr raw
    in
    case tokens line of
        Just ("OK" :: []) ->
            Ok_ Nothing

        Just [ "OK", "queued" ] ->
            Answer Queued

        Just [ "OK", m ] ->
            nat m |> Maybe.map (Just >> Ok_) |> Maybe.withDefault (Unknown line)

        Just ("ERR" :: _) ->
            Err_ (String.dropLeft 4 line)

        Just ts ->
            answer ts |> Maybe.map Answer |> Maybe.withDefault (Unknown line)

        Nothing ->
            Unknown line


stripCr : String -> String
stripCr s =
    if String.endsWith "\u{000D}" s then
        String.dropRight 1 s

    else
        s


nat : String -> Maybe Int
nat s =
    if s /= "" && String.all Char.isDigit s then
        String.toInt s

    else
        Nothing


num : String -> Maybe Float
num s =
    let
        body =
            if String.startsWith "-" s || String.startsWith "+" s then
                String.dropLeft 1 s

            else
                s
    in
    -- String.toFloat takes "NaN", "Infinity" and "": the unit's numbers are plain decimals.
    if body /= "" && String.all (\c -> Char.isDigit c || c == '.') body then
        String.toFloat s

    else
        Nothing


answer : List String -> Maybe AnswerLine
answer ts =
    case ts of
        [ "ed", "hello", proto, build, "part", n, engine ] ->
            Maybe.map2 (\pr pt -> HelloIs { protocol = pr, build = build, part = pt, engine = engine }) (nat proto) (nat n)

        [ "page", n, label, "of", count ] ->
            Maybe.map2 (\a b -> Page { n = a, label = label, count = b }) (nat n) (nat count)

        [ "panel", name, variant ] ->
            Just (Panel { name = name, variant = variant })

        [ "part", n, engine ] ->
            nat n |> Maybe.map (\i -> PartIs i engine)

        [ "param", a, label, kind, lo, hi, step, def ] ->
            param a label kind lo hi step def

        [ "opt", a, i, name ] ->
            nat i |> Maybe.map (\ix -> Opt { addr = Addr a, index = ix, name = name })

        [ "route", label, "next-slice" ] ->
            Just (Route label)

        [ "val", a, norm, stored, text ] ->
            Maybe.map2 (\n s -> Val { addr = Addr a, norm = n, stored = s, text = text }) (num norm) (num stored)

        _ ->
            Nothing


param : String -> String -> String -> String -> String -> String -> String -> Maybe AnswerLine
param a label kind lo hi step def =
    Maybe.map5
        (\k l h s d -> Param { addr = Addr a, label = label, kind = k, min = l, max = h, step = s, default = d })
        (kindOf kind)
        (num lo)
        (num hi)
        (num step)
        (num def)


kindOf : String -> Maybe Kind
kindOf s =
    case s of
        "cont" ->
            Just Cont

        "choice" ->
            Just Choice

        "toggle" ->
            Just Toggle

        _ ->
            Nothing


{-| Split on spaces; a `"quoted"` run is one token, quotes dropped. A quote
closes only before a space or the end, so a quote inside a text survives.
`Nothing` for an unterminated quote.
-}
tokens : String -> Maybe (List String)
tokens s =
    go (String.toList s) [] |> Maybe.map List.reverse


go : List Char -> List String -> Maybe (List String)
go cs acc =
    case cs of
        [] ->
            Just acc

        ' ' :: rest ->
            go rest acc

        '"' :: rest ->
            quoted rest [] |> Maybe.andThen (\( t, after ) -> go after (t :: acc))

        _ ->
            let
                ( w, after ) =
                    spanWord cs []
            in
            go after (w :: acc)


spanWord : List Char -> List Char -> ( String, List Char )
spanWord cs w =
    case cs of
        [] ->
            ( String.fromList (List.reverse w), [] )

        ' ' :: _ ->
            ( String.fromList (List.reverse w), cs )

        c :: rest ->
            spanWord rest (c :: w)


quoted : List Char -> List Char -> Maybe ( String, List Char )
quoted cs w =
    case cs of
        [] ->
            Nothing

        '"' :: [] ->
            Just ( String.fromList (List.reverse w), [] )

        '"' :: ' ' :: rest ->
            Just ( String.fromList (List.reverse w), ' ' :: rest )

        c :: rest ->
            quoted rest (c :: w)
