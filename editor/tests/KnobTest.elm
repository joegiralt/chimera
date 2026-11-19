module KnobTest exposing (suite)

import Contract
import Expect
import Html.Attributes exposing (attribute)
import Json.Encode as Encode
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Msg(..), Travel(..))
import Ui.Knob as Knob
import Ui.Tokens as Tokens
import Wire exposing (addr)


cutoff : Knob.Config
cutoff =
    Knob.new (addr "FILTER.CUTOFF") (Control.polarity { min = 20, max = 20000 }) |> Knob.label "CUTOFF"


{-| TRANSPOSE runs either side of nothing, so it is bipolar.
-}
transpose : Knob.Config
transpose =
    Knob.new (addr "ALGO.TRNSP") (Control.polarity { min = -24, max = 24 }) |> Knob.label "TRNSP"


narrowTo : Int -> Knob.Config -> Knob.Config
narrowTo n c =
    Tokens.chars n |> Maybe.map (\k -> Knob.chars k c) |> Maybe.withDefault c


suite : Test
suite =
    describe "Ui.Knob"
        [ test "it_shows_its_label_and_the_units_text" <|
            \_ ->
                Knob.view cutoff (Control.reading 0.5 "1.20 kHz")
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "CUTOFF", Sel.text "1.20 kHz" ]
        , test "a_refused_reading_wears_the_mark_and_the_units_reason" <|
            \_ ->
                Knob.view cutoff (Control.refused "NOT ON THIS MACHINE" (Control.reading 0.5 "1.20 kHz"))
                    |> Query.fromHtml
                    |> Query.has [ Sel.class "refused", Sel.attribute (attribute "title" "THE UNIT REFUSED: NOT ON THIS MACHINE"), Sel.text "1.20 kHz" ]
        , test "a_value_not_reported_says_so_and_draws_no_pointer" <|
            \_ ->
                Knob.view cutoff Control.missing
                    |> Query.fromHtml
                    |> Expect.all
                        [ Query.has [ Sel.class "missing", Sel.text "--" ]
                        , Query.findAll [ Sel.tag "line", Sel.attribute (attribute "stroke-linecap" "round") ] >> Query.count (Expect.equal 0)
                        ]
        , test "the_hero_is_bigger_and_travels_further" <|
            \_ ->
                Knob.view (Knob.hero cutoff) (Control.reading 0.5 "x")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 3 ) ]))
                    |> Event.expect (Grab (addr "FILTER.CUTOFF") Hero 3)
        , test "a_plain_knob_grabs_with_knob_travel" <|
            \_ ->
                Knob.view cutoff (Control.reading 0.5 "x")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 3 ) ]))
                    |> Event.expect (Grab (addr "FILTER.CUTOFF") Knob 3)
        , test "a_bipolar_knob_draws_its_fill_from_the_centre" <|
            \_ ->
                Knob.view transpose (Control.reading 0.8 "+60")
                    |> Query.fromHtml
                    |> Query.has [ Sel.class "bip" ]
        , test "a_range_that_never_crosses_nothing_is_unipolar" <|
            \_ ->
                Knob.view cutoff (Control.reading 0.8 "1.20 kHz")
                    |> Query.fromHtml
                    |> Query.hasNot [ Sel.class "bip" ]
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
                    , \_ -> Knob.view (narrowTo 4 cutoff) (Control.reading 0 "") |> Query.fromHtml |> Query.find [ Sel.class "v" ] |> Query.has [ Sel.style "width" "4ch" ]
                    ]
                    ()
        , describe "the width contract"
            (List.map
                (\( name, c ) -> test name <| \_ -> Contract.widthHolds (Knob.width c) (List.map (Knob.view c) Contract.readings))
                [ ( "plain", cutoff ), ( "hero", Knob.hero cutoff ), ( "bipolar", transpose ), ( "narrow", narrowTo 3 cutoff ) ]
            )
        ]
