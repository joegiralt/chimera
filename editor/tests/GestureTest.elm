module GestureTest exposing (suite)

import Expect
import Fixture
import Gesture exposing (Scale(..), Want(..))
import Protocol exposing (Addr(..))
import Schema exposing (Param)
import Test exposing (Test, describe, test)


param : String -> Param
param =
    Fixture.param


suite : Test
suite =
    describe "Gesture"
        [ describe "coalescing"
            [ test "many_wants_for_one_control_flush_as_one_the_last" <|
                \_ ->
                    Gesture.empty
                        |> Gesture.want (Addr "FILTER.CUTOFF") (Number 100)
                        |> Gesture.want (Addr "FILTER.CUTOFF") (Number 200)
                        |> Gesture.want (Addr "FILTER.CUTOFF") (Number 300)
                        |> Gesture.flush
                        |> Tuple.first
                        |> Expect.equal [ ( Addr "FILTER.CUTOFF", Number 300 ) ]
            , test "two_controls_flush_one_each" <|
                \_ ->
                    Gesture.empty
                        |> Gesture.want (Addr "FILTER.RESO") (Number 0.5)
                        |> Gesture.want (Addr "FILTER.CUTOFF") (Number 300)
                        |> Gesture.want (Addr "FILTER.RESO") (Number 0.6)
                        |> Gesture.flush
                        |> Tuple.first
                        |> Expect.equal [ ( Addr "FILTER.CUTOFF", Number 300 ), ( Addr "FILTER.RESO", Number 0.6 ) ]
            , test "a_flush_empties_so_the_next_frame_sends_nothing" <|
                \_ ->
                    Gesture.empty
                        |> Gesture.want (Addr "FILTER.RESO") (Number 0.5)
                        |> Gesture.flush
                        |> Tuple.second
                        |> Gesture.flush
                        |> Tuple.first
                        |> Expect.equal []
            , test "an_option_is_a_want_too" <|
                \_ ->
                    Gesture.empty
                        |> Gesture.want (Addr "FILTER.KIND") (Option 1)
                        |> Gesture.want (Addr "FILTER.KIND") (Option 2)
                        |> Gesture.flush
                        |> Tuple.first
                        |> Expect.equal [ ( Addr "FILTER.KIND", Option 2 ) ]
            ]
        , describe "where a turn lands"
            [ test "cutoff_turns_by_octaves_the_rate_by_range" <|
                \_ ->
                    Expect.equal [ Octaves, Linear, Linear ]
                        (List.map (param >> Gesture.scaleOf) [ "FILTER.CUTOFF", "LFO1.RATE", "FILTER.RESO" ])
            , test "norm_maps_the_unit_range" <|
                \_ ->
                    let
                        c =
                            param "FILTER.CUTOFF"
                    in
                    Expect.all
                        [ \_ -> Expect.within (Expect.Absolute 0.01) 20 (Gesture.atNorm c 0)
                        , \_ -> Expect.within (Expect.Absolute 0.01) 20000 (Gesture.atNorm c 1)
                        , \_ -> Expect.within (Expect.Absolute 0.01) 632.46 (Gesture.atNorm c 0.5)
                        , \_ -> Expect.equal 64 (Gesture.atNorm (param "ALGO.MORPH") 0.5 |> round)
                        , \_ -> Expect.equal 0 (Gesture.atNorm (param "ALGO.TRNSP") 0.5)
                        ]
                        ()
            , test "stored_values_are_plain_decimals" <|
                \_ ->
                    Gesture.atNorm (param "FILTER.RESO") (1 / 3)
                        |> String.fromFloat
                        |> String.contains "e"
                        |> Expect.equal False
            , test "a_drag_up_raises_by_pixels_over_the_travel" <|
                \_ ->
                    Expect.within (Expect.Absolute 1.0e-9) 0.7 (Gesture.dragNorm { px = 150, from = 0.2 } 300 225)
            , test "a_drag_stays_in_range" <|
                \_ ->
                    Expect.equal [ 0, 1 ] [ Gesture.dragNorm { px = 150, from = 0.2 } 0 1000, Gesture.dragNorm { px = 150, from = 0.2 } 1000 0 ]
            , test "a_choice_steps_by_pixels_and_stops_at_its_ends" <|
                \_ ->
                    Expect.equal [ 5, 0, 31, 4 ]
                        [ Gesture.dragIndex { n = 32, from = 4 } 100 94
                        , Gesture.dragIndex { n = 32, from = 4 } 100 200
                        , Gesture.dragIndex { n = 32, from = 4 } 100 -900
                        , Gesture.dragIndex { n = 32, from = 4 } 100 100
                        ]
            , test "a_wheel_notch_moves_at_least_one_unit_step" <|
                \_ ->
                    Expect.all
                        [ \_ -> Expect.within (Expect.Absolute 1.0e-9) (1 / 7) (Gesture.wheelNorm (param "OP_A.FDBK"))
                        , \_ -> Expect.within (Expect.Absolute 1.0e-9) (1 / 64) (Gesture.wheelNorm (param "FILTER.CUTOFF"))
                        ]
                        ()
            ]
        ]
