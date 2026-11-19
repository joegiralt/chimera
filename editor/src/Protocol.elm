module Protocol exposing
    ( Addr(..)
    , AnswerLine(..)
    , Command(..)
    , Flags
    , Hash
    , Kind(..)
    , Line(..)
    , OptIndex
    , Scale(..)
    , SchemaId(..)
    , Value(..)
    , addrKey
    , encode
    , hashText
    , optIndex
    , parse
    , schemaToken
    )

{-| The unit's text console, `ed` namespace: one command out, answer lines in.
Parsing never fails: a line that isn't understood is `Unknown` and is ignored.
-}

import Unit exposing (Engine, Gen, Part)


{-| A parameter address, `OP3.CRSE`. The unit's own text, opaque here.
-}
type Addr
    = Addr String


addrKey : Addr -> String
addrKey (Addr a) =
    a


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


{-| What else may target a parameter, as the unit's rules say (ADR 0289):
a MIDI map (`m`), the mod matrix (`d`).
-}
type alias Flags =
    { mappable : Bool, modulation : Bool }


{-| A schema the unit describes: an engine's pages, or the mixer's.
-}
type SchemaId
    = EngineSchema Engine
    | MixerSchema


{-| A schema's hash, sixteen hex digits: describe's own bytes, so equal
hashes are the same text. Opaque: only `parse` makes one.
-}
type Hash
    = Hash String


hashText : Hash -> String
hashText (Hash h) =
    h


schemaToken : SchemaId -> String
schemaToken s =
    case s of
        EngineSchema e ->
            Unit.engineName e

        MixerSchema ->
            "MIXER"


schemaFromToken : String -> Maybe SchemaId
schemaFromToken t =
    if t == "MIXER" then
        Just MixerSchema

    else
        Unit.engineFromName t |> Maybe.map EngineSchema


type Command
    = Hello
    | AskSchema SchemaId
    | Describe SchemaId Int
    | Get Addr
    | Set Addr Value
    | Since (Maybe Gen)
    | Follow Part


type AnswerLine
    = HelloIs { protocol : Int, build : String, part : Int, engine : String }
    | SchemaIs { schema : SchemaId, pages : Int, hash : Hash }
    | Page { n : Int, label : String, count : Int }
    | Panel { name : String, variant : String }
    | Param { addr : Addr, label : String, kind : Kind, scale : Scale, min : Float, max : Float, step : Float, default : Float, flags : Flags }
    | Opt { addr : Addr, index : OptIndex, name : String }
    | Route String
    | Source String
    | Val { addr : Addr, norm : Float, stored : Float, text : String }
    | PartIs Int String
      -- The MIXER section of `since`, whole: what was held for it is dropped.
    | MixerIs


type Line
    = Answer AnswerLine
    | Ok_ (Maybe Int)
      -- `OK <m> more`: a `since` answer with sections still owed; ask again.
    | OkMore Int
    | Queued
    | Err_ String
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

        AskSchema s ->
            Just ("ed schema " ++ schemaToken s)

        Describe s page ->
            Just ("ed describe " ++ schemaToken s ++ " " ++ String.fromInt page)

        Get (Addr a) ->
            word a |> Maybe.map (\x -> "ed get " ++ x)

        Set (Addr a) (Num n) ->
            Maybe.map2 (\x v -> "ed set " ++ x ++ " " ++ v) (word a) (decimal n)

        Set (Addr a) (Name n) ->
            Maybe.map2 (\x v -> "ed set " ++ x ++ " " ++ v) (word a) (word n)

        Set (Addr a) (Index (OptIndex i)) ->
            word a |> Maybe.map (\x -> "ed set " ++ x ++ " #" ++ String.fromInt i)

        Since held ->
            Just ("ed since " ++ String.fromInt (held |> Maybe.map Unit.genToInt |> Maybe.withDefault 0))

        Follow p ->
            Just ("ed part " ++ String.fromInt (Unit.partNumber p))


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
                case String.split " " m of
                    [ n ] ->
                        nat n |> Maybe.map (Just >> Ok_) |> Maybe.withDefault Unknown

                    [ n, "more" ] ->
                        nat n |> Maybe.map OkMore |> Maybe.withDefault Unknown

                    _ ->
                        Unknown

    else
        case tokens line of
            Just ts ->
                answer ts |> Maybe.map Answer |> Maybe.withDefault Unknown

            Nothing ->
                Unknown


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

        [ "schema", token, pages, hash ] ->
            Maybe.map3 (\sc n h -> SchemaIs { schema = sc, pages = n, hash = h }) (schemaFromToken token) (nat pages) (hashOf hash)

        [ "mixer" ] ->
            Just MixerIs

        [ "page", n, label, "of", count ] ->
            Maybe.map2 (\a b -> Page { n = a, label = label, count = b }) (nat n) (nat count)

        [ "panel", name, variant ] ->
            Just (Panel { name = name, variant = variant })

        [ "part", n, engine ] ->
            nat n |> Maybe.map (\i -> PartIs i engine)

        [ "param", a, label, kind, scale, lo, hi, step, def, flags ] ->
            Maybe.map2 Tuple.pair (scaleOf scale) (flagsOf flags)
                |> Maybe.andThen (\( sc, fl ) -> param a label kind sc fl ( lo, hi ) ( step, def ))

        [ "opt", a, i, name ] ->
            nat i |> Maybe.andThen optIndex |> Maybe.map (\ix -> Opt { addr = Addr a, index = ix, name = name })

        [ "route", label, "next-slice" ] ->
            Just (Route label)

        [ "source", text ] ->
            Just (Source text)

        [ "val", a, norm, stored, text ] ->
            Maybe.map2 (\n s -> Val { addr = Addr a, norm = n, stored = s, text = text }) (num norm) (num stored)

        _ ->
            Nothing


param : String -> String -> String -> Scale -> Flags -> ( String, String ) -> ( String, String ) -> Maybe AnswerLine
param a label kind scale flags ( lo, hi ) ( step, def ) =
    Maybe.map5
        (\k l h s d -> Param { addr = Addr a, label = label, kind = k, scale = scale, min = l, max = h, step = s, default = d, flags = flags })
        (kindOf kind)
        (num lo)
        (num hi)
        (num step)
        (num def)


{-| `m`, `d`, `md` or `-`: nothing else.
-}
flagsOf : String -> Maybe Flags
flagsOf s =
    case s of
        "-" ->
            Just { mappable = False, modulation = False }

        "m" ->
            Just { mappable = True, modulation = False }

        "d" ->
            Just { mappable = False, modulation = True }

        "md" ->
            Just { mappable = True, modulation = True }

        _ ->
            Nothing


{-| Sixteen lowercase hex digits.
-}
hashOf : String -> Maybe Hash
hashOf h =
    if String.length h == 16 && String.all (\c -> Char.isDigit c || (c >= 'a' && c <= 'f')) h then
        Just (Hash h)

    else
        Nothing


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
