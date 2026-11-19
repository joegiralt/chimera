module SelectorTest exposing (suite)

import Contract exposing (opt)
import Expect exposing (Expectation)
import Json.Encode as Encode
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Direction(..), Msg(..))
import Ui.Selector as Selector
import Wire exposing (addr)


modes : List String
modes =
    [ "LP4", "LP2", "LP1", "BP2", "BP4", "HP4", "NOTCH1", "NOTCH2" ]


mode : Maybe Selector.Config
mode =
    Selector.fromOptions (addr "FILTER.MODE") (Contract.options modes) |> Maybe.map (Selector.label "MODE")


{-| A test of the eight-option selector, which must exist.
-}
withMode : (Selector.Config -> Expectation) -> Expectation
withMode f =
    mode |> Maybe.map f |> Maybe.withDefault (Expect.fail "eight options make a selector")


suite : Test
suite =
    describe "Ui.Selector"
        [ test "four_to_eight_options_make_a_selector" <|
            \_ ->
                List.map (\n -> Selector.fromOptions (addr "X") (Contract.options (List.repeat n "A")) /= Nothing) [ 3, 4, 8, 9 ]
                    |> Expect.equal [ False, True, True, False ]
        , test "it_prints_every_position_and_lights_the_current" <|
            \_ ->
                withMode
                    (\sel ->
                        Selector.view sel (Control.chosen (opt 6))
                            |> Query.fromHtml
                            |> Query.find [ Sel.attribute (Control.ariaChecked True) ]
                            |> Query.has [ Sel.text "NOTCH1" ]
                    )
        , test "a_printed_position_picks_it" <|
            \_ ->
                withMode
                    (\sel ->
                        Selector.view sel (Control.chosen (opt 0))
                            |> Query.fromHtml
                            |> Query.find [ Sel.attribute (Control.ariaRole "radio"), Sel.containing [ Sel.text "HP4" ] ]
                            |> Event.simulate Event.click
                            |> Event.expect (Pick (addr "FILTER.MODE") (opt 5))
                    )
        , test "the_small_knob_steps_by_wheel" <|
            \_ ->
                withMode
                    (\sel ->
                        Selector.view sel (Control.chosen (opt 0))
                            |> Query.fromHtml
                            |> Query.find [ Sel.class "kn" ]
                            |> Event.simulate (Event.custom "wheel" (Encode.object [ ( "deltaY", Encode.float -1 ) ]))
                            |> Event.expect (Step (addr "FILTER.MODE") Up)
                    )
        , test "it_is_a_labelled_radio_group" <|
            \_ ->
                withMode
                    (\sel ->
                        Selector.view sel (Control.chosen (opt 0))
                            |> Query.fromHtml
                            |> Query.find [ Sel.attribute (Control.ariaRole "radiogroup") ]
                            |> Query.has [ Sel.attribute (Control.ariaLabel "MODE") ]
                    )
        , describe "the width contract"
            [ test "every_option" <| \_ -> withMode (\sel -> Contract.widthHolds (Selector.width sel) (List.map (\i -> Selector.view sel (Control.chosen (opt i))) (List.range 0 7)))
            ]
        ]
