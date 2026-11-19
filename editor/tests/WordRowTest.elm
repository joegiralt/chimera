module WordRowTest exposing (suite)

import Contract exposing (opt)
import Protocol exposing (Addr(..))
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Msg(..))
import Ui.WordRow as WordRow


accent : WordRow.Config
accent =
    WordRow.new (Addr "THEME.ACCENT") (Contract.options [ "TEAL", "AMBER", "RED", "PUNCH", "WHITE" ]) |> WordRow.label "ACCENT"


suite : Test
suite =
    describe "Ui.WordRow"
        [ test "it_lights_the_active_word" <|
            \_ ->
                WordRow.view accent (Control.chosen (opt 3))
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaChecked True) ]
                    |> Query.has [ Sel.text "PUNCH" ]
        , test "a_word_picks_its_option" <|
            \_ ->
                WordRow.view accent (Control.chosen (opt 0))
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "radio"), Sel.containing [ Sel.text "RED" ] ]
                    |> Event.simulate Event.click
                    |> Event.expect (Pick (Addr "THEME.ACCENT") (opt 2))
        , test "it_is_a_labelled_radio_group" <|
            \_ ->
                WordRow.view accent (Control.chosen (opt 0))
                    |> Query.fromHtml
                    |> Query.find [ Sel.attribute (Control.ariaRole "radiogroup") ]
                    |> Query.has [ Sel.attribute (Control.ariaLabel "ACCENT") ]
        , describe "the width contract"
            [ test "every_word" <| \_ -> Contract.widthHolds (WordRow.width accent) (List.map (\i -> WordRow.view accent (Control.chosen (opt i))) (List.range 0 4))
            ]
        ]
