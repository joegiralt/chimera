module FaderTest exposing (suite)

import Contract
import Json.Encode as Encode
import Protocol exposing (Addr(..))
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Direction(..), Msg(..), Travel(..))
import Ui.Fader as Fader


level : Fader.Config
level =
    Fader.new (Addr "OP1.LEVEL") |> Fader.label "LEVEL"


suite : Test
suite =
    describe "Ui.Fader"
        [ test "it_shows_its_label_and_the_units_text" <|
            \_ ->
                Fader.view level (Control.reading 0.98 "98")
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "LEVEL", Sel.text "98" ]
        , test "a_drag_grabs_with_fader_travel" <|
            \_ ->
                Fader.view level (Control.reading 0.5 "50")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 9 ) ]))
                    |> Event.expect (Grab (Addr "OP1.LEVEL") Fader 9)
        , test "the_wheel_steps_it" <|
            \_ ->
                Fader.view level (Control.reading 0.5 "50")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Event.simulate (Event.custom "wheel" (Encode.object [ ( "deltaY", Encode.float 4 ) ]))
                    |> Event.expect (Step (Addr "OP1.LEVEL") Down)
        , test "a_bipolar_fader_marks_its_centre" <|
            \_ ->
                Fader.view (Fader.bipolar level) (Control.reading 0.2 "-60")
                    |> Query.fromHtml
                    |> Query.has [ Sel.class "bip" ]
        , test "it_is_a_labelled_slider_that_speaks_its_text" <|
            \_ ->
                Fader.view level (Control.reading 0.5 "50")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Query.has [ Sel.attribute (Control.ariaLabel "LEVEL"), Sel.attribute (Control.ariaValueText "50") ]
        , describe "the width contract"
            (List.map
                (\( name, c ) -> test name <| \_ -> Contract.widthHolds (Fader.width c) (List.map (Fader.view c) Contract.readings))
                [ ( "plain", level ), ( "hero", Fader.hero level ), ( "bipolar", Fader.bipolar level ), ( "wide", Fader.chars 9 level ) ]
            )
        ]
