module ModelTest exposing (suite)

import Dict
import Expect
import Fixture
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
        |> feed [ "ed hello 3 0.3.0-dev part 1 ALGO", "OK" ]
        |> tick 33
        |> Tuple.first
        |> feed
            [ "page 0 ALG of 1"
            , "panel ALG -"
            , "param FILTER.CUTOFF \"CUTOFF\" cont oct 20 20000 1 1000"
            , "param ENV1.HOLD \"HOLD\" choice lin 0 2 1 0"
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


{-| Option `n` as a set's value; a negative `n` is no option at all.
-}
idx : Int -> Value
idx n =
    case Protocol.optIndex n of
        Just i ->
            Index i

        Nothing ->
            Name "no option"


suite : Test
suite =
    describe "Model"
        [ test "a_choice_pick_uses_the_active_variants_names" <|
            \_ ->
                let
                    pick : List ( String, String ) -> String -> Int -> List Command
                    pick v a i =
                        { following | schema = Fixture.schema, variants = Dict.fromList v }
                            |> Model.step (Pick (Addr a) i)
                            |> Tuple.second
                in
                Expect.all
                    [ \_ -> Expect.equal [ Set (Addr "FILTER.MODE") (idx 1) ] (pick [ ( "FILTER", "LADDER" ) ] "FILTER.MODE" 1)
                    , \_ -> Expect.equal [ Set (Addr "ENV1.FORM") (idx 0) ] (pick [ ( "ENV1", "B-LFO" ) ] "ENV1.FORM" 0)
                    ]
                    ()
        , test "every_choice_under_every_variant_sends_an_option_that_variant_lists" <|
            \_ ->
                let
                    bad : List ( String, String, Int )
                    bad =
                        Schema.pages Fixture.schema
                            |> List.concatMap .panels
                            |> List.filter (\pn -> pn.variant /= "-")
                            |> List.concatMap
                                (\pn ->
                                    pn.params
                                        |> List.filter (\p -> not (List.isEmpty p.options))
                                        |> List.concatMap
                                            (\p ->
                                                p.options
                                                    |> List.filterMap
                                                        (\( i, name ) ->
                                                            let
                                                                sent : List Command
                                                                sent =
                                                                    { following | schema = Fixture.schema, variants = Dict.fromList [ ( pn.name, pn.variant ) ] }
                                                                        |> Model.step (Pick p.addr i)
                                                                        |> Tuple.second
                                                            in
                                                            if name /= "" && sent == [ Set p.addr (idx i) ] then
                                                                Nothing

                                                            else
                                                                Just ( pn.name, pn.variant, i )
                                                        )
                                            )
                                )
                in
                Expect.equal [] bad
        , test "a_val_the_schema_does_not_list_is_dropped" <|
            \_ ->
                Expect.equal Nothing
                    (following |> feed [ "val OUT.PAN 0.5 0 \"C\"" ] |> Model.shown (Addr "OUT.PAN"))
        , test "an_old_unit_refusing_hello_says_update_the_unit" <|
            \_ ->
                Model.init
                    |> Model.step (LinkChanged Open)
                    |> Tuple.first
                    |> tick 0
                    |> Tuple.first
                    |> feed [ "ERR unknown command ed, try help" ]
                    |> Model.notice
                    |> Expect.equal (Just "UPDATE THE UNIT")
        , test "the_status_names_the_link_part_and_engine_once_following" <|
            \_ ->
                let
                    sim : Model.Model
                    sim =
                        following |> Model.update ConnectSocket |> Tuple.first
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "SIMULATOR · PART 1 · ALGO") (Model.status sim)
                    , \_ -> Expect.equal (Just "UNIT · PART 1 · ALGO") (Model.status (Model.update ConnectSerial following |> Tuple.first))
                    , \_ -> Expect.equal Nothing (Model.status Model.init)
                    ]
                    ()
        , test "connect_runs_hello_describe_then_since_0" <|
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

                    ( _, c ) =
                        m
                            |> feed [ "ed hello 3 b part 1 ALGO", "OK" ]
                            |> tick 33
                            |> Tuple.first
                            |> feed [ "page 0 A of 2", "panel A -", "OK" ]
                            |> tick 66
                in
                Expect.equal [ Describe Algo 1 ] c
        , test "shows_only_the_units_value" <|
            \_ ->
                let
                    m : Model.Model
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
                    m : Model.Model
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
                    m : Model.Model
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
                    m : Model.Model
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
                    m : Model.Model
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
        , test "a_hello_timeout_halts_and_a_late_ok_never_reaches_following" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        Model.init
                            |> Model.step (LinkChanged Open)
                            |> Tuple.first
                            |> tick 0
                            |> Tuple.first
                            |> tick 1000
                            |> Tuple.first
                            |> feed [ "ed hello 3 b part 1 ALGO", "OK" ]
                in
                Expect.all
                    [ \x -> Expect.equal (Just "THE UNIT STOPPED ANSWERING") (Model.notice x)
                    , \x -> Expect.equal [] (tick 1033 x |> Tuple.second)
                    , \x -> Expect.equal [] (tick 5000 x |> Tuple.second)
                    ]
                    m
        , test "a_describe_timeout_halts_the_same_way" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        Model.init
                            |> Model.step (LinkChanged Open)
                            |> Tuple.first
                            |> tick 0
                            |> Tuple.first
                            |> feed [ "ed hello 3 b part 1 ALGO", "OK" ]
                            |> tick 33
                            |> Tuple.first
                            |> feed [ "page 0 A of 2", "panel A -" ]
                            |> tick 1033
                            |> Tuple.first
                            |> feed [ "OK" ]
                in
                Expect.all
                    [ \x -> Expect.equal (Just "THE UNIT STOPPED ANSWERING") (Model.notice x)
                    , \x -> Expect.equal [] (tick 1066 x |> Tuple.second)
                    ]
                    m
        , test "a_since_timeout_re_asks_once_and_no_third_goes_out_until_the_re_asks_answer" <|
            \_ ->
                let
                    ( m1, c1 ) =
                        following |> tick 99

                    ( m2, c2 ) =
                        m1 |> tick 1099

                    -- the late answer to the first ask
                    ( m3, c3 ) =
                        m2 |> feed [ "OK 4" ] |> tick 1132

                    -- the re-ask's answer
                    ( _, c4 ) =
                        m3 |> feed [ "OK 5" ] |> tick 1165
                in
                Expect.equal [ [ Since Nothing ], [ Since Nothing ], [], [ Since (gen 5) ] ] [ c1, c2, c3, c4 ]
        , test "following_never_halts_a_lost_answer_is_asked_for_again" <|
            \_ ->
                let
                    ( m1, _ ) =
                        following |> tick 99

                    ( m2, c2 ) =
                        m1 |> tick 1099

                    ( m3, c3 ) =
                        m2 |> tick 2099

                    m4 : Model.Model
                    m4 =
                        m3 |> feed [ cutoffLine "777", "OK 7" ]

                    ( _, c4 ) =
                        m4 |> tick 2132
                in
                Expect.all
                    [ \_ -> Expect.equal [ [ Since Nothing ], [ Since Nothing ] ] [ c2, c3 ]
                    , \_ -> Expect.equal Nothing (Model.notice m3)
                    , \_ -> Expect.equal (Just "777 Hz") (shown m4)
                    , \_ -> Expect.equal (gen 7) m4.gen
                    , \_ -> Expect.equal [ Since (gen 7) ] c4
                    ]
                    ()
        , test "only_the_re_asks_ok_arrives_and_following_goes_on" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> tick 99
                            |> Tuple.first
                            |> tick 1099
                            |> Tuple.first
                            |> feed [ "OK 5" ]
                in
                Expect.all
                    [ \x -> Expect.equal [] (tick 1500 x |> Tuple.second)
                    , \x -> Expect.equal [ Since (gen 5) ] (tick 2100 x |> Tuple.second)
                    , \x -> Expect.equal Nothing (Model.notice (tick 2100 x |> Tuple.first))
                    ]
                    m
        , test "a_refused_first_since_is_settled_the_same_way" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> tick 99
                            |> Tuple.first
                            |> feed [ "ERR busy" ]
                            |> tick 1099
                            |> Tuple.first
                            |> feed [ "OK 5" ]
                in
                Expect.equal [ Since (gen 5) ] (tick 2100 m |> Tuple.second)
        , test "a_lost_link_clears_the_values" <|
            \_ ->
                Expect.equal Nothing
                    (following |> feed [ cutoffLine "1000", "OK 1" ] |> Model.step (LinkChanged Lost) |> Tuple.first |> shown)
        , test "an_echo_of_the_same_value_clears_pending" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed [ cutoffLine "1000", "OK 1" ]
                            |> Model.step (Edit cutoff (Num 1000))
                            |> Tuple.first
                in
                Expect.equal ( True, False ) ( Model.isPending cutoff m, Model.isPending cutoff (feed [ cutoffLine "1000" ] m) )
        , test "part_change_follows_the_box" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed [ cutoffLine "1000", "OK 1" ]
                            |> Model.step (Edit (Addr "ENV1.HOLD") (Num 1))
                            |> Tuple.first
                            |> feed [ "part 3 ALGO", cutoffLine "300", "OK 2" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Unit.partFromInt 3) m.part
                    , \_ -> Expect.equal False (Model.isPending (Addr "ENV1.HOLD") m)
                    , \_ -> Expect.equal (Just "300 Hz") (shown m)
                    , \_ -> Expect.equal Nothing (Model.notice m)
                    ]
                    ()
        , test "a_new_part_drops_the_old_parts_values" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed [ cutoffLine "1000", "panel FILTER CLEAN", "OK 1" ]
                            |> Model.step (Edit cutoff (Num 5))
                            |> Tuple.first
                            |> feed [ "part 2 VA", "OK 2" ]
                in
                Expect.equal ( Nothing, Dict.empty, False ) ( Model.shown cutoff m, m.variants, Model.isPending cutoff m )
        , test "non_algo_part_shows_message" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following |> feed [ "part 2 VA", "OK 1" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "PART 2 PLAYS VA. THE EDITOR EDITS ALGO FOR NOW.") (Model.notice m)
                    , \_ -> Expect.equal (Just Va) m.engine
                    ]
                    ()
        , test "no_edit_on_a_part_that_is_not_algo" <|
            \_ ->
                Expect.equal []
                    (following |> feed [ "part 2 VA", "OK 1" ] |> Model.step (Edit cutoff (Num 1)) |> Tuple.second)
        , test "a_choice_is_sent_by_its_number" <|
            \_ ->
                Expect.equal [ Set (Addr "ENV1.HOLD") (idx 0) ]
                    (following |> Model.step (Pick (Addr "ENV1.HOLD") 0) |> Tuple.second)
        , test "a_choice_name_with_a_space_is_sent_by_its_number_and_pends" <|
            \_ ->
                -- GATE EXT is two words: its number is one (#996).
                let
                    ( m, c ) =
                        following |> Model.step (Pick (Addr "ENV1.HOLD") 2)
                in
                Expect.equal ( [ Set (Addr "ENV1.HOLD") (idx 2) ], True ) ( c, Model.isPending (Addr "ENV1.HOLD") m )
        , test "an_option_the_schema_lacks_sends_nothing" <|
            \_ ->
                Expect.equal [] (following |> Model.step (Pick (Addr "ENV1.HOLD") 1) |> Tuple.second)
        , test "an_engine_the_editor_does_not_know_says_update_the_editor" <|
            \_ ->
                Expect.equal (Just "UPDATE THE EDITOR") (following |> feed [ "part 2 GRANULAR", "OK 1" ] |> Model.notice)
        , test "every_engine_is_named_as_the_unit_names_it" <|
            \_ ->
                Expect.equal (List.map Just [ Algo, Modal, Va, Tape, Drum ])
                    (List.map Unit.engineFromName [ "ALGO", "MODAL", "VA", "TAPE", "DRUM" ])
        , test "a_since_too_large_says_so_and_stops_asking" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following |> feed [ "ERR ed since too large" ]
                in
                Expect.all
                    [ \x -> Expect.equal (Just "THE UNIT'S ANSWER IS TOO LARGE FOR ONE READ") (Model.notice x)
                    , \x -> Expect.equal [] (tick 33 x |> Tuple.second)
                    , \x -> Expect.equal [] (tick 5000 x |> Tuple.second)
                    ]
                    m
        , test "tapes_source_text_is_kept_while_following" <|
            \_ ->
                Expect.equal (Just "KICK 909") (following |> feed [ "source \"KICK 909\"", "OK 1" ] |> .source)
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
                    (feed [ "ed hello 4 x part 1 ALGO", "OK" ] m)
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
                    (feed [ "ed hello 2 x part 1 ALGO", "OK" ] m)
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
        , test "a_part_button_asks_the_box_and_waits_for_it" <|
            \_ ->
                case Unit.partFromInt 2 of
                    Just p2 ->
                        let
                            ( m, cs ) =
                                following |> Model.step (FollowPart p2)
                        in
                        Expect.all
                            [ \_ -> Expect.equal [ Follow p2 ] cs
                            , -- Never guessed: the mark moves when `since` says so.
                              \_ -> Expect.equal (Unit.partFromInt 1) m.part
                            , \_ -> Expect.equal (Just p2) (feed [ "part 2 VA", "OK 2" ] m).part
                            ]
                            ()

                    Nothing ->
                        Expect.fail "no Part 2"
        , test "a_part_button_before_following_sends_nothing" <|
            \_ ->
                case Unit.partFromInt 2 of
                    Just p2 ->
                        Model.init
                            |> Model.step (LinkChanged Open)
                            |> Tuple.first
                            |> Model.step (FollowPart p2)
                            |> Tuple.second
                            |> Expect.equal []

                    Nothing ->
                        Expect.fail "no Part 2"
        ]
