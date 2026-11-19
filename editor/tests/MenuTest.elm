module MenuTest exposing (suite)

import Contract exposing (opt)
import Expect
import Html.Attributes exposing (attribute)
import Json.Encode as Encode
import Protocol exposing (Addr(..))
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control
import Ui.Menu as Menu exposing (Msg(..))
import Ui.Screen as Screen exposing (Glyph(..))
import Unit exposing (Engine(..))


algA : Addr
algA =
    Addr "ALGO.A"


names : List String
names =
    List.map (\i -> "A" ++ String.fromInt i) (List.range 1 9) ++ [ "T1", "A32 LONG" ]


alg : Menu.Config
alg =
    Menu.new algA (Control.nonEmpty ( opt 0, "A1" ) (List.drop 1 (Contract.options names))) |> Menu.label "ALG A"


opened : Menu.State
opened =
    Menu.update (Open algA) |> Tuple.first


suite : Test
suite =
    describe "Ui.Menu"
        [ test "no_options_no_menu" <|
            \_ -> Control.options [] |> Expect.equal Nothing
        , test "it_shows_the_current_options_name" <|
            \_ ->
                Menu.view alg Menu.shut (Control.chosen (opt 9))
                    |> Query.fromHtml
                    |> Query.find [ Sel.class "mn" ]
                    |> Query.has [ Sel.text "T1" ]
        , test "shut_it_lists_nothing_and_a_click_opens_it" <|
            \_ ->
                Expect.all
                    [ \q -> q |> Query.findAll [ Sel.attribute (Control.ariaRole "listbox") ] |> Query.count (Expect.equal 0)
                    , \q -> q |> Query.find [ Sel.class "mn" ] |> Event.simulate Event.click |> Event.expect (Open algA)
                    , \q -> q |> Query.find [ Sel.class "mn" ] |> Query.has [ Sel.attribute (attribute "aria-haspopup" "listbox"), Sel.attribute (attribute "aria-expanded" "false") ]
                    ]
                    (Menu.view alg Menu.shut (Control.chosen (opt 0)) |> Query.fromHtml)
        , test "open_it_lists_every_option_and_a_click_chooses" <|
            \_ ->
                Expect.all
                    [ \q -> q |> Query.findAll [ Sel.attribute (Control.ariaRole "option") ] |> Query.count (Expect.equal 11)
                    , \q -> q |> Query.find [ Sel.attribute (Control.ariaRole "option"), Sel.containing [ Sel.text "A5" ] ] |> Event.simulate Event.click |> Event.expect (Choose algA (opt 4))
                    , \q -> q |> Query.find [ Sel.attribute (attribute "aria-selected" "true") ] |> Query.has [ Sel.text "A1" ]
                    , \q -> q |> Query.find [ Sel.class "mn" ] |> Event.simulate Event.click |> Event.expect Close
                    ]
                    (Menu.view alg opened (Control.chosen (opt 0)) |> Query.fromHtml)
        , test "escape_closes_it" <|
            \_ ->
                Menu.view alg opened (Control.chosen (opt 0))
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "listbox") ]
                    |> Event.simulate (Event.custom "keydown" (Encode.object [ ( "key", Encode.string "Escape" ) ]))
                    |> Event.expect Close
        , test "a_choice_shuts_it_and_says_a_pick" <|
            \_ ->
                Menu.update (Choose algA (opt 3))
                    |> Expect.equal ( Menu.shut, Just (Control.Pick algA (opt 3)) )
        , test "another_menu_opening_shuts_this_one" <|
            \_ ->
                Menu.view alg (Menu.update (Open (Addr "ALGO.B")) |> Tuple.first) (Control.chosen (opt 0))
                    |> Query.fromHtml
                    |> Query.findAll [ Sel.attribute (Control.ariaRole "listbox") ]
                    |> Query.count (Expect.equal 0)
        , test "options_can_carry_the_units_glyphs" <|
            \_ ->
                Menu.view (Menu.glyphs (\_ -> Just (Screen.new Va EnvAdsr)) alg) opened (Control.chosen (opt 0))
                    |> Query.fromHtml
                    |> Query.findAll [ Sel.class "screen" ]
                    |> Query.count (Expect.equal 11)
        , test "in_a_title_bar_it_holds_its_label_beside_its_widest_value" <|
            \_ ->
                -- ALG A (40px) + padding, arrow and gaps (34px) + the widest name in the 14px face (9px a character)
                Control.widthText (Menu.width (Menu.inHeader alg))
                    |> Expect.equal ("max(" ++ String.fromInt (Control.labelPx "ALG A" + 34 + 9 * List.foldl (String.length >> max) 0 names) ++ "px, 0ch)")
        , test "in_a_title_bar_it_wears_the_header_look" <|
            \_ ->
                Menu.view (Menu.inHeader alg) Menu.shut (Control.chosen (opt 0))
                    |> Query.fromHtml
                    |> Query.has [ Sel.class "hbtn" ]
        , describe "the width contract"
            [ test "every_option_shut_and_open" <|
                \_ ->
                    Contract.widthHolds (Menu.width alg)
                        (List.concatMap (\i -> [ Menu.view alg Menu.shut (Control.chosen (opt i)), Menu.view alg opened (Control.chosen (opt i)) ]) (List.range 0 10))
            , test "in_a_header" <|
                \_ ->
                    Contract.widthHolds (Menu.width (Menu.inHeader alg))
                        (List.map (\i -> Menu.view (Menu.inHeader alg) Menu.shut (Control.chosen (opt i))) (List.range 0 10))
            ]
        ]
