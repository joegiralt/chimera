module ControlTest exposing (suite)

import Addr exposing (Addr)
import Contract
import Expect
import Fixture
import Html
import Json.Encode as Encode
import Protocol exposing (OptIndex, Scale(..))
import Schema exposing (Param)
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Direction(..), Intent(..), Msg(..), Target(..), Travel(..), Want(..))
import Ui.Tokens as Tokens
import Wire exposing (addr)


param : String -> Param
param =
    Fixture.param


{-| A linear param of kind `k` over `lo`..`hi`, `st` a step.
-}
mk : Protocol.Kind -> Float -> Float -> Float -> Param
mk k lo hi st =
    { addr = addr "TEST.P"
    , label = "P"
    , kind = k
    , scale = Linear
    , min = lo
    , max = hi
    , step = st
    , default = lo
    , flags = { mappable = False, modulation = False }
    , options = []
    }


{-| Where `v` sits on a linear param's travel.
-}
normOf : Param -> Float -> Float
normOf p v =
    (v - p.min) / (p.max - p.min)


suite : Test
suite =
    describe "Ui.Control"
        [ describe "coalescing"
            [ test "many_wants_for_one_control_flush_as_one_the_last" <|
                \_ ->
                    Control.empty
                        |> Control.want (addr "FILTER.CUTOFF") (Number 100)
                        |> Control.want (addr "FILTER.CUTOFF") (Number 200)
                        |> Control.want (addr "FILTER.CUTOFF") (Number 300)
                        |> Control.flush
                        |> Tuple.first
                        |> Expect.equal [ ( addr "FILTER.CUTOFF", Number 300 ) ]
            , test "two_controls_flush_one_each" <|
                \_ ->
                    Control.empty
                        |> Control.want (addr "FILTER.RESO") (Number 0.5)
                        |> Control.want (addr "FILTER.CUTOFF") (Number 300)
                        |> Control.want (addr "FILTER.RESO") (Number 0.6)
                        |> Control.flush
                        |> Tuple.first
                        |> Expect.equal [ ( addr "FILTER.CUTOFF", Number 300 ), ( addr "FILTER.RESO", Number 0.6 ) ]
            , test "a_flush_empties_so_the_next_frame_sends_nothing" <|
                \_ ->
                    Control.empty
                        |> Control.want (addr "FILTER.RESO") (Number 0.5)
                        |> Control.flush
                        |> Tuple.second
                        |> Control.flush
                        |> Tuple.first
                        |> Expect.equal []
            , test "an_option_is_a_want_too" <|
                \_ ->
                    Control.empty
                        |> Control.want (addr "FILTER.KIND") (Option (Contract.opt 1))
                        |> Control.want (addr "FILTER.KIND") (Option (Contract.opt 2))
                        |> Control.flush
                        |> Tuple.first
                        |> Expect.equal [ ( addr "FILTER.KIND", Option (Contract.opt 2) ) ]
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
            , test "a_continuous_param_is_never_snapped" <|
                \_ ->
                    let
                        rate : Param
                        rate =
                            mk Protocol.Cont 0.01 20 0.15

                        level : Param
                        level =
                            mk Protocol.Cont 0 1 0.0078125
                    in
                    Expect.equal [ 0.05, 0.8, 0.01 ]
                        [ Control.atNorm rate (normOf rate 0.05), Control.atNorm level 0.8, Control.atNorm rate 0 ]
            , test "a_stepped_param_lands_on_its_steps_from_zero" <|
                \_ ->
                    let
                        note : Param
                        note =
                            mk Protocol.Stepped 0 1 0.1
                    in
                    Expect.equal [ 0.6, 0, 1, 0.3 ]
                        [ Control.atNorm note 0.55, Control.atNorm note 0.04, Control.atNorm note 0.97, Control.atNorm note 0.33 ]
            , test "a_stepped_param_whose_bottom_is_off_its_steps_keeps_to_its_range" <|
                \_ ->
                    let
                        q : Param
                        q =
                            mk Protocol.Stepped 0.04 1 0.1
                    in
                    -- 0.33 rounds to 0.3 (from zero, not from 0.04); the bottom's
                    -- 0 is out of range: the bottom.
                    Expect.equal [ 0.3, 0.04 ] [ Control.atNorm q (normOf q 0.33), Control.atNorm q 0 ]
            , test "a_stepped_param_below_zero_rounds_away_from_zero" <|
                \_ ->
                    let
                        t : Param
                        t =
                            mk Protocol.Stepped -24 24 1
                    in
                    Expect.equal [ -3, -4, -4, 3 ]
                        [ Control.atNorm t (normOf t -3.4), Control.atNorm t (normOf t -3.6), Control.atNorm t (normOf t -3.5), Control.atNorm t (normOf t 3.4) ]
            , test "a_step_of_0_snaps_nothing" <|
                \_ ->
                    Expect.within (Expect.Absolute 1.0e-9) 0.33 (Control.atNorm (mk Protocol.Stepped 0 1 0) 0.33)
            , test "an_octave_param_keeps_its_place" <|
                \_ ->
                    Expect.within (Expect.Absolute 0.01) 632.46 (Control.atNorm (param "FILTER.CUTOFF") 0.5)
            , test "a_wheel_from_an_off_grid_top_lands_on_the_next_step_not_past_it" <|
                \_ ->
                    let
                        q : Param
                        q =
                            mk Protocol.Stepped 0 10 3
                    in
                    Expect.equal [ 9, 10, 3, 6 ]
                        [ Control.nudged q { from = 1, by = -1, wheel = 0.3 }
                        , Control.nudged q { from = normOf q 9, by = 1, wheel = 0.3 }
                        , Control.nudged q { from = normOf q 6, by = -1, wheel = 0.3 }
                        , Control.nudged q { from = normOf q 4.5, by = 1, wheel = 0.3 }
                        ]
            , test "a_wheel_moves_a_continuous_param_by_its_notch" <|
                \_ ->
                    Expect.within (Expect.Absolute 1.0e-4)
                        (0.5 + 1 / 64)
                        (Control.nudged (mk Protocol.Cont 0 1 0.0078125) { from = 0.5, by = 1, wheel = 1 / 64 })
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
            , test "algos_fdbk_steps_in_tenths_as_the_schema_says" <|
                \_ ->
                    -- `param OP_A.FDBK "OP1" cont lin 0 7 0.1 0 m` (0.4, #1045).
                    [ "OP_A", "OP_B", "OP_C", "OP_D", "OP_E", "OP_F" ]
                        |> List.map (\o -> param (o ++ ".FDBK"))
                        |> List.map (\p -> ( ( p.min, p.max ), p.step ))
                        |> Expect.equal (List.repeat 6 ( ( 0, 7 ), 0.1 ))
            , test "a_wheel_notch_moves_at_least_one_unit_step" <|
                \_ ->
                    Expect.all
                        [ \_ -> Expect.within (Expect.Absolute 1.0e-9) (1 / 7) (Control.wheelNorm (param "OP_A.VEL"))
                        , \_ -> Expect.within (Expect.Absolute 1.0e-6) (1 / 70) ((param "OP_A.FDBK").step / ((param "OP_A.FDBK").max - (param "OP_A.FDBK").min))
                        , \_ -> Expect.within (Expect.Absolute 1.0e-9) (1 / 64) (Control.wheelNorm (param "OP_A.FDBK"))
                        , \_ -> Expect.within (Expect.Absolute 1.0e-9) (1 / 64) (Control.wheelNorm (param "FILTER.CUTOFF"))
                        ]
                        ()
            ]
        , describe "the shared update" updates
        , describe "the shared hands" hands
        ]


cutoff : Addr
cutoff =
    addr "FILTER.CUTOFF"


kind : Addr
kind =
    addr "FILTER.KIND"


opt : Int -> OptIndex
opt i =
    Protocol.optIndex i |> Maybe.withDefault Protocol.firstOption


{-| CUTOFF at 0.5 (wheel steps of 1/64); KIND on its second of four options.
-}
lookup : Addr -> Maybe Target
lookup a =
    if a == cutoff then
        Just (Continuous { norm = 0.5, wheel = 1 / 64 })

    else if a == kind then
        Just (Discrete { options = List.map opt [ 0, 1, 2, 3 ], current = opt 1 })

    else
        Nothing


run : List Msg -> List Intent
run msgs =
    List.foldl
        (\m ( d, out ) ->
            let
                ( d1, i ) =
                    Control.update lookup m d
            in
            ( d1, out ++ i )
        )
        ( Control.idle, [] )
        msgs
        |> Tuple.second


updates : List Test
updates =
    [ test "a_drag_up_raises_the_value_by_the_travel" <|
        \_ ->
            run [ Grab cutoff Knob 300, Moved 225 ]
                |> Expect.equal [ SetNorm cutoff 1 ]
    , test "the_hero_travels_further" <|
        \_ ->
            run [ Grab cutoff Hero 300, Moved 240 ]
                |> Expect.equal [ SetNorm cutoff 0.75 ]
    , test "a_drag_of_a_choice_steps_its_options" <|
        \_ ->
            run [ Grab kind Knob 100, Moved 55 ]
                |> Expect.equal [ SetOption kind (opt 2) ]
    , test "after_release_a_move_does_nothing" <|
        \_ ->
            run [ Grab cutoff Knob 300, Released, Moved 0 ]
                |> Expect.equal []
    , test "a_grab_of_an_unknown_address_drags_nothing" <|
        \_ ->
            run [ Grab (addr "NOPE") Knob 300, Moved 0 ]
                |> Expect.equal []
    , test "a_wheel_notch_steps_a_value_and_a_choice" <|
        \_ ->
            run [ Step cutoff Up, Step kind Down, Step kind Up ]
                |> Expect.equal [ Nudge cutoff { from = 0.5, by = 1, wheel = 1 / 64 }, SetOption kind (opt 0), SetOption kind (opt 2) ]
    , test "a_step_stops_at_the_ends" <|
        \_ ->
            Control.update (\_ -> Just (Discrete { options = [ opt 0, opt 1 ], current = opt 1 })) (Step kind Up) Control.idle
                |> Tuple.second
                |> Expect.equal [ SetOption kind (opt 1) ]
    , test "a_pick_and_a_reset_pass_through" <|
        \_ ->
            run [ Pick kind (opt 3), Reset cutoff ]
                |> Expect.equal [ SetOption kind (opt 3), Restore cutoff ]
    ]


hands : List Test
hands =
    let
        probe : Query.Single Msg
        probe =
            Html.div (Control.grab cutoff Knob) []
                |> Query.fromHtml
    in
    [ test "a_mouse_down_grabs" <|
        \_ -> probe |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 12 ) ])) |> Event.expect (Grab cutoff Knob 12)
    , test "the_wheel_steps_up_when_it_rolls_away" <|
        \_ -> probe |> Event.simulate (Event.custom "wheel" (Encode.object [ ( "deltaY", Encode.float -3 ) ])) |> Event.expect (Step cutoff Up)
    , test "an_arrow_down_steps_down" <|
        \_ -> probe |> Event.simulate (Event.custom "keydown" (Encode.object [ ( "key", Encode.string "ArrowDown" ) ])) |> Event.expect (Step cutoff Down)
    , test "a_double_click_resets" <|
        \_ -> probe |> Event.simulate Event.doubleClick |> Event.expect (Reset cutoff)
    , test "a_readout_is_sized_in_ch_with_tabular_numerals" <|
        \_ ->
            Control.readout Tokens.readoutChars (Control.reading 0.5 "1.20 kHz")
                |> Query.fromHtml
                |> Query.has [ Sel.style "width" "8ch", Sel.class "v", Sel.text "1.20 kHz" ]
    , test "a_chars_count_is_never_less_than_one" <|
        \_ -> List.map (Tokens.chars >> Maybe.map Tokens.charsCount) [ 0, -2, 1, 9 ] |> Expect.equal [ Nothing, Nothing, Just 1, Just 9 ]
    , test "bipolar_follows_from_the_range_alone" <|
        \_ ->
            List.map Control.polarity [ { min = -24, max = 24 }, { min = 0, max = 127 }, { min = -1, max = 0 }, { min = 20, max = 20000 } ]
                |> List.map Control.isBipolar
                |> Expect.equal [ True, False, False, False ]
    , test "a_value_not_reported_reads_as_missing_never_as_nothing" <|
        \_ -> ( Control.norm Control.missing, Control.text Control.missing ) |> Expect.equal ( Nothing, "--" )
    ]
