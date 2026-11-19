module ScreenTest exposing (suite)

import Expect
import Html.Attributes exposing (attribute)
import Protocol exposing (Addr(..))
import Test exposing (Test, describe, test)
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control
import Ui.Pad as Pad
import Ui.Screen as Screen exposing (Glyph(..), Shape(..))
import Unit exposing (Engine(..))


{-| ALGO's page 0 (ALG), drawn by the firmware, with v6's crop under it.
-}
chart : Screen.Config
chart =
    Screen.drawn Algo 0 Strip |> Screen.fallback AlgoChart |> Screen.caption "ALGORITHM"


suite : Test
suite =
    describe "Ui.Screen"
        [ test "it_is_the_chimera_screen_element_with_engine_page_number_and_region" <|
            \_ ->
                Screen.view (Screen.drawn Algo 22 Tall)
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "chimera-screen" ]
                    |> Query.has [ Sel.attribute (attribute "engine" "ALGO"), Sel.attribute (attribute "page" "22"), Sel.attribute (attribute "region" "screen") ]
        , test "the_vals_are_the_stored_values_as_ed_set_takes_them" <|
            \_ ->
                Screen.view (Screen.values [ ( Addr "ALGO.MORPH", 40 ), ( Addr "OP_A.WAVE", 9 ), ( Addr "FILTER.CUTOFF", 632.5 ) ] chart)
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "chimera-screen" ]
                    |> Query.has [ Sel.attribute (attribute "vals" "ALGO.MORPH=40;OP_A.WAVE=9;FILTER.CUTOFF=632.5") ]
        , test "until_the_wasm_draws_it_shows_the_units_crop" <|
            \_ ->
                Screen.view chart
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "chimera-screen" ]
                    |> Query.find [ Sel.tag "img" ]
                    |> Query.has [ Sel.attribute (attribute "src" "glyphs/algo_chart.png") ]
        , test "a_page_with_no_crop_shows_no_image" <|
            \_ ->
                Screen.view (Screen.drawn Va 1 Strip)
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
                        (\vs _ -> Screen.view (Screen.values vs chart) |> Query.fromHtml |> Query.has [ Sel.style "width" (Control.widthText (Screen.width chart)), Sel.style "height" "68px" ])
                        [ [], [ ( Addr "ALGO.MORPH", 0 ) ], [ ( Addr "ALGO.MORPH", 127 ), ( Addr "ALGO.ALG_A", 31 ) ] ]
                    )
                    ()
        , test "a_tall_screen_is_the_units_tall_region" <|
            \_ ->
                Screen.view (Screen.drawn Algo 22 Tall) |> Query.fromHtml |> Query.has [ Sel.style "height" "158px" ]
        , test "every_pad_has_a_number" <|
            \_ ->
                List.map Pad.number Pad.all |> Expect.equal (List.range 1 12)
        ]
