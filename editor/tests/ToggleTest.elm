module ToggleTest exposing (suite)

import Contract exposing (opt)
import Expect
import Protocol exposing (Addr(..))
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Msg(..))
import Ui.Toggle as Toggle


steal : Toggle.Config
steal =
    Toggle.two (Addr "PITCH.STEAL") ( opt 0, "CUT" ) ( opt 1, "GLIDE" ) |> Toggle.label "STEAL"


speed : Toggle.Config
speed =
    Toggle.three (Addr "ENV1.SPEED") ( opt 0, "FAST" ) ( opt 1, "MED" ) ( opt 2, "SLOW" ) |> Toggle.label "SPEED"


suite : Test
suite =
    describe "Ui.Toggle"
        [ test "only_two_or_three_options_make_a_toggle" <|
            \_ ->
                List.map (\n -> Toggle.fromOptions (Addr "X") (Contract.options (List.repeat n "A")) /= Nothing) [ 1, 2, 3, 4 ]
                    |> Expect.equal [ False, True, True, False ]
        , test "it_prints_every_position_and_lights_the_current" <|
            \_ ->
                Toggle.view speed (Control.chosen (opt 1))
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaChecked True) ]
                    |> Query.has [ Sel.text "MED" ]
        , test "a_word_picks_its_option" <|
            \_ ->
                Toggle.view speed (Control.chosen (opt 0))
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "radio"), Sel.containing [ Sel.text "SLOW" ] ]
                    |> Event.simulate Event.click
                    |> Event.expect (Pick (Addr "ENV1.SPEED") (opt 2))
        , test "the_lever_goes_to_the_next_position_and_wraps" <|
            \_ ->
                Expect.all
                    [ \_ -> Toggle.view speed (Control.chosen (opt 1)) |> Query.fromHtml |> Query.find [ Sel.class "lv" ] |> Event.simulate Event.click |> Event.expect (Pick (Addr "ENV1.SPEED") (opt 2))
                    , \_ -> Toggle.view steal (Control.chosen (opt 1)) |> Query.fromHtml |> Query.find [ Sel.class "lv" ] |> Event.simulate Event.click |> Event.expect (Pick (Addr "PITCH.STEAL") (opt 0))
                    ]
                    ()
        , test "it_is_a_labelled_radio_group" <|
            \_ ->
                Toggle.view steal (Control.chosen (opt 0))
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "radiogroup") ]
                    |> Query.has [ Sel.attribute (Control.ariaLabel "STEAL") ]
        , test "a_pending_choice_wears_the_mark" <|
            \_ ->
                Toggle.view steal (Control.pendingChoice (Control.chosen (opt 0)))
                    |> Query.fromHtml
                    |> Query.has [ Sel.class "pending" ]
        , describe "the width contract"
            [ test "two" <| \_ -> Contract.widthHolds (Toggle.width steal) (List.map (Control.chosen >> Toggle.view steal) [ opt 0, opt 1, opt 0, opt 1, opt 0 ])
            , test "three" <| \_ -> Contract.widthHolds (Toggle.width speed) (List.map (Control.chosen >> Toggle.view speed) [ opt 0, opt 1, opt 2, opt 1, opt 0 ])
            ]
        ]
