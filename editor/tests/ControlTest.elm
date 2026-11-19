module ControlTest exposing (suite)

import Expect
import Fixture
import Protocol exposing (Addr(..), Scale(..))
import Schema exposing (Param)
import Test exposing (Test, describe, test)
import Ui.Control as Control exposing (Want(..))


param : String -> Param
param =
    Fixture.param


suite : Test
suite =
    describe "Ui.Control"
        [ describe "coalescing"
            [ test "many_wants_for_one_control_flush_as_one_the_last" <|
                \_ ->
                    Control.empty
                        |> Control.want (Addr "FILTER.CUTOFF") (Number 100)
                        |> Control.want (Addr "FILTER.CUTOFF") (Number 200)
                        |> Control.want (Addr "FILTER.CUTOFF") (Number 300)
                        |> Control.flush
                        |> Tuple.first
                        |> Expect.equal [ ( Addr "FILTER.CUTOFF", Number 300 ) ]
            , test "two_controls_flush_one_each" <|
                \_ ->
                    Control.empty
                        |> Control.want (Addr "FILTER.RESO") (Number 0.5)
                        |> Control.want (Addr "FILTER.CUTOFF") (Number 300)
                        |> Control.want (Addr "FILTER.RESO") (Number 0.6)
                        |> Control.flush
                        |> Tuple.first
                        |> Expect.equal [ ( Addr "FILTER.CUTOFF", Number 300 ), ( Addr "FILTER.RESO", Number 0.6 ) ]
            , test "a_flush_empties_so_the_next_frame_sends_nothing" <|
                \_ ->
                    Control.empty
                        |> Control.want (Addr "FILTER.RESO") (Number 0.5)
                        |> Control.flush
                        |> Tuple.second
                        |> Control.flush
                        |> Tuple.first
                        |> Expect.equal []
            , test "an_option_is_a_want_too" <|
                \_ ->
                    Control.empty
                        |> Control.want (Addr "FILTER.KIND") (Option 1)
                        |> Control.want (Addr "FILTER.KIND") (Option 2)
                        |> Control.flush
                        |> Tuple.first
                        |> Expect.equal [ ( Addr "FILTER.KIND", Option 2 ) ]
            ]
        , describe "where a turn lands"
            [ test "the_unit_says_cutoff_turns_by_octaves_the_rate_by_range" <|
                \_ ->
                    Expect.equal [ Octaves, Linear, Linear ]
                        (List.map (param >> Control.scaleOf) [ "FILTER.CUTOFF", "LFO1.RATE", "FILTER.RESO" ])
            , test "norm_maps_the_unit_range" <|
                \_ ->
                    let
                        c : Param
                        c =
                            param "FILTER.CUTOFF"
                    in
                    Expect.all
                        [ \_ -> Expect.within (Expect.Absolute 0.01) 20 (Control.atNorm c 0)
                        , \_ -> Expect.within (Expect.Absolute 0.01) 20000 (Control.atNorm c 1)
                        , \_ -> Expect.within (Expect.Absolute 0.01) 632.46 (Control.atNorm c 0.5)
                        , \_ -> Expect.equal 64 (Control.atNorm (param "ALGO.MORPH") 0.5 |> round)
                        , \_ -> Expect.equal 0 (Control.atNorm (param "ALGO.TRNSP") 0.5)
                        ]
                        ()
            , test "stored_values_are_plain_decimals" <|
                \_ ->
                    Control.atNorm (param "FILTER.RESO") (1 / 3)
                        |> String.fromFloat
                        |> String.contains "e"
                        |> Expect.equal False
            , test "a_drag_up_raises_by_pixels_over_the_travel" <|
                \_ ->
                    Expect.within (Expect.Absolute 1.0e-9) 0.7 (Control.dragNorm { px = 150, from = 0.2 } 300 225)
            , test "a_drag_stays_in_range" <|
                \_ ->
                    Expect.equal [ 0, 1 ] [ Control.dragNorm { px = 150, from = 0.2 } 0 1000, Control.dragNorm { px = 150, from = 0.2 } 1000 0 ]
            , test "a_choice_steps_by_pixels_and_stops_at_its_ends" <|
                \_ ->
                    Expect.equal [ 5, 0, 31, 4 ]
                        [ Control.dragIndex { n = 32, from = 4 } 100 94
                        , Control.dragIndex { n = 32, from = 4 } 100 200
                        , Control.dragIndex { n = 32, from = 4 } 100 -900
                        , Control.dragIndex { n = 32, from = 4 } 100 100
                        ]
            , test "a_wheel_notch_moves_at_least_one_unit_step" <|
                \_ ->
                    Expect.all
                        [ \_ -> Expect.within (Expect.Absolute 1.0e-9) (1 / 7) (Control.wheelNorm (param "OP_A.FDBK"))
                        , \_ -> Expect.within (Expect.Absolute 1.0e-9) (1 / 64) (Control.wheelNorm (param "FILTER.CUTOFF"))
                        ]
                        ()
            ]
        ]
