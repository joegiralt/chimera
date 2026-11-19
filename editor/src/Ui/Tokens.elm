module Ui.Tokens exposing
    ( Os(..)
    , Preference
    , Theme(..)
    , Token(..)
    , fromName
    , fromPreference
    , label
    , name
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
