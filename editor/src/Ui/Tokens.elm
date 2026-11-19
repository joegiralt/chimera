module Ui.Tokens exposing
    ( Chars
    , Face(..)
    , Os(..)
    , Preference
    , Theme(..)
    , Token(..)
    , chars
    , charsCount
    , fromName
    , fromPreference
    , label
    , name
    , oneMore
    , preference
    , readoutChars
    , textPx
    , themed
    , themes
    , var
    )

{-| DAY (the cream Cascadia-style panel) and NIGHT (v3/v4's dark). Every
colour is a CSS token (`public/style.css`); a theme is a `data-theme` on the
UI's root, so the gallery can show both side by side.
-}

import Html
import Html.Attributes exposing (attribute)
import Json.Decode as Decode exposing (Decoder)


type Theme
    = Day
    | Night


{-| What the browser says about its own scheme.
-}
type Os
    = OsDark
    | OsLight


{-| What the shim reads at start: the stored choice (if any), and the OS.
-}
type alias Preference =
    { stored : Maybe String
    , os : Os
    }


themes : List Theme
themes =
    [ Day, Night ]


{-| The flags theme.js hands Elm at start: `{ stored, dark }`. Flags that
don't decode are an OS in light mode with nothing stored.
-}
preference : Decode.Value -> Preference
preference flags =
    Decode.decodeValue preferenceDecoder flags
        |> Result.withDefault { stored = Nothing, os = OsLight }


preferenceDecoder : Decoder Preference
preferenceDecoder =
    Decode.map2 (\s dark -> { stored = s, os = osOf dark })
        (Decode.field "stored" (Decode.nullable Decode.string))
        (Decode.field "dark" Decode.bool)


osOf : Bool -> Os
osOf dark =
    if dark then
        OsDark

    else
        OsLight


{-| A stored choice wins; without one, the OS decides.
-}
fromPreference : Preference -> Theme
fromPreference p =
    case Maybe.andThen fromName p.stored of
        Just t ->
            t

        Nothing ->
            case p.os of
                OsDark ->
                    Night

                OsLight ->
                    Day


name : Theme -> String
name t =
    case t of
        Day ->
            "day"

        Night ->
            "night"


fromName : String -> Maybe Theme
fromName s =
    case s of
        "day" ->
            Just Day

        "night" ->
            Just Night

        _ ->
            Nothing


{-| The word on the switch.
-}
label : Theme -> String
label =
    name >> String.toUpper


themed : Theme -> Html.Attribute msg
themed t =
    attribute "data-theme" (name t)


{-| The tokens Elm draws with (inline SVG); the rest live only in CSS.
-}
type Token
    = Ink
    | Acc
    | Mod
    | KnobBody
    | KnobFace
    | KnobPointer


var : Token -> String
var t =
    "var(--"
        ++ (case t of
                Ink ->
                    "ink"

                Acc ->
                    "acc"

                Mod ->
                    "mod"

                KnobBody ->
                    "knob"

                KnobFace ->
                    "knob2"

                KnobPointer ->
                    "knob-ptr"
           )
        ++ ")"



-- THE FACES' METRICS


{-| A readout's width in characters of its own face: never less than one.
-}
type Chars
    = Chars Int


chars : Int -> Maybe Chars
chars n =
    if n >= 1 then
        Just (Chars n)

    else
        Nothing


charsCount : Chars -> Int
charsCount (Chars n) =
    n


{-| A character's slack beside a readout.
-}
oneMore : Chars -> Chars
oneMore (Chars n) =
    Chars (n + 1)


{-| The longest text the box's formats print: `1.20 kHz`, `+100`, `C#-1`.
-}
readoutChars : Chars
readoutChars =
    Chars 8


{-| The faces text is set in, each with the pixels a capital takes
(public/style.css sets them).
-}
type Face
    = LabelFace
    | ToggleFace
    | WordFace
    | SelectorFace
    | HeaderValueFace


capitalPx : Face -> Float
capitalPx f =
    case f of
        -- 12px condensed capitals
        LabelFace ->
            7.2

        -- 12px condensed, letter-spaced
        ToggleFace ->
            7.5

        -- the word row's 15px condensed
        WordFace ->
            9.3

        SelectorFace ->
            7

        -- the title bar's 14px mono
        HeaderValueFace ->
            9


{-| Pixels a word takes in a face.
-}
textPx : Face -> String -> Int
textPx f s =
    ceiling (toFloat (String.length s) * capitalPx f)
