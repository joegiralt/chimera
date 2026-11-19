module Ui.Screen exposing (Config, Glyph(..), Shape(..), caption, drawn, fallback, new, values, view, width)

{-| One of the unit's screens. A drawn screen is a `<chimera-screen>`
element (public/screen.js): the firmware's own renderer, compiled to WASM,
paints `ed describe`'s page from the stored values, so it moves as they do.
Until the WASM is loaded it shows v6's crop of the same screen, if there is
one. A crop alone (the pad strip's icons) is only an image.
-}

import Html exposing (Html, div, img, text)
import Html.Attributes exposing (alt, attribute, class, src, style)
import Protocol exposing (Addr, addrKey)
import Ui.Control as Control exposing (Width)
import Ui.Pad as Pad exposing (Pad)
import Unit exposing (Engine)


{-| v6's cropped stills: the fallback while the WASM loads.
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


{-| The unit's screen regions: the strip under the focus band (240 by 68),
or the tall one a filter or an envelope takes (240 by 158).
-}
type Shape
    = Strip
    | Tall


type Art
    = Crop Glyph
    | Drawn Int Shape


type Config
    = Config
        { engine : Engine
        , art : Art
        , fallback : Maybe Glyph
        , caption : String
        , values : List ( Addr, Float )
        }


{-| A crop alone.
-}
new : Engine -> Glyph -> Config
new e g =
    Config { engine = e, art = Crop g, fallback = Nothing, caption = "", values = [] }


{-| The firmware's art for `ed describe`'s page `n`.
-}
drawn : Engine -> Int -> Shape -> Config
drawn e n s =
    Config { engine = e, art = Drawn n s, fallback = Nothing, caption = "", values = [] }


fallback : Glyph -> Config -> Config
fallback g (Config c) =
    Config { c | fallback = Just g }


caption : String -> Config -> Config
caption s (Config c) =
    Config { c | caption = s }


{-| The stored values the screen draws, as `ed set` takes them.
-}
values : List ( Addr, Float ) -> Config -> Config
values vs (Config c) =
    Config { c | values = vs }


type alias Still =
    { file : String, w : Int, h : Int }


crop : Glyph -> Still
crop g =
    case g of
        AlgoChart ->
            { file = "algo_chart", w = 176, h = 112 }

        OpWave ->
            { file = "op_wave", w = 162, h = 53 }

        EnvAdsr ->
            { file = "env_a", w = 128, h = 89 }

        EnvFunction ->
            { file = "env_b", w = 128, h = 89 }

        LfoClassic ->
            { file = "lfo_classic", w = 162, h = 53 }

        FilterClean ->
            { file = "flt_clean", w = 128, h = 89 }

        VaWave ->
            { file = "va_scope", w = 162, h = 53 }

        PadIcon p ->
            { file = "pad" ++ String.fromInt (Pad.number p), w = 56, h = 51 }


{-| The box, from the art alone: the unit's pixels one to one.
-}
size : Art -> ( Int, Int )
size art =
    case art of
        Crop g ->
            ( (crop g).w, (crop g).h )

        Drawn _ Strip ->
            ( 240, 68 )

        Drawn _ Tall ->
            ( 240, 158 )


width : Config -> Width
width (Config c) =
    Control.width { px = Tuple.first (size c.art), ch = 0 }


image : String -> Glyph -> Html msg
image what g =
    img [ src ("glyphs/" ++ (crop g).file ++ ".png"), alt what ] []


view : Config -> Html msg
view ((Config c) as config) =
    div [ class "screen", Control.widthStyle (width config), style "height" (String.fromInt (Tuple.second (size c.art)) ++ "px") ]
        [ case c.art of
            Crop g ->
                image c.caption g

            Drawn n _ ->
                Html.node "chimera-screen"
                    [ attribute "engine" (Unit.engineName c.engine)
                    , attribute "page" (String.fromInt n)
                    , attribute "region" "screen"
                    , attribute "vals" (String.join ";" (List.map (\( a, v ) -> addrKey a ++ "=" ++ String.fromFloat v) c.values))
                    ]
                    (c.fallback |> Maybe.map (image c.caption >> List.singleton) |> Maybe.withDefault [])
        , if c.caption == "" then
            text ""

          else
            div [ class "cap" ] [ text c.caption ]
        ]
