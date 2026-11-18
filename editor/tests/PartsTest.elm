module PartsTest exposing (suite)

import Expect
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Unit exposing (Part)
import View.Parts as Parts


type Msg
    = Clicked Part


suite : Test
suite =
    describe "View.Parts"
        [ test "a_button_a_part_the_followed_one_marked" <|
            \_ ->
                Parts.view (Unit.partFromInt 3) Clicked
                    |> Query.fromHtml
                    |> Expect.all
                        [ Query.findAll [ Sel.tag "button" ] >> Query.count (Expect.equal 6)
                        , Query.findAll [ Sel.tag "button", Sel.class "on" ] >> Query.count (Expect.equal 1)
                        , Query.find [ Sel.tag "button", Sel.class "on" ] >> Query.has [ Sel.text "3" ]
                        ]
        , test "nothing_followed_marks_none" <|
            \_ ->
                Parts.view Nothing Clicked
                    |> Query.fromHtml
                    |> Query.findAll [ Sel.tag "button", Sel.class "on" ]
                    |> Query.count (Expect.equal 0)
        , test "a_click_asks_for_its_part" <|
            \_ ->
                case Unit.partFromInt 5 of
                    Just p5 ->
                        Parts.view (Unit.partFromInt 1) Clicked
                            |> Query.fromHtml
                            |> Query.find [ Sel.tag "button", Sel.containing [ Sel.text "5" ] ]
                            |> Event.simulate Event.click
                            |> Event.expect (Clicked p5)

                    Nothing ->
                        Expect.fail "no Part 5"
        ]
