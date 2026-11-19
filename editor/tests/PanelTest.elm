module PanelTest exposing (suite)

import Expect
import Fixture
import Layout.Algo exposing (Control(..))
import Model
import Protocol exposing (Addr(..), Value(..))
import Schema
import Test exposing (Test, describe, test)
import Ui.Control as Control
import Unit exposing (Engine(..))
import View.Algo as Algo
import View.Controls as Controls


model : Model.Model
model =
    Model.init
        |> (\m -> { m | schema = Fixture.schema, engine = Just Algo })
        |> Model.step (Model.Line "val FILTER.CUTOFF 0.5 632.4555 \"632 Hz\"")
        |> Tuple.first
        |> Model.step (Model.Line "val FILTER.KIND 0 0 \"CLEAN\"")
        |> Tuple.first


cutoff : Addr
cutoff =
    Addr "FILTER.CUTOFF"


cutoffParam : Schema.Param
cutoffParam =
    Fixture.param "FILTER.CUTOFF"


run : List Algo.Msg -> ( Algo.State, List Model.Msg )
run =
    List.foldl
        (\msg ( s, out ) ->
            Algo.update msg model s |> Tuple.mapSecond (\m -> out ++ m)
        )
        ( Algo.init, [] )


grab : Algo.Msg
grab =
    Algo.Hand (Controls.Grab cutoff Hero 300)


suite : Test
suite =
    describe "ALGO panel edits"
        [ test "a_drag_asks_nothing_until_the_frame" <|
            \_ ->
                run [ grab, Algo.Moved 290, Algo.Moved 280 ]
                    |> Tuple.second
                    |> Expect.equal []
        , test "a_whole_drag_in_one_frame_sends_one_set_for_the_control" <|
            \_ ->
                let
                    ( _, out ) =
                        run [ grab, Algo.Moved 290, Algo.Moved 280, Algo.Moved 270, Algo.Frame ]
                in
                Expect.equal 1 (List.length out)
        , test "that_one_set_is_the_last_position" <|
            \_ ->
                let
                    ( _, out ) =
                        run [ grab, Algo.Moved 290, Algo.Moved 240, Algo.Frame ]

                    -- 0.5 + 60 / 240 = 0.75 of ten octaves above 20 Hz
                    expected : Model.Msg
                    expected =
                        Model.Edit cutoff (Num (Control.atNorm cutoffParam 0.75))
                in
                Expect.equal [ expected ] out
        , test "the_next_frame_sends_nothing_new" <|
            \_ ->
                run [ grab, Algo.Moved 290, Algo.Frame, Algo.Frame ]
                    |> Tuple.second
                    |> List.length
                    |> Expect.equal 1
        , test "a_released_drag_still_flushes_its_last_want" <|
            \_ ->
                run [ grab, Algo.Moved 250, Algo.Released, Algo.Frame ]
                    |> Tuple.second
                    |> List.length
                    |> Expect.equal 1
        , test "a_choice_click_sends_a_pick_once_per_frame" <|
            \_ ->
                run [ Algo.Hand (Controls.Pick (Addr "FILTER.KIND") 1), Algo.Hand (Controls.Pick (Addr "FILTER.KIND") 2), Algo.Frame ]
                    |> Tuple.second
                    |> Expect.equal (Protocol.optIndex 2 |> Maybe.map (Model.Pick (Addr "FILTER.KIND")) |> Maybe.map List.singleton |> Maybe.withDefault [])
        , test "a_sideways_wheel_edits_nothing" <|
            \_ ->
                run [ Algo.Hand (Controls.Wheel cutoff 0), Algo.Frame ]
                    |> Tuple.second
                    |> Expect.equal []
        ]
