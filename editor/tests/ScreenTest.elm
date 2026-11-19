module ScreenTest exposing (suite)

import Expect
import Html.Attributes exposing (attribute)
import Protocol exposing (Addr(..))
import Test exposing (Test, describe, test)
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control
import Ui.Pad as Pad
import Ui.Screen as Screen exposing (Glyph(..))
import Unit exposing (Engine(..))


chart : Screen.Config
chart =
    Screen.new Algo AlgoChart |> Screen.caption "ALGORITHM"


suite : Test
suite =
    describe "Ui.Screen"
        [ test "it_is_the_chimera_screen_element_with_engine_page_and_region" <|
            \_ ->
                Screen.view chart
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "chimera-screen" ]
                    |> Query.has [ Sel.attribute (attribute "engine" "ALGO"), Sel.attribute (attribute "page" "ALGO"), Sel.attribute (attribute "region" "chart") ]
        , test "the_values_key_names_what_it_shows" <|
            \_ ->
                Screen.view (Screen.values [ ( Addr "ALGO.MORPH", "40" ), ( Addr "ALGO.A", "T1" ) ] chart)
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "chimera-screen" ]
                    |> Query.has [ Sel.attribute (attribute "values" "ALGO.MORPH=40;ALGO.A=T1") ]
        , test "until_the_element_draws_it_shows_the_units_crop" <|
            \_ ->
                Screen.view (Screen.new Va EnvFunction)
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "img" ]
                    |> Query.has [ Sel.attribute (attribute "src" "glyphs/env_b.png") ]
        , test "a_pad_shows_its_own_icon" <|
            \_ ->
                Screen.view (Screen.new Drum (PadIcon Pad.Pad9))
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "img" ]
                    |> Query.has [ Sel.attribute (attribute "src" "glyphs/pad9.png") ]
        , test "the_caption_is_printed" <|
            \_ ->
                Screen.view chart |> Query.fromHtml |> Query.has [ Sel.text "ALGORITHM" ]
        , test "its_size_comes_from_the_glyph_never_the_values" <|
            \_ ->
                Expect.all
                    (List.map
                        (\vs _ -> Screen.view (Screen.values vs chart) |> Query.fromHtml |> Query.has [ Sel.style "width" (Control.widthText (Screen.width chart)) ])
                        [ [], [ ( Addr "ALGO.MORPH", "0" ) ], [ ( Addr "ALGO.MORPH", "127" ), ( Addr "ALGO.A", "A17 IS LONG" ) ] ]
                    )
                    ()
        , test "every_pad_has_a_number" <|
            \_ ->
                List.map Pad.number Pad.all |> Expect.equal (List.range 1 12)
        ]
