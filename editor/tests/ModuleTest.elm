module ModuleTest exposing (suite)

import Expect
import Html exposing (text)
import Html.Attributes exposing (attribute)
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Module as Module exposing (Fold(..), Jack(..))


type Msg
    = Toggled


filter : Module.Config Msg
filter =
    Module.new "FILTER"
        |> Module.note "THIS PAD"
        |> Module.summary [ ( "FREQ", "1.45 kHz" ), ( "Q", "40" ) ]
        |> Module.jacks [ In "FREQ", In "Q", Out "ENV" ]
        |> Module.body [ text "the controls" ]


suite : Test
suite =
    describe "Ui.Module"
        [ test "the_black_title_bar_prints_title_and_note" <|
            \_ ->
                Module.view { fold = Open, toggle = Toggled } filter
                    |> Query.fromHtml
                    |> Query.find [ Sel.tag "header" ]
                    |> Query.has [ Sel.text "FILTER", Sel.text "THIS PAD" ]
        , test "the_title_bar_is_the_fold_handle" <|
            \_ ->
                Module.view { fold = Open, toggle = Toggled } filter
                    |> Query.fromHtml
                    |> Query.find [ Sel.class "handle" ]
                    |> Event.simulate Event.click
                    |> Event.expect Toggled
        , test "open_it_shows_its_body_and_jacks" <|
            \_ ->
                Module.view { fold = Open, toggle = Toggled } filter
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "the controls", Sel.class "jacks", Sel.attribute (attribute "aria-expanded" "true") ]
        , test "folded_it_shows_the_summary_and_no_body" <|
            \_ ->
                Expect.all
                    [ Query.has [ Sel.class "folded", Sel.text "1.45 kHz", Sel.attribute (attribute "aria-expanded" "false") ]
                    , Query.findAll [ Sel.class "body" ] >> Query.count (Expect.equal 0)
                    , Query.findAll [ Sel.class "jacks" ] >> Query.count (Expect.equal 0)
                    ]
                    (Module.view { fold = Folded, toggle = Toggled } filter |> Query.fromHtml)
        , test "inputs_and_outputs_are_told_apart" <|
            \_ ->
                Module.view { fold = Open, toggle = Toggled } filter
                    |> Query.fromHtml
                    |> Expect.all
                        [ Query.findAll [ Sel.class "jk", Sel.class "in" ] >> Query.count (Expect.equal 2)
                        , Query.findAll [ Sel.class "jk", Sel.class "out" ] >> Query.count (Expect.equal 1)
                        ]
        , test "a_module_is_half_a_row_unless_full" <|
            \_ ->
                Expect.equal [ False, True ] [ Module.isFull filter, Module.isFull (Module.full filter) ]
        , test "its_chip_is_its_title_unless_told" <|
            \_ ->
                Expect.equal [ "FILTER", "VOICE" ] [ Module.chipOf filter, Module.chipOf (Module.chip "VOICE" filter) ]
        , test "folds_are_kept_per_module" <|
            \_ ->
                let
                    folds : Module.Folds
                    folds =
                        Module.noFolds |> Module.update (Module.SetFold [ Module.idOf filter ] Folded)
                in
                Expect.equal [ Folded, Open ] [ Module.foldOf (Module.idOf filter) folds, Module.foldOf (Module.idOf (Module.new "AMP")) folds ]
        ]
