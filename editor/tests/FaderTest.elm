module FaderTest exposing (suite)

import Contract
import Json.Encode as Encode
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Direction(..), Msg(..), Travel(..))
import Ui.Fader as Fader
import Ui.Tokens as Tokens
import Wire exposing (addr)


level : Fader.Config
level =
    Fader.new (addr "OP1.LEVEL") (Control.polarity { min = 0, max = 1 }) |> Fader.label "LEVEL"


{-| An amount runs either side of nothing, so it is bipolar.
-}
amount : Fader.Config
amount =
    Fader.new (addr "PAD1_RT1.AMOUNT") (Control.polarity { min = -100, max = 100 }) |> Fader.label "AMOUNT"


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
                    |> Event.expect (Grab (addr "OP1.LEVEL") Fader 9)
        , test "the_wheel_steps_it" <|
            \_ ->
                Fader.view level (Control.reading 0.5 "50")
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "slider") ]
                    |> Event.simulate (Event.custom "wheel" (Encode.object [ ( "deltaY", Encode.float 4 ) ]))
                    |> Event.expect (Step (addr "OP1.LEVEL") Down)
        , test "a_bipolar_fader_marks_its_centre" <|
            \_ ->
                Fader.view amount (Control.reading 0.2 "-60")
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
                [ ( "plain", level ), ( "hero", Fader.hero level ), ( "bipolar", amount ), ( "wide", Tokens.chars 9 |> Maybe.map (\k -> Fader.chars k level) |> Maybe.withDefault level ) ]
            )
        ]
