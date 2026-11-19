module KnobTest exposing (suite)

import Contract
import Expect
import Json.Encode as Encode
import Protocol exposing (Addr(..))
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Msg(..), Travel(..))
import Ui.Knob as Knob


cutoff : Knob.Config
cutoff =
    Knob.new (Addr "FILTER.CUTOFF") |> Knob.label "CUTOFF"


suite : Test
suite =
    describe "Ui.Knob"
        [ test "it_shows_its_label_and_the_units_text" <|
            \_ ->
                Knob.view cutoff (Control.reading 0.5 "1.20 kHz")
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "CUTOFF", Sel.text "1.20 kHz" ]
        , test "a_pending_reading_wears_the_mark" <|
            \_ ->
                Knob.view cutoff (Control.pending (Control.reading 0.5 "1.20 kHz"))
                    |> Query.fromHtml
                    |> Query.has [ Sel.class "pending" ]
        , test "the_hero_is_bigger_and_travels_further" <|
            \_ ->
                Knob.view (Knob.hero cutoff) (Control.reading 0.5 "x")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 3 ) ]))
                    |> Event.expect (Grab (Addr "FILTER.CUTOFF") Hero 3)
        , test "a_plain_knob_grabs_with_knob_travel" <|
            \_ ->
                Knob.view cutoff (Control.reading 0.5 "x")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 3 ) ]))
                    |> Event.expect (Grab (Addr "FILTER.CUTOFF") Knob 3)
        , test "a_bipolar_knob_draws_its_fill_from_the_centre" <|
            \_ ->
                Knob.view (Knob.bipolar cutoff) (Control.reading 0.8 "+60")
                    |> Query.fromHtml
                    |> Query.has [ Sel.class "bip" ]
        , test "it_is_a_labelled_slider_that_speaks_its_text" <|
            \_ ->
                Knob.view cutoff (Control.reading 0.5 "1.20 kHz")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Query.has [ Sel.attribute (Control.ariaLabel "CUTOFF"), Sel.attribute (Control.ariaValueText "1.20 kHz") ]
        , test "the_readout_is_chars_wide_default_eight" <|
            \_ ->
                Expect.all
                    [ \_ -> Knob.view cutoff (Control.reading 0 "") |> Query.fromHtml |> Query.find [ Sel.class "v" ] |> Query.has [ Sel.style "width" "8ch" ]
                    , \_ -> Knob.view (Knob.chars 4 cutoff) (Control.reading 0 "") |> Query.fromHtml |> Query.find [ Sel.class "v" ] |> Query.has [ Sel.style "width" "4ch" ]
                    ]
                    ()
        , describe "the width contract"
            (List.map
                (\( name, c ) -> test name <| \_ -> Contract.widthHolds (Knob.width c) (List.map (Knob.view c) Contract.readings))
                [ ( "plain", cutoff ), ( "hero", Knob.hero cutoff ), ( "bipolar", Knob.bipolar cutoff ), ( "narrow", Knob.chars 3 cutoff ) ]
            )
        ]
