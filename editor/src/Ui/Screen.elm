module Ui.Screen exposing
    ( Config
    , Focus(..)
    , Glyph(..)
    , Region
    , Shape(..)
    , Spec
    , caption
    , drawn
    , encodeSpec
    , fallback
    , focus
    , new
    , onPad
    , padIcon
    , spec
    , values
    , view
    , width
    )

{-| One of the unit's screens. A drawn screen is a `<chimera-screen>`
element (public/screen.js): the firmware's own renderer, compiled to WASM,
paints `ed describe`'s page from the stored values, so it moves as they do.
Elm hands it one attribute, `spec`, a JSON object (`encodeSpec`). Until the
WASM is loaded it shows v6's crop of the same screen, if there is one. A crop
alone is only an image.
-}

import Addr exposing (Addr)
import Html exposing (Html, div, img, text)
import Html.Attributes exposing (alt, attribute, class, src, style)
import Json.Encode as Encode
import Protocol exposing (PageNo)
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


{-| What of the page the renderer paints: the page's art, the focus band's
gauge, the whole screen, or one DRUM pad's machine icon.
-}
type Region
    = ScreenArt
    | PadIconOf Pad


{-| The cell in the focus band, knob A to F.
-}
type Focus
    = CellA
    | CellB
    | CellC


focusIndex : Focus -> Int
focusIndex f =
    case f of
        CellA ->
            0

        CellB ->
            1

        CellC ->
            2


{-| What `<chimera-screen>` is told: the engine and page, the region, the
focused cell, the DRUM pad it draws for, and the stored values it draws, in
`describe` order.
-}
type alias Spec =
    { engine : Engine
    , page : PageNo
    , region : Region
    , focus : Maybe Focus
    , pad : Maybe Pad
    , vals : List ( Addr, Float )
    }


type Art
    = Crop Glyph
    | Drawn { page : PageNo, region : Region, shape : Shape, focus : Maybe Focus, pad : Maybe Pad }


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
drawn : Engine -> PageNo -> Shape -> Config
drawn e n s =
    Config { engine = e, art = Drawn { page = n, region = ScreenArt, shape = s, focus = Nothing, pad = Nothing }, fallback = Nothing, caption = "", values = [] }


{-| Pad `p`'s machine icon, from its MACHINE value on page `n` (its MACHINE
page), the crop of it under it until the WASM draws.
-}
padIcon : PageNo -> Pad -> Config
padIcon n p =
    Config { engine = Unit.Drum, art = Drawn { page = n, region = PadIconOf p, shape = Strip, focus = Nothing, pad = Just p }, fallback = Just (PadIcon p), caption = "", values = [] }


{-| The cell the unit's focus band is on while it draws this page.
-}
focus : Focus -> Config -> Config
focus f (Config c) =
    case c.art of
        Drawn d ->
            Config { c | art = Drawn { d | focus = Just f } }

        Crop _ ->
            Config c


{-| The DRUM pad the unit has selected while it draws this page.
-}
onPad : Pad -> Config -> Config
onPad p (Config c) =
    case c.art of
        Drawn d ->
            Config { c | art = Drawn { d | pad = Just p } }

        Crop _ ->
            Config c


fallback : Glyph -> Config -> Config
fallback g (Config c) =
    Config { c | fallback = Just g }


caption : String -> Config -> Config
caption s (Config c) =
    Config { c | caption = s }


{-| The stored values the screen draws, as `ed set` takes them, in
`describe` order.
-}
values : List ( Addr, Float ) -> Config -> Config
values vs (Config c) =
    Config { c | values = vs }


{-| What the element is told, for a drawn screen.
-}
spec : Config -> Maybe Spec
spec (Config c) =
    case c.art of
        Drawn d ->
            Just { engine = c.engine, page = d.page, region = d.region, focus = d.focus, pad = d.pad, vals = c.values }

        Crop _ ->
            Nothing


encodeSpec : Spec -> Encode.Value
encodeSpec s =
    Encode.object
        [ ( "engine", Encode.string (Unit.engineName s.engine) )
        , ( "page", Encode.int (Protocol.pageNoToInt s.page) )
        , ( "region", Encode.string (regionName s.region) )
        , ( "focus", s.focus |> Maybe.map (focusIndex >> Encode.int) |> Maybe.withDefault Encode.null )
        , ( "pad", s.pad |> Maybe.map (Pad.number >> Encode.int) |> Maybe.withDefault Encode.null )
        , ( "vals", Encode.list (\( a, v ) -> Encode.list identity [ Encode.string (Addr.key a), Encode.float v ]) s.vals )
        ]


regionName : Region -> String
regionName r =
    case r of
        ScreenArt ->
            "screen"

        PadIconOf _ ->
            "padicon"


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

        Drawn d ->
            case ( d.region, d.shape ) of
                ( PadIconOf p, _ ) ->
                    ( (crop (PadIcon p)).w, (crop (PadIcon p)).h )

                ( ScreenArt, Strip ) ->
                    ( 240, 68 )

                ( ScreenArt, Tall ) ->
                    ( 240, 158 )


width : Config -> Width
width (Config c) =
    Control.pxWidth (Tuple.first (size c.art))


image : String -> Glyph -> Html msg
image what g =
    img [ src ("glyphs/" ++ (crop g).file ++ ".png"), alt what ] []


{-| The box keeps the unit's aspect and never grows past its module: a
narrow module scales it down, never clips it.
-}
view : Config -> Html msg
view ((Config c) as config) =
    let
        ( w, h ) =
            size c.art
    in
    div [ class "screen", Control.widthStyle (width config), style "aspect-ratio" (String.fromInt w ++ " / " ++ String.fromInt h) ]
        [ case spec config of
            Just s ->
                Html.node "chimera-screen"
                    [ attribute "spec" (Encode.encode 0 (encodeSpec s)) ]
                    (c.fallback |> Maybe.map (image c.caption >> List.singleton) |> Maybe.withDefault [])

            Nothing ->
                case c.art of
                    Crop g ->
                        image c.caption g

                    Drawn _ ->
                        text ""
        , if c.caption == "" then
            text ""

          else
            div [ class "cap" ] [ text c.caption ]
        ]
