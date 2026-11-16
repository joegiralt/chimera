module ModelTest exposing (suite)

import Dict
import Expect
import Model exposing (Msg(..))
import Protocol exposing (Addr(..), Command(..), Value(..))
import Schema
import Test exposing (Test, describe, test)
import Time
import Transport exposing (Link(..))
import Unit exposing (Engine(..))


at : Int -> Time.Posix
at =
    Time.millisToPosix


feed : List String -> Model.Model -> Model.Model
feed ls m =
    List.foldl (\l -> Model.step (Line l) >> Tuple.first) m ls


tick : Int -> Model.Model -> ( Model.Model, List Command )
tick ms =
    Model.step (Tick (at ms))


gen : Int -> Maybe Unit.Gen
gen =
    Unit.genFromInt


cutoff : Addr
cutoff =
    Addr "FILTER.CUTOFF"


{-| A connected editor that has said hello and read a one-page schema: following, nothing asked yet.
-}
following : Model.Model
following =
    Model.init
        |> Model.step (LinkChanged Open)
        |> Tuple.first
        |> tick 0
        |> Tuple.first
        |> feed [ "ed hello 2 0.3.0-dev part 1 ALGO", "OK" ]
        |> tick 33
        |> Tuple.first
        |> feed
            [ "page 0 ALG of 1"
            , "panel ALG -"
            , "param FILTER.CUTOFF \"CUTOFF\" cont 20 20000 1 1000"
            , "param ENV1.HOLD \"HOLD\" choice 0 2 1 0"
            , "opt ENV1.HOLD 0 \"OFF\""
            , "opt ENV1.HOLD 2 \"GATE EXT\""
            , "OK"
            ]


cutoffLine : String -> String
cutoffLine v =
    "val FILTER.CUTOFF 0.5 " ++ v ++ " \"" ++ v ++ " Hz\""


shown : Model.Model -> Maybe String
shown m =
    Model.shown cutoff m |> Maybe.map .text


suite : Test
suite =
    describe "Model"
        [ test "connect_runs_hello_describe_then_since_0" <|
            \_ ->
                let
                    ( m, c ) =
                        following |> tick 99
                in
                Expect.all
                    [ \_ -> Expect.equal [ Since Nothing ] c
                    , \_ -> Expect.equal [ Hello ] (Model.init |> Model.step (LinkChanged Open) |> Tuple.first |> tick 0 |> Tuple.second)
                    , \_ -> Expect.equal 1 (List.length (Schema.pages m.schema))
                    , \_ -> Expect.equal (Just "CUTOFF") (Schema.param cutoff m.schema |> Maybe.map .label)
                    ]
                    ()
        , test "describe_walks_every_page_before_following" <|
            \_ ->
                let
                    ( m, _ ) =
                        Model.init |> Model.step (LinkChanged Open) |> Tuple.first |> tick 0

                    ( m2, c ) =
                        m
                            |> feed [ "ed hello 2 b part 1 ALGO", "OK" ]
                            |> tick 33
                            |> Tuple.first
                            |> feed [ "page 0 A of 2", "panel A -", "OK" ]
                            |> tick 66
                in
                Expect.equal [ Describe "ALGO" 1 ] c
        , test "shows_only_the_units_value" <|
            \_ ->
                let
                    m =
                        following
                            |> feed [ cutoffLine "1000", "OK 1" ]
                            |> Model.step (Edit cutoff (Num 5000))
                            |> Tuple.first
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "1000 Hz") (shown m)
                    , \_ -> Expect.equal True (Model.isPending cutoff m)
                    , \_ -> Expect.equal (Just "1000 Hz") (m |> feed [ "OK queued" ] |> shown)
                    , \_ -> Expect.equal (Just "1000 Hz") (m |> feed [ "ERR busy" ] |> shown)
                    , \_ -> Expect.equal (Just "5000 Hz") (m |> feed [ cutoffLine "5000" ] |> shown)
                    ]
                    ()
        , test "an_edit_sends_set" <|
            \_ ->
                Expect.equal [ Set cutoff (Num 5000) ]
                    (following |> Model.step (Edit cutoff (Num 5000)) |> Tuple.second)
        , test "pending_clears_on_report" <|
            \_ ->
                let
                    m =
                        following |> Model.step (Edit cutoff (Num 5000)) |> Tuple.first
                in
                Expect.all
                    [ \_ -> Expect.equal True (Model.isPending cutoff m)
                    , \_ -> Expect.equal False (Model.isPending cutoff (feed [ cutoffLine "5000" ] m))
                    , \_ -> Expect.equal True (Model.isPending cutoff (feed [ "val ENV1.HOLD 0 0 \"OFF\"" ] m))
                    ]
                    ()
        , test "pending_clears_after_250ms_with_no_report" <|
            \_ ->
                let
                    m =
                        following
                            |> Model.step (Tick (at 1000))
                            |> Tuple.first
                            |> Model.step (Edit cutoff (Num 5000))
                            |> Tuple.first
                in
                Expect.all
                    [ \_ -> Expect.equal True (Model.isPending cutoff (m |> Model.step (Tick (at 1249)) |> Tuple.first))
                    , \_ -> Expect.equal False (Model.isPending cutoff (m |> Model.step (Tick (at 1250)) |> Tuple.first))
                    ]
                    ()
        , test "a_repeated_edit_restarts_the_250ms" <|
            \_ ->
                let
                    m =
                        following
                            |> Model.step (Tick (at 1000))
                            |> Tuple.first
                            |> Model.step (Edit cutoff (Num 5000))
                            |> Tuple.first
                            |> Model.step (Tick (at 1200))
                            |> Tuple.first
                            |> Model.step (Edit cutoff (Num 6000))
                            |> Tuple.first
                            |> Model.step (Tick (at 1400))
                            |> Tuple.first
                in
                Expect.equal True (Model.isPending cutoff m)
        , test "since_answer_replaces_values_and_gen" <|
            \_ ->
                let
                    m =
                        following
                            |> feed [ "part 1 ALGO", "panel FILTER CLEAN", cutoffLine "1000", "OK 5" ]
                            |> feed [ cutoffLine "2000", "panel FILTER 2040", "OK 6" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "2000 Hz") (shown m)
                    , \_ -> Expect.equal (gen 6) m.gen
                    , \_ -> Expect.equal (Just "2040") (Dict.get "FILTER" m.variants)
                    ]
                    ()
        , test "a_stale_answer_number_is_never_zero" <|
            \_ ->
                Expect.equal Nothing (following |> feed [ "OK 0" ]).gen
        , test "one_since_in_flight" <|
            \_ ->
                let
                    ( m1, c1 ) =
                        following |> tick 99

                    ( m2, c2 ) =
                        m1 |> tick 132

                    ( m3, c3 ) =
                        m2 |> feed [ "OK 4" ] |> tick 165

                    ( _, c4 ) =
                        m3 |> tick 198
                in
                Expect.equal [ [ Since Nothing ], [], [ Since (gen 4) ], [] ] [ c1, c2, c3, c4 ]
        , test "a_lost_answer_frees_the_slot_after_a_second_and_asks_again_with_the_old_number" <|
            \_ ->
                let
                    m =
                        following
                            |> tick 99
                            |> Tuple.first
                            |> feed [ "OK 4" ]
                            |> tick 132
                            |> Tuple.first
                in
                Expect.equal
                    [ [], [ Since (gen 4) ] ]
                    [ m |> tick 1131 |> Tuple.second, m |> tick 1132 |> Tuple.second ]
        , test "part_change_follows_the_box" <|
            \_ ->
                let
                    m =
                        following
                            |> feed [ cutoffLine "1000", "OK 1" ]
                            |> feed [ "part 3 ALGO", cutoffLine "300", "OK 2" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Unit.partFromInt 3) m.part
                    , \_ -> Expect.equal (Just "300 Hz") (shown m)
                    , \_ -> Expect.equal Nothing (Model.notice m)
                    ]
                    ()
        , test "a_new_part_drops_the_old_parts_values" <|
            \_ ->
                let
                    m =
                        following
                            |> feed [ cutoffLine "1000", "panel FILTER CLEAN", "OK 1" ]
                            |> feed [ "part 2 VA", "OK 2" ]
                in
                Expect.equal ( Nothing, Dict.empty ) ( Model.shown cutoff m, m.variants )
        , test "non_algo_part_shows_message" <|
            \_ ->
                let
                    m =
                        following |> feed [ "part 2 VA", "OK 1" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "PART 2 PLAYS VA. THE EDITOR EDITS ALGO FOR NOW.") (Model.notice m)
                    , \_ -> Expect.equal (Other "VA") (Maybe.withDefault Algo m.engine)
                    ]
                    ()
        , test "no_edit_on_a_part_that_is_not_algo" <|
            \_ ->
                Expect.equal []
                    (following |> feed [ "part 2 VA", "OK 1" ] |> Model.step (Edit cutoff (Num 1)) |> Tuple.second)
        , test "a_choice_is_sent_by_its_name" <|
            \_ ->
                Expect.equal [ Set (Addr "ENV1.HOLD") (Name "OFF") ]
                    (following |> Model.step (Pick (Addr "ENV1.HOLD") 0) |> Tuple.second)
        , test "a_choice_name_with_a_space_cannot_be_sent_so_nothing_is_sent_or_pending" <|
            \_ ->
                -- The unit matches a choice by one word; GATE EXT has two (a firmware gap).
                let
                    ( m, c ) =
                        following |> Model.step (Pick (Addr "ENV1.HOLD") 2)
                in
                Expect.equal ( [], False ) ( c, Model.isPending (Addr "ENV1.HOLD") m )
        , test "hello_with_the_same_protocol_runs" <|
            \_ ->
                Expect.equal Nothing (Model.notice following)
        , test "unit_newer_than_the_editor_says_update_the_editor_and_stops" <|
            \_ ->
                let
                    ( m, _ ) =
                        Model.init |> Model.step (LinkChanged Open) |> Tuple.first |> tick 0
                in
                Expect.all
                    [ \x -> Expect.equal (Just "UPDATE THE EDITOR") (Model.notice x)
                    , \x -> Expect.equal [] (tick 33 x |> Tuple.second)
                    , \x -> Expect.equal [] (tick 5000 x |> Tuple.second)
                    ]
                    (feed [ "ed hello 3 x part 1 ALGO", "OK" ] m)
        , test "unit_older_than_the_editor_says_update_the_unit_and_stops" <|
            \_ ->
                let
                    ( m, _ ) =
                        Model.init |> Model.step (LinkChanged Open) |> Tuple.first |> tick 0
                in
                Expect.all
                    [ \x -> Expect.equal (Just "UPDATE THE UNIT") (Model.notice x)
                    , \x -> Expect.equal [] (tick 33 x |> Tuple.second)
                    ]
                    (feed [ "ed hello 1 x part 1 ALGO", "OK" ] m)
        , test "a_refusal_while_connecting_stops_with_its_text" <|
            \_ ->
                let
                    ( m, _ ) =
                        Model.init |> Model.step (LinkChanged Open) |> Tuple.first |> tick 0
                in
                Expect.equal (Just "THE UNIT REFUSED: ed takes hello") (Model.notice (feed [ "ERR ed takes hello" ] m))
        , test "a_lost_link_shows_its_text_and_stops_asking" <|
            \_ ->
                let
                    ( m, _ ) =
                        following |> Model.step (LinkChanged Lost)
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "THE UNIT WAS DISCONNECTED") (Model.notice m)
                    , \_ -> Expect.equal [] (tick 5000 m |> Tuple.second)
                    ]
                    ()
        , test "reconnecting_starts_again_from_hello" <|
            \_ ->
                let
                    ( m, _ ) =
                        following |> feed [ cutoffLine "1000", "OK 9" ] |> Model.step (LinkChanged Lost)
                in
                Expect.equal ( [ Hello ], Nothing )
                    (m
                        |> Model.step (LinkChanged Open)
                        |> Tuple.first
                        |> (\x -> ( tick 6000 x |> Tuple.second, x.gen ))
                    )
        ]
