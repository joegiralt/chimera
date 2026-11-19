module ScreenTest exposing (suite)

import Expect
import Html.Attributes exposing (attribute)
import Json.Encode as Encode
import Test exposing (Test, describe, test)
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control
import Ui.Pad as Pad
import Ui.Screen as Screen exposing (Focus(..), Glyph(..), Shape(..))
import Unit exposing (Engine(..))
import Wire exposing (addr, page)


{-| ALGO's page 0 (ALG), drawn by the firmware, with v6's crop under it.
-}
chart : Screen.Config
chart =
    Screen.drawn Algo (page 0) Strip |> Screen.fallback AlgoChart |> Screen.caption "ALGORITHM"


{-| The `spec` attribute a screen hands `<chimera-screen>`.
-}
specOf : Screen.Config -> String
specOf c =
    Screen.spec c |> Maybe.map (Screen.encodeSpec >> Encode.encode 0) |> Maybe.withDefault ""


suite : Test
suite =
    describe "Ui.Screen"
        [ test "it_is_the_chimera_screen_element_with_one_spec_attribute" <|
            \_ ->
                Screen.view (Screen.drawn Algo (page 22) Tall)
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "chimera-screen" ]
                    |> Query.has [ Sel.attribute (attribute "spec" """{"engine":"ALGO","page":22,"region":"screen","focus":null,"pad":null,"vals":[]}""") ]
        , test "the_vals_are_the_stored_values_in_the_order_given" <|
            \_ ->
                specOf (Screen.values [ ( addr "ALGO.MORPH", 40 ), ( addr "OP_A.WAVE", 9 ), ( addr "FILTER.CUTOFF", 632.5 ) ] chart)
                    |> Expect.equal """{"engine":"ALGO","page":0,"region":"screen","focus":null,"pad":null,"vals":[["ALGO.MORPH",40],["OP_A.WAVE",9],["FILTER.CUTOFF",632.5]]}"""
        , test "the_focus_is_the_cells_number" <|
            \_ ->
                specOf (Screen.focus CellC chart)
                    |> String.contains "\"focus\":2,"
                    |> Expect.equal True
        , test "a_pad_icon_is_that_pads_padicon_region" <|
            \_ ->
                specOf (Screen.padIcon (page 24) Pad.Pad2)
                    |> Expect.equal """{"engine":"DRUM","page":24,"region":"padicon","focus":null,"pad":2,"vals":[]}"""
        , test "a_kit_screen_says_which_pad_it_draws_for" <|
            \_ ->
                specOf (Screen.drawn Drum (page 22) Strip |> Screen.onPad Pad.Pad2)
                    |> String.contains "\"pad\":2,"
                    |> Expect.equal True
        , test "until_the_wasm_draws_it_shows_the_units_crop" <|
            \_ ->
                Screen.view chart
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "chimera-screen" ]
                    |> Query.find [ Sel.tag "img" ]
                    |> Query.has [ Sel.attribute (attribute "src" "glyphs/algo_chart.png") ]
        , test "a_page_with_no_crop_shows_no_image" <|
            \_ ->
                Screen.view (Screen.drawn Va (page 1) Strip)
                    |> Query.fromHtml
                    |> Query.findAll [ Sel.tag "img" ]
                    |> Query.count (Expect.equal 0)
        , test "a_crop_alone_is_an_image_never_an_element_that_would_hide_it" <|
            \_ ->
                Expect.all
                    [ \_ -> Screen.view (Screen.new Drum (PadIcon Pad.Pad9)) |> Query.fromHtml |> Query.find [ Sel.tag "img" ] |> Query.has [ Sel.attribute (attribute "src" "glyphs/pad9.png") ]
                    , \_ -> Screen.view (Screen.new Drum (PadIcon Pad.Pad9)) |> Query.fromHtml |> Query.findAll [ Sel.tag "chimera-screen" ] |> Query.count (Expect.equal 0)
                    ]
                    ()
        , test "the_caption_is_printed" <|
            \_ ->
                Screen.view chart |> Query.fromHtml |> Query.has [ Sel.text "ALGORITHM" ]
        , test "its_size_comes_from_the_shape_never_the_values" <|
            \_ ->
                Expect.all
                    (List.map
                        (\vs _ -> Screen.view (Screen.values vs chart) |> Query.fromHtml |> Query.has [ Sel.style "width" (Control.widthText (Screen.width chart)), Sel.style "aspect-ratio" "240 / 68" ])
                        [ [], [ ( addr "ALGO.MORPH", 0 ) ], [ ( addr "ALGO.MORPH", 127 ), ( addr "ALGO.ALG_A", 31 ) ] ]
                    )
                    ()
        , test "a_tall_screen_is_the_units_tall_region" <|
            \_ ->
                Screen.view (Screen.drawn Algo (page 22) Tall) |> Query.fromHtml |> Query.has [ Sel.style "aspect-ratio" "240 / 158" ]
        , test "every_pad_has_a_number" <|
            \_ ->
                List.map Pad.number Pad.all |> Expect.equal (List.range 1 12)
        ]
