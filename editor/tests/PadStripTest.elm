module PadStripTest exposing (suite)

import Expect
import Html.Attributes exposing (attribute)
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Pad as Pad exposing (Pad(..))
import Ui.PadStrip as PadStrip exposing (Msg(..), Tab(..))


names : Pad -> String
names p =
    if p == Pad1 then
        "KICK"

    else if p == Pad2 then
        "SNARE"

    else
        "TOM"


strip : PadStrip.Config
strip =
    PadStrip.new "DRUM INIT" names


suite : Test
suite =
    describe "Ui.PadStrip"
        [ test "a_kit_tab_and_twelve_pads" <|
            \_ ->
                PadStrip.view strip Kit
                    |> Query.fromHtml
                    |> Query.findAll [ Sel.attribute (attribute "role" "tab") ]
                    |> Query.count (Expect.equal 13)
        , test "a_pad_prints_its_number_and_the_units_name" <|
            \_ ->
                PadStrip.view strip Kit
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "01", Sel.text "KICK", Sel.text "02", Sel.text "SNARE", Sel.text "DRUM INIT" ]
        , test "every_pad_shows_its_icon" <|
            \_ ->
                PadStrip.view strip Kit
                    |> Query.fromHtml
                    |> Query.findAll [ Sel.tag "img" ]
                    |> Query.count (Expect.equal 12)
        , test "the_shown_tab_is_selected" <|
            \_ ->
                Expect.all
                    [ \_ -> PadStrip.view strip (PadTab Pad9) |> Query.fromHtml |> Query.find [ Sel.attribute (attribute "aria-selected" "true") ] |> Query.has [ Sel.text "09" ]
                    , \_ -> PadStrip.view strip Kit |> Query.fromHtml |> Query.find [ Sel.attribute (attribute "aria-selected" "true") ] |> Query.has [ Sel.text "KIT" ]
                    ]
                    ()
        , test "a_click_selects_a_pad_or_the_kit" <|
            \_ ->
                Expect.all
                    [ \_ -> PadStrip.view strip Kit |> Query.fromHtml |> Query.find [ Sel.attribute (attribute "role" "tab"), Sel.containing [ Sel.text "12" ] ] |> Event.simulate Event.click |> Event.expect (Select (PadTab Pad12))
                    , \_ -> PadStrip.view strip (PadTab Pad3) |> Query.fromHtml |> Query.find [ Sel.class "kit" ] |> Event.simulate Event.click |> Event.expect (Select Kit)
                    ]
                    ()
        , test "pads_label_as_the_unit_prints_them" <|
            \_ -> List.map Pad.label [ Pad1, Pad12 ] |> Expect.equal [ "01", "12" ]
        ]
