module RowTest exposing (suite)

import Expect exposing (Expectation)
import Html exposing (text)
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Chip as Chip
import Ui.Module as Module exposing (Fold(..))
import Ui.Row as Row


m : String -> Module.Config Module.Msg
m t =
    Module.new Chip.Folding t |> Module.body [ text t ]


titles : Row.Row msg -> List String
titles =
    Row.members >> List.map Module.titleOf


one : List Expectation -> Expectation
one results =
    case results of
        [ e ] ->
            e

        [] ->
            Expect.fail "one row expected, none came"

        _ :: _ :: _ ->
            Expect.fail "one row expected, more came"


suite : Test
suite =
    describe "Ui.Row"
        [ test "modules_pair_up_and_a_full_one_takes_a_row" <|
            \_ ->
                Row.pairUp [ m "A", m "B", Module.full (m "MOD"), m "C", m "D", m "E" ]
                    |> List.map titles
                    |> Expect.equal [ [ "A", "B" ], [ "MOD" ], [ "C", "D" ], [ "E" ] ]
        , test "a_full_module_never_pairs" <|
            \_ ->
                Row.pairUp [ m "A", Module.full (m "MOD"), m "B" ]
                    |> List.map titles
                    |> Expect.equal [ [ "A" ], [ "MOD" ], [ "B" ] ]
        , test "a_lone_half_module_keeps_its_half_beside_a_blank" <|
            \_ ->
                Row.pairUp [ m "A" ]
                    |> List.map (Row.view identity Module.noFolds >> Query.fromHtml >> Query.findAll [ Sel.class "blank" ] >> Query.count (Expect.equal 1))
                    |> one
        , test "a_full_row_is_one_column" <|
            \_ ->
                Row.pairUp [ Module.full (m "MOD") ]
                    |> List.map (Row.view identity Module.noFolds >> Query.fromHtml >> Query.has [ Sel.class "one" ])
                    |> one
        , test "folding_one_of_a_pair_shows_both_folded" <|
            \_ ->
                Row.pairUp [ m "A", m "B" ]
                    |> List.map (Row.view identity (Module.noFolds |> Module.update (Module.SetFold [ Module.idOf (m "A") ] Folded)) >> Query.fromHtml >> Query.findAll [ Sel.class "folded" ] >> Query.count (Expect.equal 2))
                    |> one
        , test "a_fold_handle_folds_the_whole_row" <|
            \_ ->
                Row.pairUp [ m "A", m "B" ]
                    |> List.map
                        (Row.view identity Module.noFolds
                            >> Query.fromHtml
                            >> Query.findAll [ Sel.class "handle" ]
                            >> Query.index 1
                            >> Event.simulate Event.click
                            >> Event.expect (Module.SetFold [ Module.idOf (m "A"), Module.idOf (m "B") ] Folded)
                        )
                    |> one
        , test "a_folded_row_opens_as_one" <|
            \_ ->
                Row.pairUp [ m "A", m "B" ]
                    |> List.map
                        (Row.view identity (Module.noFolds |> Module.update (Module.SetFold [ Module.idOf (m "B") ] Folded))
                            >> Query.fromHtml
                            >> Query.findAll [ Sel.class "handle" ]
                            >> Query.first
                            >> Event.simulate Event.click
                            >> Event.expect (Module.SetFold [ Module.idOf (m "A"), Module.idOf (m "B") ] Open)
                        )
                    |> one
        ]
