module ControlsTest exposing (suite)

import Layout.Algo exposing (Control(..))
import Protocol exposing (Addr(..))
import Test exposing (Test, describe, test)
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import View.Controls as Controls


face : Controls.Face
face =
    { addr = Addr "FILTER.CUTOFF"
    , label = "CUTOFF"
    , text = "1000 Hz"
    , pos = 0.5
    , pending = False
    , options = []
    , current = 0
    }


suite : Test
suite =
    describe "View.Controls"
        [ test "a_value_shows_the_units_text" <|
            \_ ->
                Controls.view Knob face
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "1000 Hz", Sel.text "CUTOFF" ]
        , test "an_unconfirmed_edit_wears_a_pending_mark" <|
            \_ ->
                Controls.view Knob { face | pending = True }
                    |> Query.fromHtml
                    |> Query.has [ Sel.class "pending" ]
        , test "a_confirmed_value_wears_none" <|
            \_ ->
                Controls.view Knob face
                    |> Query.fromHtml
                    |> Query.hasNot [ Sel.class "pending" ]
        , test "a_choice_lists_the_units_option_names_and_lights_the_current" <|
            \_ ->
                Controls.view Choice { face | text = "GLIDE", options = [ ( 0, "CUT" ), ( 1, "GLIDE" ) ], current = 1 }
                    |> Query.fromHtml
                    |> Query.find [ Sel.class "on" ]
                    |> Query.has [ Sel.text "GLIDE" ]
        , test "a_toggle_prints_every_position" <|
            \_ ->
                Controls.view Toggle { face | options = [ ( 0, "CHROMA" ), ( 1, "MUSICAL" ) ] }
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "CHROMA", Sel.text "MUSICAL" ]
        ]
