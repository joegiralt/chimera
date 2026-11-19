module Protocol exposing
    ( AnswerLine(..)
    , Command(..)
    , Kind(..)
    , Line(..)
    , OptIndex
    , PageLabel
    , PageNo
    , Scale(..)
    , Value(..)
    , encode
    , firstOption
    , firstPage
    , nextPage
    , optIndex
    , pageLabel
    , pageLabelFrom
    , pageNo
    , pageNoToInt
    , parse
    )

{-| The unit's text console, `ed` namespace: one command out, answer lines in.
Parsing never fails: a line that isn't understood is `Unknown` and is ignored.
-}

import Addr exposing (Addr)
import Unit exposing (Engine, Gen, Part)


{-| A page of `ed describe`, by the unit's number for it: 0 is the first.
-}
type PageNo
    = PageNo Int


pageNo : Int -> Maybe PageNo
pageNo n =
    if n >= 0 then
        Just (PageNo n)

    else
        Nothing


firstPage : PageNo
firstPage =
    PageNo 0


pageNoToInt : PageNo -> Int
pageNoToInt (PageNo n) =
    n


nextPage : PageNo -> PageNo
nextPage (PageNo n) =
    PageNo (n + 1)


{-| A page's name as the unit gives it (`FLT`, `PAD3_MCH`), read once.
-}
type PageLabel
    = PageLabel String


{-| A label off the wire: one word.
-}
pageLabelFrom : String -> Maybe PageLabel
pageLabelFrom s =
    word s |> Maybe.map PageLabel


pageLabel : PageLabel -> String
pageLabel (PageLabel l) =
    l


{-| A choice is sent by its option's number (`#2`), so a name with a space
(`GATE EXT`) is sent too. `Name` stays for a human's own words.
-}
type Value
    = Num Float
    | Name String
    | Index OptIndex


{-| A choice's option by its number, as `opt` lines give it: never
negative, so `#n` is always a number the unit reads.
-}
type OptIndex
    = OptIndex Int


{-| A choice's first option, `#0`.
-}
firstOption : OptIndex
firstOption =
    OptIndex 0


optIndex : Int -> Maybe OptIndex
optIndex i =
    if i >= 0 then
        Just (OptIndex i)

    else
        Nothing


{-| How a value moves under a hand, as the unit describes it.
-}
type Scale
    = Linear
    | Octaves


type Kind
    = Cont
    | Choice
    | Toggle


type Command
    = Hello
    | Describe Engine PageNo
    | Get Addr
    | Set Addr Value
    | Since (Maybe Gen)
    | Follow Part


type AnswerLine
    = HelloIs { protocol : Int, build : String, part : Int, engine : String }
    | Page { n : PageNo, label : PageLabel, count : Int }
    | Panel { name : String, variant : String }
    | Param { addr : Addr, label : String, kind : Kind, scale : Scale, min : Float, max : Float, step : Float, default : Float }
    | Opt { addr : Addr, index : OptIndex, name : String }
    | Route String
    | Source String
    | Val { addr : Addr, norm : Float, stored : Float, text : String }
    | PartIs Int String
      -- A set the unit took and then would not apply, and the box's reason
      -- (its own toast's words).
    | Refused Addr String


type Line
    = Answer AnswerLine
    | Ok_ (Maybe Int)
    | Queued
    | Err_ String
      -- `ERR ed set <addr> <why>`: a set the unit would not take.
    | SetRefused Addr String
      -- `ERR ed since too large` (ADR 0285): the unit's whole answer is past
      -- what one answer may carry. Asking again gets the same.
    | SinceTooLarge
    | Unknown


{-| The line to send, or `Nothing` if it could not be one command: an address
or name with whitespace or a control character would split or inject a second
command, and a float that is not a plain decimal (NaN, Infinity, `1.0e-7`)
is not one the unit reads.
-}
encode : Command -> Maybe String
encode c =
    case c of
        Hello ->
            Just "ed hello"

        Describe engine (PageNo page) ->
            Just ("ed describe " ++ Unit.engineName engine ++ " " ++ String.fromInt page)

        Get a ->
            wire a |> Maybe.map (\k -> "ed get " ++ k)

        Set a v ->
            wire a |> Maybe.andThen (\k -> setValue v |> Maybe.map (\x -> "ed set " ++ k ++ " " ++ x))

        Since held ->
            Just ("ed since " ++ String.fromInt (held |> Maybe.map Unit.genToInt |> Maybe.withDefault 0))

        Follow p ->
            Just ("ed part " ++ String.fromInt (Unit.partNumber p))


{-| The address, if it is one the unit could write.
-}
wire : Addr -> Maybe String
wire a =
    if Addr.onWire a then
        Just (Addr.key a)

    else
        Nothing


setValue : Value -> Maybe String
setValue v =
    case v of
        Num n ->
            decimal n

        Name n ->
            word n

        Index (OptIndex i) ->
            Just ("#" ++ String.fromInt i)


word : String -> Maybe String
word w =
    if w /= "" && String.all (\ch -> Char.toCode ch > 32 && Char.toCode ch /= 127) w then
        Just w

    else
        Nothing


decimal : Float -> Maybe String
decimal n =
    let
        t : String
        t =
            String.fromFloat n
    in
    if isNaN n || isInfinite n || String.contains "e" t then
        Nothing

    else
        Just t


parse : String -> Line
parse raw =
    let
        line : String
        line =
            stripCr raw

        trimmed : String
        trimmed =
            String.trimLeft line
    in
    if trimmed == "ERR ed since too large" then
        SinceTooLarge

    else if String.startsWith "ERR ed set " trimmed then
        setRefusal (String.dropLeft 11 trimmed)

    else if trimmed == "ERR" || String.startsWith "ERR " trimmed then
        -- By prefix, not tokens: a refusal's text may hold a stray quote.
        Err_ (String.trim (String.dropLeft 3 trimmed))

    else if trimmed == "OK" || String.startsWith "OK " trimmed then
        case String.trim (String.dropLeft 2 trimmed) of
            "" ->
                Ok_ Nothing

            "queued" ->
                Queued

            m ->
                nat m |> Maybe.map (Just >> Ok_) |> Maybe.withDefault Unknown

    else
        case tokens line of
            Just ts ->
                answer ts |> Maybe.map Answer |> Maybe.withDefault Unknown

            Nothing ->
                Unknown


{-| `<addr> <why>`, or `has no <addr> here`; anything else is a plain `ERR`.
-}
setRefusal : String -> Line
setRefusal rest =
    let
        plain : Line
        plain =
            Err_ ("ed set " ++ rest)
    in
    case String.words rest of
        "has" :: "no" :: a :: _ ->
            Addr.fromWire a |> Maybe.map (\x -> SetRefused x rest) |> Maybe.withDefault plain

        a :: ((_ :: _) as why) ->
            Addr.fromWire a |> Maybe.map (\x -> SetRefused x (String.join " " why)) |> Maybe.withDefault plain

        [ _ ] ->
            plain

        [] ->
            plain


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
        body : String
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
            Maybe.map3 (\a l b -> Page { n = PageNo a, label = l, count = b }) (nat n) (pageLabelFrom label) (nat count)

        [ "panel", name, variant ] ->
            Just (Panel { name = name, variant = variant })

        [ "part", n, engine ] ->
            nat n |> Maybe.map (\i -> PartIs i engine)

        [ "param", a, label, kind, scale, lo, hi, step, def ] ->
            Maybe.andThen (\sc -> param a label kind sc lo hi step def) (scaleOf scale)

        [ "opt", a, i, name ] ->
            Maybe.map2 (\x ix -> Opt { addr = x, index = ix, name = name }) (Addr.fromWire a) (nat i |> Maybe.andThen optIndex)

        [ "route", label, "next-slice" ] ->
            Just (Route label)

        [ "source", text ] ->
            Just (Source text)

        [ "val", a, norm, stored, text ] ->
            Maybe.map3 (\x n s -> Val { addr = x, norm = n, stored = s, text = text }) (Addr.fromWire a) (num norm) (num stored)

        [ "refused", a, why ] ->
            Addr.fromWire a |> Maybe.map (\x -> Refused x why)

        _ ->
            Nothing


param : String -> String -> String -> Scale -> String -> String -> String -> String -> Maybe AnswerLine
param a label kind scale lo hi step def =
    Maybe.map5
        (\( x, k ) l h s d -> Param { addr = x, label = label, kind = k, scale = scale, min = l, max = h, step = s, default = d })
        (Maybe.map2 Tuple.pair (Addr.fromWire a) (kindOf kind))
        (num lo)
        (num hi)
        (num step)
        (num def)


scaleOf : String -> Maybe Scale
scaleOf s =
    case s of
        "lin" ->
            Just Linear

        "oct" ->
            Just Octaves

        _ ->
            Nothing


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
