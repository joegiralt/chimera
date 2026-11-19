module Ui.Screen exposing (Config, Glyph(..), caption, new, values, view, width)

{-| One of the unit's screens. It is a `<chimera-screen>` element that the
port shim draws from the firmware's own renderer (WASM); until it does,
the element shows v6's crop of the same screen, so a page never waits.
-}

import Html exposing (Html, div, img, text)
import Html.Attributes exposing (alt, attribute, class, src, style)
import Protocol exposing (Addr, addrKey)
import Ui.Control as Control exposing (Width)
import Ui.Pad as Pad exposing (Pad)
import Unit exposing (Engine)


{-| A screen the unit draws, by panel and variant.
-}
type Glyph
    = AlgoChart
    | OpWave
    | EnvAdsr
    | EnvFunction
    | LfoClassic
    | FilterClean
    | VaWave
    | PadIcon Pad


type Config
    = Config
        { engine : Engine
        , glyph : Glyph
        , caption : String
        , values : List ( Addr, String )
        }


new : Engine -> Glyph -> Config
new e g =
    Config { engine = e, glyph = g, caption = "", values = [] }


caption : String -> Config -> Config
caption s (Config c) =
    Config { c | caption = s }


{-| What the screen shows, so the element redraws when any of it moves.
-}
values : List ( Addr, String ) -> Config -> Config
values vs (Config c) =
    Config { c | values = vs }


type alias Place =
    { page : String, region : String, file : String, w : Int, h : Int }


place : Glyph -> Place
place g =
    case g of
        AlgoChart ->
            { page = "ALGO", region = "chart", file = "algo_chart", w = 176, h = 112 }

        OpWave ->
            { page = "OP", region = "wave", file = "op_wave", w = 162, h = 53 }

        EnvAdsr ->
            { page = "ENV", region = "adsr", file = "env_a", w = 128, h = 89 }

        EnvFunction ->
            { page = "ENV", region = "function", file = "env_b", w = 128, h = 89 }

        LfoClassic ->
            { page = "LFO", region = "classic", file = "lfo_classic", w = 162, h = 53 }

        FilterClean ->
            { page = "FILTER", region = "clean", file = "flt_clean", w = 128, h = 89 }

        VaWave ->
            { page = "OSC", region = "wave", file = "va_scope", w = 162, h = 53 }

        PadIcon p ->
            { page = "KIT", region = "pad" ++ String.fromInt (Pad.number p), file = "pad" ++ String.fromInt (Pad.number p), w = 56, h = 51 }


width : Config -> Width
width (Config c) =
    Control.width { px = (place c.glyph).w, ch = 0 }


view : Config -> Html msg
view ((Config c) as config) =
    let
        p : Place
        p =
            place c.glyph
    in
    div [ class "screen", Control.widthStyle (width config), style "height" (String.fromInt p.h ++ "px") ]
        [ Html.node "chimera-screen"
            [ attribute "engine" (Unit.engineName c.engine)
            , attribute "page" p.page
            , attribute "region" p.region
            , attribute "values" (String.join ";" (List.map (\( a, v ) -> addrKey a ++ "=" ++ v) c.values))
            ]
            [ img [ src ("glyphs/" ++ p.file ++ ".png"), alt c.caption ] [] ]
        , if c.caption == "" then
            text ""

          else
            div [ class "cap" ] [ text c.caption ]
        ]
