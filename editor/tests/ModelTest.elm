module ModelTest exposing (suite)

import Addr exposing (Addr)
import Dict
import Expect
import Fixture
import Model exposing (Msg(..))
import Protocol exposing (Command(..), OptIndex, Value(..))
import Schema
import Test exposing (Test, describe, test)
import Time
import Transport exposing (Link(..))
import Unit exposing (Engine(..))
import Wire exposing (addr, page)


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
    addr "FILTER.CUTOFF"


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



-- A pick of option `n`, as a hand's reaches the model.


pick : Addr -> Int -> Model.Model -> List Command
pick a n m =
    Protocol.optIndex n
        |> Maybe.map (\i -> Model.step (Pick a i) m |> Tuple.second)
        |> Maybe.withDefault []


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
                    under : List ( String, String ) -> String -> Int -> List Command
                    under v a i =
                        pick (addr a) i { following | schema = Fixture.schema, variants = Dict.fromList v }
                in
                Expect.all
                    [ \_ -> Expect.equal [ Set (addr "FILTER.MODE") (idx 1) ] (under [ ( "FILTER", "LADDER" ) ] "FILTER.MODE" 1)
                    , \_ -> Expect.equal [ Set (addr "ENV1.FORM") (idx 0) ] (under [ ( "ENV1", "B-LFO" ) ] "ENV1.FORM" 0)
                    ]
                    ()
        , test "every_choice_under_every_variant_sends_an_option_that_variant_lists" <|
            \_ ->
                let
                    bad : List ( String, String, OptIndex )
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
                                                            if name /= "" && sent == [ Set p.addr (Index i) ] then
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
                    (following |> feed [ "val OUT.PAN 0.5 0 \"C\"" ] |> Model.shown (addr "OUT.PAN"))
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
                        following |> Model.update (Connect Model.ViaSimulator) |> Tuple.first
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "SIMULATOR · PART 1 · ALGO") (Model.status sim)
                    , \_ -> Expect.equal (Just "UNIT · PART 1 · ALGO") (Model.status (Model.update (Connect Model.ViaUnit) following |> Tuple.first))
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
                Expect.equal [ Describe Algo (page 1) ] c
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
                    , \_ -> Expect.equal (Just "1000 Hz") (m |> feed [ "OK queued" ] |> shown)
                    , \_ -> Expect.equal (Just "1000 Hz") (m |> feed [ "ERR busy" ] |> shown)
                    , \_ -> Expect.equal (Just "5000 Hz") (m |> feed [ cutoffLine "5000" ] |> shown)
                    ]
                    ()
        , test "an_edit_sends_set" <|
            \_ ->
                Expect.equal [ Set cutoff (Num 5000) ]
                    (following |> Model.step (Edit cutoff (Num 5000)) |> Tuple.second)
        , test "a_refusal_from_since_shows_the_units_reason_until_its_next_value" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed [ cutoffLine "1000", "OK 1" ]
                            |> Model.step (Edit cutoff (Num 5000))
                            |> Tuple.first
                            |> feed [ "OK queued", "refused FILTER.CUTOFF \"NOT ON THIS MACHINE\"", "OK 2" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "NOT ON THIS MACHINE") (Model.refusal cutoff m)
                    , \_ -> Expect.equal (Just "1000 Hz") (shown m)
                    , \_ -> Expect.equal Nothing (Model.refusal cutoff (feed [ cutoffLine "1200", "OK 3" ] m))
                    , \_ -> Expect.equal (Just "NOT ON THIS MACHINE") (Model.refusal cutoff (feed [ "val ENV1.HOLD 0 0 \"OFF\"", "OK 3" ] m))
                    ]
                    ()
        , test "an_err_to_a_set_shows_the_same_way" <|
            \_ ->
                following
                    |> feed [ cutoffLine "1000", "OK 1" ]
                    |> Model.step (Edit cutoff (Num 5000))
                    |> Tuple.first
                    |> feed [ "ERR ed set FILTER.CUTOFF takes a number" ]
                    |> Expect.all
                        [ Model.refusal cutoff >> Expect.equal (Just "takes a number")
                        , shown >> Expect.equal (Just "1000 Hz")
                        , Model.notice >> Expect.equal Nothing
                        ]
        , test "no_value_is_guessed_an_edit_shows_nothing_until_the_unit_reports" <|
            \_ ->
                following
                    |> feed [ cutoffLine "1000", "OK 1" ]
                    |> Model.step (Edit cutoff (Num 5000))
                    |> Tuple.first
                    |> Model.step (Tick (at 5000))
                    |> Tuple.first
                    |> shown
                    |> Expect.equal (Just "1000 Hz")
        , test "a_new_part_drops_its_refusals" <|
            \_ ->
                following
                    |> feed [ "refused FILTER.CUTOFF \"NO\"", "OK 1", "part 2 ALGO", "OK 2" ]
                    |> Model.refusal cutoff
                    |> Expect.equal Nothing
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
        , test "part_change_follows_the_box" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed [ cutoffLine "1000", "OK 1" ]
                            |> Model.step (Edit (addr "ENV1.HOLD") (Num 1))
                            |> Tuple.first
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
                    m : Model.Model
                    m =
                        following
                            |> feed [ cutoffLine "1000", "panel FILTER CLEAN", "OK 1" ]
                            |> Model.step (Edit cutoff (Num 5))
                            |> Tuple.first
                            |> feed [ "part 2 VA", "OK 2" ]
                in
                Expect.equal ( Nothing, Dict.empty ) ( Model.shown cutoff m, m.variants )
        , test "a_part_on_another_engine_is_described_afresh" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following |> feed [ cutoffLine "1000", "OK 1", "part 2 VA", "OK 2" ]
                in
                Expect.all
                    [ \_ -> Expect.equal Nothing (Model.notice m)
                    , \_ -> Expect.equal (Just Va) m.engine
                    , \_ -> Expect.equal [ Describe Va (page 0) ] (tick 99 m |> Tuple.second)
                    , \_ -> Expect.equal Nothing (Model.status m)
                    ]
                    ()
        , test "the_old_page_stays_up_while_the_new_engine_is_read" <|
            \_ ->
                following
                    |> feed [ cutoffLine "1000", "OK 1", "part 2 VA", "OK 2" ]
                    |> Expect.all
                        [ .described >> Expect.equal (Just Algo)
                        , .schema >> Schema.pages >> List.length >> Expect.equal 1
                        ]
        , test "an_edit_waits_for_the_engine_the_box_is_on_to_be_read" <|
            -- During an engine change FILTER.CUTOFF is ALGO's no longer:
            -- nothing goes out against the old schema.
            \_ ->
                let
                    switched : Model.Model
                    switched =
                        following |> feed [ cutoffLine "1000", "OK 1", "part 2 VA" ]
                in
                Expect.all
                    [ \_ -> Expect.equal [] (Model.step (Edit cutoff (Num 5000)) switched |> Tuple.second)
                    , \_ -> Expect.equal [] (Model.step (Edit cutoff (Num 5000)) (feed [ "OK 2" ] switched) |> Tuple.second)
                    , \_ -> Expect.equal [] (pick (addr "ENV1.HOLD") 2 switched)
                    , \_ -> Expect.equal False (Model.takesEdits switched)
                    ]
                    ()
        , test "a_new_part_is_a_new_seat" <|
            \_ ->
                Expect.notEqual (Model.seat following) (Model.seat (feed [ "part 3 ALGO" ] following))
        , test "hello_says_which_engine_to_describe" <|
            \_ ->
                Model.init
                    |> Model.step (LinkChanged Open)
                    |> Tuple.first
                    |> tick 0
                    |> Tuple.first
                    |> feed [ "ed hello 3 b part 4 MODAL", "OK" ]
                    |> tick 33
                    |> Tuple.second
                    |> Expect.equal [ Describe Modal (page 0) ]
        , test "the_same_engine_on_another_part_keeps_its_schema" <|
            \_ ->
                following
                    |> feed [ "OK 1", "part 3 ALGO", "OK 2" ]
                    |> tick 99
                    |> Tuple.second
                    |> Expect.equal [ Since (gen 2) ]
        , test "a_redescribed_engine_follows_from_nothing_and_takes_edits" <|
            \_ ->
                let
                    described : Model.Model
                    described =
                        following
                            |> feed [ "OK 1", "part 2 VA", "OK 2" ]
                            |> tick 99
                            |> Tuple.first
                            |> feed [ "page 0 VA of 1", "panel VA -", "param FILTER.CUTOFF \"CUTOFF\" cont oct 20 20000 1 1000", "OK" ]

                    ( _, asked ) =
                        tick 132 described
                in
                Expect.all
                    [ \_ -> Expect.equal [ Since Nothing ] asked
                    , \_ -> Expect.equal [ Set cutoff (Num 1000) ] (Model.step (Edit cutoff (Num 1000)) described |> Tuple.second)
                    ]
                    ()
        , test "a_choice_is_sent_by_its_number" <|
            \_ ->
                Expect.equal [ Set (addr "ENV1.HOLD") (idx 0) ]
                    (following |> pick (addr "ENV1.HOLD") 0)
        , test "a_choice_name_with_a_space_is_sent_by_its_number" <|
            \_ ->
                -- GATE EXT is two words: its number is one (#996).
                let
                    ( _, c ) =
                        Protocol.optIndex 2
                            |> Maybe.map (\i -> following |> Model.step (Pick (addr "ENV1.HOLD") i))
                            |> Maybe.withDefault ( following, [] )
                in
                Expect.equal [ Set (addr "ENV1.HOLD") (idx 2) ] c
        , test "an_option_the_schema_lacks_sends_nothing" <|
            \_ ->
                Expect.equal [] (following |> pick (addr "ENV1.HOLD") 1)
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
