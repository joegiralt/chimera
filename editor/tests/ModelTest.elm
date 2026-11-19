module ModelTest exposing (suite)

import Addr exposing (Addr)
import Dict
import Expect
import Fixture
import Model exposing (Effect(..), Msg(..))
import Protocol exposing (Clock(..), Command(..), Learn(..), MapCommand(..), MapKind(..), OnOff(..), OptIndex, OrbitCommand(..), Playback(..), RowStatus(..), SaveInto(..), SchemaId(..), SlotState(..), Value(..))
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


{-| The lines to the unit among a step's effects.
-}
sends : List Effect -> List Command
sends =
    List.filterMap
        (\e ->
            case e of
                Send c ->
                    Just c

                Recall _ ->
                    Nothing

                Keep _ _ ->
                    Nothing
        )


{-| A step, and the lines it sends.
-}
stepSends : Msg -> Model.Model -> ( Model.Model, List Command )
stepSends msg =
    Model.step msg >> Tuple.mapSecond sends


tick : Int -> Model.Model -> ( Model.Model, List Command )
tick ms =
    stepSends (Tick (at ms))


algoHash : String
algoHash =
    "00000000000000a1"


mixerHash : String
mixerHash =
    "000000000000000b"


mtxHash : String
mtxHash =
    "00000000000000c0"


settingsHash : String
settingsHash =
    "00000000000000d0"


settingsPage : List String
settingsPage =
    [ "page 0 THM of 1"
    , "crumb \"PERSONAL\" \"THEME\""
    , "panel THM -"
    , "param THEME.ACCENT \"ACCENT\" choice lin 0 4 1 0 -"
    , "opt THEME.ACCENT 0 \"TEAL\""
    , "opt THEME.ACCENT 1 \"AMBER\""
    , "param PART3.SNAP \"SNAP\" choice lin 0 2 1 0 -"
    ]


mapHash : String
mapHash =
    "00000000000000e0"


mapPage : List String
mapPage =
    [ "page 0 MAP of 1"
    , "input CC JUMP PICKUP SCALE TWOS-COMP OFFSET SIGNED SIGNED-2 MOMENTARY TOGGLE"
    , "input CC14 JUMP PICKUP SCALE"
    , "kind JUMP \"ABSOLUTE · JUMPS TO THE KNOB\" \"\""
    , "kind TWOS-COMP \"RELATIVE · TWOS COMPLEMENT\" \"+1 = 1  -1 = 127\""
    , "ccs 1 2 74"
    , "msbs 1 2 31"
    , "span 0 255"
    , "rows 16"
    , "maprows 90"
    ]


orbitHash : String
orbitHash =
    "00000000000000f0"


orbitPage : List String
orbitPage =
    [ "page 0 ORB of 1"
    , "panel ORB -"
    , "act \"RING\" RING"
    , "param ORBIT.KEY \"KEY\" choice lin 0 11 1 0 -"
    , "opt ORBIT.KEY 0 \"C\""
    , "opt ORBIT.KEY 1 \"C#\""
    , "act \"SCENE\" SCENE"
    , "param TEMPO.BPM \"BPM\" cont lin 30 300 1 120 -"
    , "param RING1.HITS \"HITS\" cont lin 0 32 1 4 m"
    ]


mtxPage : List String
mtxPage =
    [ "page 0 MTX of 1"
    , "src ENV1 \"ENV1\""
    , "src OP5 \"OP 5\""
    , "col FREQ FILTER.CUTOFF"
    , "amount -127 127"
    , "dests 16"
    ]


{-| The cache had nothing for the schema being recalled.
-}
missed : SchemaId -> String -> Model.Model -> Model.Model
missed s h m =
    m |> Model.step (Recalled { key = key s h, lines = Nothing }) |> Tuple.first


key : SchemaId -> String -> String
key s h =
    "4/" ++ Protocol.schemaToken s ++ "/" ++ h


algoPage : List String
algoPage =
    [ "page 0 ALG of 1"
    , "panel ALG -"
    , "param FILTER.CUTOFF \"CUTOFF\" cont oct 20 20000 1 1000 md"
    , "param ENV1.HOLD \"HOLD\" choice lin 0 2 1 0 -"
    , "opt ENV1.HOLD 0 \"OFF\""
    , "opt ENV1.HOLD 2 \"GATE EXT\""
    ]


mixerPage : List String
mixerPage =
    [ "page 0 REV of 1"
    , "panel REV -"
    , "param PART3.LEVEL \"LEVEL\" cont lin 0 1 0.0078125 0.75 m"
    , "panel REVERB HALL"
    , "param REVERB.SIZE \"SIZE\" cont lin 0 1 0.0078125 0.5 m"
    ]


{-| Said hello: about to ask for the followed engine's schema.
-}
greeted : Model.Model
greeted =
    Model.init
        |> Model.step (LinkChanged Open)
        |> Tuple.first
        |> tick 0
        |> Tuple.first
        |> feed [ "ed hello 4 0.3.0-dev part 1 ALGO", "OK" ]


gen : Int -> Maybe Unit.Gen
gen =
    Unit.genFromInt


cutoff : Addr
cutoff =
    addr "FILTER.CUTOFF"


{-| Every schema read but MAP's last `OK`.
-}
describingMap : Model.Model
describingMap =
    greeted
        |> tick 33
        |> Tuple.first
        |> feed [ "schema ALGO 1 " ++ algoHash, "OK" ]
        |> missed (EngineSchema Algo) algoHash
        |> tick 66
        |> Tuple.first
        |> feed (algoPage ++ [ "OK" ])
        |> tick 99
        |> Tuple.first
        |> feed [ "schema MIXER 1 " ++ mixerHash, "OK" ]
        |> missed MixerSchema mixerHash
        |> tick 132
        |> Tuple.first
        |> feed (mixerPage ++ [ "OK" ])
        |> tick 133
        |> Tuple.first
        |> feed [ "schema MTX-ALGO 1 " ++ mtxHash, "OK" ]
        |> missed (MatrixSchema Algo) mtxHash
        |> tick 134
        |> Tuple.first
        |> feed (mtxPage ++ [ "OK" ])
        |> tick 135
        |> Tuple.first
        |> feed [ "schema SETTINGS 1 " ++ settingsHash, "OK" ]
        |> missed SettingsSchema settingsHash
        |> tick 136
        |> Tuple.first
        |> feed (settingsPage ++ [ "OK" ])
        |> tick 137
        |> Tuple.first
        |> feed [ "schema MAP 1 " ++ mapHash, "OK" ]
        |> missed MapSchema mapHash
        |> tick 138
        |> Tuple.first
        |> feed mapPage


{-| Every schema read but ORBIT's last `OK`.
-}
describingOrbit : Model.Model
describingOrbit =
    describingMap
        |> feed [ "OK" ]
        |> tick 139
        |> Tuple.first
        |> feed [ "schema ORBIT 1 " ++ orbitHash, "OK" ]
        |> missed OrbitSchema orbitHash
        |> tick 140
        |> Tuple.first
        |> feed orbitPage


{-| A connected editor that has said hello and read a one-page schema of
each kind (the engine's, MIXER's, the matrix's, SETTINGS', MAP's and
ORBIT's), none cached: following, nothing asked yet.
-}
following : Model.Model
following =
    feed [ "OK" ] describingOrbit


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
        |> Maybe.map (\i -> stepSends (Pick a i) m |> Tuple.second)
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
                                                                        |> stepSends (Pick p.addr i)
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
                    ( _, c ) =
                        greeted
                            |> tick 33
                            |> Tuple.first
                            |> feed [ "schema ALGO 2 " ++ algoHash, "OK" ]
                            |> missed (EngineSchema Algo) algoHash
                            |> tick 66
                            |> Tuple.first
                            |> feed [ "page 0 A of 2", "panel A -", "OK" ]
                            |> tick 99
                in
                Expect.equal [ Describe (EngineSchema Algo) (page 1) ] c
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
                    (following |> stepSends (Edit cutoff (Num 5000)) |> Tuple.second)
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
        , test "a_whole_answer_says_no_refusal_part_2_never_shows_part_1s" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed [ "part 1 ALGO", cutoffLine "1000", "OK 1", "refused FILTER.CUTOFF \"PART 1'S\"", "OK 2" ]
                            |> feed [ "", "part 2 ALGO", cutoffLine "1000", "refused FILTER.CUTOFF \"PART 1'S\"", "OK 3" ]
                in
                Expect.all
                    [ \_ -> Expect.equal Nothing (Model.refusal cutoff m)
                    , \_ -> Expect.equal (Just "1000 Hz") (shown m)

                    -- A diff after it says Part 2's own.
                    , \_ -> Expect.equal (Just "PART 2'S") (Model.refusal cutoff (feed [ "refused FILTER.CUTOFF \"PART 2'S\"", "OK 4" ] m))
                    ]
                    ()
        , test "since_0_is_whole_its_refusals_stale" <|
            \_ ->
                following
                    |> feed [ "part 1 ALGO", "refused FILTER.CUTOFF \"OLD\"", "OK 1" ]
                    |> Model.refusal cutoff
                    |> Expect.equal Nothing
        , test "a_refusal_of_an_address_not_in_the_schema_is_ignored" <|
            \_ ->
                following
                    |> feed [ "part 1 ALGO", "OK 1", "refused PAD3.CHOKE \"9 GROUPS · KIT HOLDS 8\"", "OK 2" ]
                    |> Model.refusal (addr "PAD3.CHOKE")
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
                            |> feed [ "ed hello 4 b part 1 ALGO", "OK" ]
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
                            |> feed [ "ed hello 4 b part 1 ALGO", "OK" ]
                            |> tick 33
                            |> Tuple.first
                            |> feed [ "schema ALGO 2 " ++ algoHash, "OK" ]
                            |> missed (EngineSchema Algo) algoHash
                            |> tick 66
                            |> Tuple.first
                            |> feed [ "page 0 A of 2", "panel A -" ]
                            |> tick 1066
                            |> Tuple.first
                            |> feed [ "OK" ]
                in
                Expect.all
                    [ \x -> Expect.equal (Just "THE UNIT STOPPED ANSWERING") (Model.notice x)
                    , \x -> Expect.equal [] (tick 1099 x |> Tuple.second)
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
                    , \_ -> Expect.equal [ AskSchema (EngineSchema Va) ] (tick 99 m |> Tuple.second)
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
                    [ \_ -> Expect.equal [] (stepSends (Edit cutoff (Num 5000)) switched |> Tuple.second)
                    , \_ -> Expect.equal [] (stepSends (Edit cutoff (Num 5000)) (feed [ "OK 2" ] switched) |> Tuple.second)
                    , \_ -> Expect.equal [] (pick (addr "ENV1.HOLD") 2 switched)
                    , \_ -> Expect.equal False (Model.takesEdits switched)
                    ]
                    ()
        , test "a_new_part_is_a_new_seat" <|
            \_ ->
                Expect.notEqual (Model.seat following) (Model.seat (feed [ "part 3 ALGO" ] following))
        , test "hello_says_which_engine_to_read" <|
            \_ ->
                Model.init
                    |> Model.step (LinkChanged Open)
                    |> Tuple.first
                    |> tick 0
                    |> Tuple.first
                    |> feed [ "ed hello 4 b part 4 MODAL", "OK" ]
                    |> tick 33
                    |> Tuple.second
                    |> Expect.equal [ AskSchema (EngineSchema Modal) ]
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
                            |> feed [ "schema VA 1 00000000000000c3", "OK" ]
                            |> missed (EngineSchema Va) "00000000000000c3"
                            |> tick 132
                            |> Tuple.first
                            |> feed [ "page 0 VA of 1", "panel VA -", "param FILTER.CUTOFF \"CUTOFF\" cont oct 20 20000 1 1000 md", "OK" ]
                            |> tick 165
                            |> Tuple.first
                            |> feed [ "schema MTX-VA 1 00000000000000c4", "OK" ]
                            |> missed (MatrixSchema Va) "00000000000000c4"
                            |> tick 198
                            |> Tuple.first
                            |> feed [ "page 0 MTX of 1", "OK" ]

                    ( _, asked ) =
                        tick 231 described
                in
                Expect.all
                    [ \_ -> Expect.equal [ Since Nothing ] asked
                    , \_ -> Expect.equal [ Set cutoff (Num 1000) ] (stepSends (Edit cutoff (Num 1000)) described |> Tuple.second)
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
                            |> Maybe.map (\i -> following |> stepSends (Pick (addr "ENV1.HOLD") i))
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
                    (feed [ "ed hello 5 x part 1 ALGO", "OK" ] m)
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
                    (feed [ "ed hello 3 x part 1 ALGO", "OK" ] m)
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
                                following |> stepSends (FollowPart p2)
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
                            |> stepSends (FollowPart p2)
                            |> Tuple.second
                            |> Expect.equal []

                    Nothing ->
                        Expect.fail "no Part 2"
        , test "the_connect_walk_asks_each_schema_and_keeps_what_it_described" <|
            \_ ->
                let
                    ( m1, c1 ) =
                        greeted |> tick 33

                    ( m2, e2 ) =
                        m1 |> feed [ "schema ALGO 1 " ++ algoHash ] |> Model.step (Line "OK")

                    ( m3, e3 ) =
                        m2 |> missed (EngineSchema Algo) algoHash |> tick 66 |> Tuple.first |> feed algoPage |> Model.step (Line "OK")
                in
                Expect.all
                    [ \_ -> Expect.equal [ AskSchema (EngineSchema Algo) ] c1
                    , \_ -> Expect.equal [ Recall (key (EngineSchema Algo) algoHash) ] e2
                    , \_ -> Expect.equal [ Keep (key (EngineSchema Algo) algoHash) algoPage ] e3
                    , \_ -> Expect.equal [ AskSchema MixerSchema ] (tick 99 m3 |> Tuple.second)
                    ]
                    ()
        , test "a_cached_schema_is_used_without_describing_it" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        greeted
                            |> tick 33
                            |> Tuple.first
                            |> feed [ "schema ALGO 1 " ++ algoHash, "OK" ]
                            |> Model.step (Recalled { key = key (EngineSchema Algo) algoHash, lines = Just algoPage })
                            |> Tuple.first
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "CUTOFF") (Schema.param cutoff m.schema |> Maybe.map .label)
                    , \_ -> Expect.equal [ AskSchema MixerSchema ] (tick 66 m |> Tuple.second)
                    ]
                    ()
        , test "a_cached_copy_that_isnt_the_pages_the_unit_said_is_described_again" <|
            \_ ->
                greeted
                    |> tick 33
                    |> Tuple.first
                    |> feed [ "schema ALGO 2 " ++ algoHash, "OK" ]
                    |> Model.step (Recalled { key = key (EngineSchema Algo) algoHash, lines = Just algoPage })
                    |> Tuple.first
                    |> tick 66
                    |> Tuple.second
                    |> Expect.equal [ Describe (EngineSchema Algo) (page 0) ]
        , test "a_recall_for_another_key_is_ignored" <|
            \_ ->
                greeted
                    |> tick 33
                    |> Tuple.first
                    |> feed [ "schema ALGO 1 " ++ algoHash, "OK" ]
                    |> Model.step (Recalled { key = key (EngineSchema Algo) "ffffffffffffffff", lines = Nothing })
                    |> Tuple.first
                    |> tick 66
                    |> Tuple.second
                    |> Expect.equal []
        , test "an_ok_with_no_schema_line_says_update_the_editor" <|
            \_ ->
                greeted
                    |> tick 33
                    |> Tuple.first
                    |> feed [ "OK" ]
                    |> Model.notice
                    |> Expect.equal (Just "UPDATE THE EDITOR")
        , test "a_new_engine_is_read_once_the_since_answer_ends" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following |> tick 165 |> Tuple.first |> feed [ "part 2 VA", "OK 3" ]
                in
                Expect.all
                    [ \_ -> Expect.equal [ AskSchema (EngineSchema Va) ] (tick 198 m |> Tuple.second)
                    , \_ -> Expect.equal Nothing m.gen
                    , -- An engine already read is used again with no ask.
                      \_ ->
                        m
                            |> tick 198
                            |> Tuple.first
                            |> feed [ "schema VA 1 00000000000000c3", "OK" ]
                            |> missed (EngineSchema Va) "00000000000000c3"
                            |> tick 231
                            |> Tuple.first
                            |> feed [ "page 0 OSC of 1", "panel OSC -", "OK" ]
                            |> tick 250
                            |> Tuple.first
                            |> feed [ "schema MTX-VA 1 00000000000000c4", "OK" ]
                            |> missed (MatrixSchema Va) "00000000000000c4"
                            |> tick 255
                            |> Tuple.first
                            |> feed [ "page 0 MTX of 1", "OK" ]
                            |> tick 264
                            |> Tuple.first
                            |> feed [ "part 1 ALGO", "OK 4" ]
                            |> tick 297
                            |> Tuple.second
                            |> Expect.equal [ Since (gen 4) ]
                    ]
                    ()
        , test "ok_more_asks_again_at_once_with_its_number" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following |> tick 165 |> Tuple.first |> feed [ "part 1 ALGO", cutoffLine "1000", "OK 7 more" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (gen 7) m.gen
                    , \_ -> Expect.equal [ Since (gen 7) ] (tick 198 m |> Tuple.second)
                    , \_ -> Expect.equal (Just "1000 Hz") (shown m)
                    ]
                    ()
        , test "the_mixers_values_are_kept_and_its_header_drops_only_its_own" <|
            \_ ->
                let
                    level : Addr
                    level =
                        addr "PART3.LEVEL"

                    m : Model.Model
                    m =
                        following
                            |> feed [ "part 1 ALGO", cutoffLine "1000", "mixer", "panel REVERB HALL", "val PART3.LEVEL 0.5 0.5 \"64\"", "OK 2" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "64") (Model.shown level m |> Maybe.map .text)
                    , \_ -> Expect.equal (Just "HALL") (Dict.get "REVERB" m.variants)
                    , -- A new Part: PART's whole, the mixer's kept.
                      \_ ->
                        feed [ "part 2 ALGO", "OK 3" ] m
                            |> (\x -> ( Model.shown level x |> Maybe.map .text, shown x, Dict.get "REVERB" x.variants ))
                            |> Expect.equal ( Just "64", Nothing, Just "HALL" )
                    , -- The mixer whole: the Part's kept, the mixer's dropped.
                      \_ ->
                        feed [ "mixer", "panel REVERB QUAD", "OK 3" ] m
                            |> (\x -> ( Model.shown level x, shown x, Dict.get "REVERB" x.variants ))
                            |> Expect.equal ( Nothing, Just "1000 Hz", Just "QUAD" )
                    ]
                    ()
        , test "a_mixer_address_is_edited_like_any_other" <|
            \_ ->
                following
                    |> stepSends (Edit (addr "PART3.LEVEL") (Num 0.25))
                    |> Tuple.second
                    |> Expect.equal [ Set (addr "PART3.LEVEL") (Num 0.25) ]
        , test "the_matrix_schema_is_read_after_the_mixers" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                in
                Expect.all
                    [ \_ -> Expect.equal [ ( "ENV1", "ENV1" ), ( "OP5", "OP 5" ) ] (Schema.matrix m.matrix).sources
                    , \_ -> Expect.equal (Just 16) (Schema.matrix m.matrix).dests
                    , \_ -> Expect.equal (Just ( -127, 127 )) (Schema.matrix m.matrix).range
                    ]
                    ()
        , test "routes_are_kept_from_since_whole_and_diffed" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed [ "part 1 ALGO", "routes 1", "route ENV1:FILTER.CUTOFF 64 \"+64\"", "route LFO1:OP_C.LEVEL -3 \"-3\"", "OK 2" ]

                    after : Model.Model
                    after =
                        m |> feed [ "route ENV1:FILTER.CUTOFF 70 \"+70\"", "unroute LFO1:OP_C.LEVEL", "OK 3" ]
                in
                Expect.all
                    [ \_ -> Expect.equal [ "ENV1:FILTER.CUTOFF", "LFO1:OP_C.LEVEL" ] (Dict.keys m.routes)
                    , \_ -> Expect.equal [ ( "ENV1:FILTER.CUTOFF", { amount = 70, text = "+70" } ) ] (Dict.toList after.routes)
                    , -- Whole again: what isn't repeated is gone.
                      \_ -> Expect.equal [] (feed [ "routes 1", "OK 4" ] after |> .routes |> Dict.toList)
                    , \_ -> Expect.equal [] (feed [ "part 2 ALGO", "OK 4" ] after |> .routes |> Dict.toList)
                    ]
                    ()
        , test "a_route_edit_sends_ed_route" <|
            \_ ->
                case Protocol.amount -40 of
                    Just a ->
                        Expect.all
                            [ \_ ->
                                following
                                    |> stepSends (EditRoute (Protocol.Cell "FM3IN") (Protocol.RouteAmount a))
                                    |> Tuple.second
                                    |> Expect.equal [ SetRoute (Protocol.Cell "FM3IN") (Protocol.RouteAmount a) ]
                            , \_ ->
                                Model.init
                                    |> stepSends (EditRoute (Protocol.Cell "FM3IN") Protocol.RouteOff)
                                    |> Tuple.second
                                    |> Expect.equal []
                            ]
                            ()

                    Nothing ->
                        Expect.fail "-40 is an amount"
        , test "the_settings_schema_is_read_after_the_matrix_with_its_crumbs" <|
            \_ ->
                Expect.all
                    [ \_ -> Expect.notEqual Nothing (Model.status following)
                    , \_ -> Expect.equal [ Just { list = "PERSONAL", row = "THEME" } ] (List.map .crumb (Schema.pages following.settings))
                    , \_ -> Expect.notEqual Nothing (Schema.param (addr "THEME.ACCENT") following.settings)
                    ]
                    ()
        , test "a_settings_value_is_kept_edited_and_picked_like_any_other" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following |> feed [ "part 1 ALGO", "settings", "val THEME.ACCENT 0 0 \"TEAL\"", "OK 2" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just "TEAL") (Model.shown (addr "THEME.ACCENT") m |> Maybe.map .text)
                    , \_ -> Expect.equal [ Set (addr "THEME.ACCENT") (idx 1) ] (pick (addr "THEME.ACCENT") 1 m)
                    , \_ -> Expect.equal [ Set (addr "PART3.SNAP") (Num 1) ] (stepSends (Edit (addr "PART3.SNAP") (Num 1)) m |> Tuple.second)
                    , -- A new Part: PART's whole; SETTINGS' values kept.
                      \_ ->
                        feed [ "part 2 ALGO", "OK 3" ] m
                            |> Model.shown (addr "THEME.ACCENT")
                            |> Maybe.map .text
                            |> Expect.equal (Just "TEAL")
                    ]
                    ()
        , test "the_settings_section_carries_the_project_the_sounds_the_toast_and_the_listing" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed
                                [ "settings"
                                , "project \"MY SONG\" 10 edited gen 2"
                                , "sound 3 \"BASS\" stale"
                                , "toast \"SAVED\""
                                , "projects 2"
                                , "proj 10 \"MY SONG\" ok gen 2"
                                , "proj 11 \"\" bad gen -"
                                , "OK 2"
                                ]

                    whole : Model.Model
                    whole =
                        m |> feed [ "settings", "projects 0", "OK 3" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just ( "MY SONG", Protocol.Edited )) (m.project |> Maybe.map (\p -> ( p.name, p.edits )))
                    , \_ -> Expect.equal (Just { name = "BASS", status = Protocol.SoundStale }) (Dict.get 3 m.sounds)
                    , \_ -> Expect.equal (Just "SAVED") m.toast
                    , \_ -> Expect.equal (Just [ 10, 11 ]) (m.listing |> Maybe.map (.projects >> List.map (.id >> Protocol.projectIdToInt)))
                    , -- Whole again: what isn't repeated is gone.
                      \_ -> Expect.equal ( Nothing, Dict.empty, Just [] ) ( whole.project, whole.sounds, whole.listing |> Maybe.map .projects )
                    ]
                    ()
        , test "the_project_commands_go_out_typed_while_following" <|
            \_ ->
                case ( Protocol.projectName "MY SONG", Protocol.projectId 10, Protocol.generation 2 ) of
                    ( Just n, Just i, Just g ) ->
                        Expect.all
                            [ \_ -> Expect.equal [ ListProjects ] (stepSends AskProjects following |> Tuple.second)
                            , \_ -> Expect.equal [ SaveProject ] (stepSends SaveOwn following |> Tuple.second)
                            , \_ -> Expect.equal [ SaveProjectAs n ] (stepSends (SaveNew n) following |> Tuple.second)
                            , \_ -> Expect.equal [ LoadProject i g Protocol.DiscardEdits ] (stepSends (LoadListed i g Protocol.DiscardEdits) following |> Tuple.second)
                            , -- Not yet following: nothing.
                              \_ -> Expect.equal [] (stepSends AskProjects greeted |> Tuple.second)
                            ]
                            ()

                    _ ->
                        Expect.fail "a name, an id and a generation"
        , test "the_map_schema_is_read_after_settings_and_kept_under_its_hash" <|
            \_ ->
                let
                    kept : List Effect
                    kept =
                        describingMap |> Model.step (Line "OK") |> Tuple.second

                    mm : Schema.MidiMap
                    mm =
                        Schema.midiMap following.midiMap
                in
                Expect.all
                    [ \_ -> Expect.notEqual Nothing (Model.status following)
                    , \_ -> Expect.equal [ Keep (key MapSchema mapHash) mapPage ] kept
                    , \_ -> Expect.equal [ { form = Protocol.FormCc14, kinds = [ Jump, Pickup, ScaleTo ] } ] (List.drop 1 mm.inputs)
                    , \_ -> Expect.equal [ { kind = TwosComp, head = "RELATIVE · TWOS COMPLEMENT", tail = "+1 = 1  -1 = 127" } ] (List.drop 1 mm.kinds)
                    , \_ -> Expect.equal ( Just 16, Just 90 ) ( mm.rows, mm.mapRows )
                    , \_ -> Expect.equal ( 3, 3, mm.span /= Nothing ) ( List.length mm.ccs, List.length mm.msbs, True )
                    ]
                    ()
        , test "a_whole_map_section_fills_the_map_state" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following |> feed (mapSection ++ [ "OK 2" ])
                in
                Expect.all
                    [ \_ -> Expect.equal (Just ( 2, Just "KEYSTEP", On )) (m.mapHeader |> Maybe.map (\h -> ( Unit.partNumber h.part, h.name, h.keep )))
                    , \_ -> Expect.equal [ ( 1, Live ), ( 2, Dead ) ] (m.soundRows |> Dict.toList |> List.map (Tuple.mapSecond .status))
                    , \_ -> Expect.equal [ ( 1, Shadowed ) ] (m.mapRows |> Dict.toList |> List.map (Tuple.mapSecond .status))
                    , \_ -> Expect.equal (Just (Armed (Just cutoff))) (m.learn |> Maybe.map .learn)
                    , \_ -> Expect.equal ( Just cutoff, Nothing ) ( Model.armedOn m, Model.sensingRow m )
                    , \_ -> Expect.equal ( Nothing, Just 3 ) (feed [ "learn sensing S3 \"CC74\"", "OK 3" ] m |> (\x -> ( Model.armedOn x, Model.sensingRow x )))
                    , \_ -> Expect.equal [ "KEYSTEP", "BEATSTEP PRO" ] m.mapFiles
                    , \_ -> Expect.equal (Just ( TwosComp, Nothing )) (Dict.get 2 m.soundRows |> Maybe.map (\r -> ( r.kind, r.addr )))
                    ]
                    ()
        , test "a_map_header_drops_what_the_section_does_not_repeat" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed (mapSection ++ [ "OK 2" ])
                            |> feed [ "map 3 none keep off", "row S1 CC1 JUMP FILTER.CUTOFF 0 255 off live", "learn off \"\"", "OK 3" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just ( 3, Nothing, Off )) (m.mapHeader |> Maybe.map (\h -> ( Unit.partNumber h.part, h.name, h.keep )))
                    , \_ -> Expect.equal [ 1 ] (Dict.keys m.soundRows)
                    , \_ -> Expect.equal [] (Dict.keys m.mapRows)
                    , \_ -> Expect.equal [] m.mapFiles
                    , \_ -> Expect.equal (Just NotLearning) (m.learn |> Maybe.map .learn)
                    ]
                    ()
        , test "a_diff_learn_line_alone_replaces_only_learn" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed (mapSection ++ [ "OK 2" ])
                            |> feed [ "learn pointing \"TURN A KNOB\"", "OK 3" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just { learn = Pointing, note = "TURN A KNOB" }) m.learn
                    , \_ -> Expect.equal [ 1, 2 ] (Dict.keys m.soundRows)
                    , \_ -> Expect.equal [ 1 ] (Dict.keys m.mapRows)
                    , \_ -> Expect.equal [ "KEYSTEP", "BEATSTEP PRO" ] m.mapFiles
                    , \_ -> Expect.notEqual Nothing m.mapHeader
                    , -- A new Part's PART section leaves MAP's alone.
                      \_ -> Expect.equal [ 1, 2 ] (feed [ "part 2 ALGO", "OK 4" ] m |> .soundRows |> Dict.keys)
                    ]
                    ()
        , test "the_map_commands_go_out_typed" <|
            \_ ->
                case Protocol.mapName "MY KEYS" of
                    Just n ->
                        Expect.all
                            [ \_ -> Expect.equal [ MapCmd ListMaps ] (stepSends (MapAct ListMaps) following |> Tuple.second)
                            , \_ -> Expect.equal [ MapCmd (SaveMap (Just n)) ] (stepSends (MapAct (SaveMap (Just n))) following |> Tuple.second)
                            , \_ -> Expect.equal [ MapCmd (LearnAt cutoff) ] (stepSends (MapAct (LearnAt cutoff)) following |> Tuple.second)
                            , -- Not yet following: nothing.
                              \_ -> Expect.equal [] (stepSends (MapAct ListMaps) greeted |> Tuple.second)
                            ]
                            ()

                    Nothing ->
                        Expect.fail "a map name"
        , test "the_orbit_schema_is_read_after_map_and_kept_under_its_hash" <|
            \_ ->
                let
                    kept : List Effect
                    kept =
                        describingOrbit |> Model.step (Line "OK") |> Tuple.second
                in
                Expect.all
                    [ \_ -> Expect.notEqual Nothing (Model.status following)
                    , \_ -> Expect.equal [ Keep (key OrbitSchema orbitHash) orbitPage ] kept
                    , \_ -> Expect.equal [ [ ( "RING", Protocol.ActRing ), ( "SCENE", Protocol.ActScene ) ] ] (Schema.pages following.orbit |> List.map (.acts >> List.map (\a -> ( a.label, a.cell ))))
                    ]
                    ()
        , test "a_build_without_orbit_reads_no_orbit_pages" <|
            \_ ->
                let
                    ( m, effects ) =
                        describingMap
                            |> feed [ "OK" ]
                            |> tick 139
                            |> Tuple.first
                            |> feed [ "schema ORBIT 0 " ++ orbitHash ]
                            |> Model.step (Line "OK")
                in
                Expect.all
                    [ \_ -> Expect.equal [] effects
                    , \_ -> Expect.notEqual Nothing (Model.status m)
                    , \_ -> Expect.equal [] (Schema.pages m.orbit)
                    , \_ -> Expect.equal [ Since Nothing ] (tick 141 m |> Tuple.second)
                    ]
                    ()
        , test "a_whole_orbit_section_fills_the_orbit_state" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following |> feed (orbitSection ++ [ "OK 2" ])
                in
                Expect.all
                    [ \_ -> Expect.equal (Just ( Playing, ClockInt, ( 3, Nothing, Protocol.Edited ) )) (m.song |> Maybe.map (\g -> ( g.playback, g.clock, ( Protocol.sceneNoToInt g.scene, g.queued, g.edits ) )))
                    , \_ -> Expect.equal [ ( 1, ( SlotSaved, 4 ) ), ( 3, ( SlotNeutral, 0 ) ) ] (m.slots |> Dict.toList |> List.map (Tuple.mapSecond (\x -> ( x.state, Protocol.revToInt x.rev ))))
                    , \_ -> Expect.equal [ Just "C#", Just "120", Just "8" ] (List.map (\a -> Model.shown (addr a) m |> Maybe.map .text) [ "ORBIT.KEY", "TEMPO.BPM", "RING1.HITS" ])
                    , \_ -> Expect.equal [ ( 1, 1 ), ( 2, 5 ) ] (m.chords |> Dict.toList |> List.map (Tuple.mapSecond Protocol.degreeToInt))
                    , \_ -> Expect.equal (Just 2741) (m.scale |> Maybe.map Protocol.scaleMaskToInt)
                    , \_ -> Expect.equal [ ( ( 1, 1 ), 60 ), ( ( 2, 32 ), 67 ) ] (m.steps |> Dict.toList |> List.map (Tuple.mapSecond Protocol.noteToInt))
                    , \_ -> Expect.equal [ ( 1, [ 60, 64, 67 ] ) ] (m.pools |> Dict.toList |> List.map (Tuple.mapSecond (Protocol.poolNotes >> List.map Protocol.noteToInt)))
                    ]
                    ()
        , test "an_orbit_header_drops_what_the_section_does_not_repeat" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed ("val PART3.LEVEL 0.5 0.5 \"64\"" :: orbitSection ++ [ "OK 2" ])
                            |> feed [ "orbit", "song stopped clock EXT scene 1 queued 2 edited no", "slot 2 saved rev 1", "OK 3" ]
                in
                Expect.all
                    [ \_ -> Expect.equal (Just ( Stopped, ClockExt, Just 2 )) (m.song |> Maybe.map (\g -> ( g.playback, g.clock, Maybe.map Protocol.sceneNoToInt g.queued )))
                    , \_ -> Expect.equal [ 2 ] (Dict.keys m.slots)
                    , \_ -> Expect.equal ( Dict.isEmpty m.chords, m.scale == Nothing ) ( True, True )
                    , \_ -> Expect.equal ( Dict.isEmpty m.steps, Dict.isEmpty m.pools ) ( True, True )
                    , \_ -> Expect.equal [ Nothing, Nothing, Nothing ] (List.map (\a -> Model.shown (addr a) m) [ "ORBIT.KEY", "TEMPO.BPM", "RING1.HITS" ])
                    , -- Another section's values stay.
                      \_ -> Expect.equal (Just "64") (Model.shown (addr "PART3.LEVEL") m |> Maybe.map .text)
                    ]
                    ()
        , test "an_orbit_diff_replaces_only_its_entries" <|
            \_ ->
                let
                    m : Model.Model
                    m =
                        following
                            |> feed (orbitSection ++ [ "OK 2" ])
                            |> feed [ "slot 1 saved rev 5", "step 1 1 62", "chord 2 4", "pool 1 48", "val RING1.HITS 0.5 16 \"16\"", "OK 3" ]
                in
                Expect.all
                    [ \_ -> Expect.equal [ ( 1, 5 ), ( 3, 0 ) ] (m.slots |> Dict.toList |> List.map (Tuple.mapSecond (.rev >> Protocol.revToInt)))
                    , \_ -> Expect.equal [ ( ( 1, 1 ), 62 ), ( ( 2, 32 ), 67 ) ] (m.steps |> Dict.toList |> List.map (Tuple.mapSecond Protocol.noteToInt))
                    , \_ -> Expect.equal [ ( 1, 1 ), ( 2, 4 ) ] (m.chords |> Dict.toList |> List.map (Tuple.mapSecond Protocol.degreeToInt))
                    , \_ -> Expect.equal [ ( 1, [ 48 ] ) ] (m.pools |> Dict.toList |> List.map (Tuple.mapSecond (Protocol.poolNotes >> List.map Protocol.noteToInt)))
                    , \_ -> Expect.equal [ Just "C#", Just "16" ] (List.map (\a -> Model.shown (addr a) m |> Maybe.map .text) [ "ORBIT.KEY", "RING1.HITS" ])
                    , \_ -> Expect.equal ( True, Just 2741 ) ( m.song /= Nothing, m.scale |> Maybe.map Protocol.scaleMaskToInt )
                    , -- A new Part's PART section leaves ORBIT's alone.
                      \_ -> Expect.equal (Just "C#") (feed [ "part 2 ALGO", "OK 4" ] m |> Model.shown (addr "ORBIT.KEY") |> Maybe.map .text)
                    ]
                    ()
        , test "the_orbit_values_are_set_and_picked" <|
            \_ ->
                Expect.all
                    [ \_ -> Expect.equal [ Set (addr "TEMPO.BPM") (Num 128) ] (stepSends (Edit (addr "TEMPO.BPM") (Num 128)) following |> Tuple.second)
                    , \_ -> Expect.equal [ Set (addr "ORBIT.KEY") (idx 1) ] (pick (addr "ORBIT.KEY") 1 following)
                    ]
                    ()
        , test "the_orbit_commands_go_out_typed_while_following" <|
            \_ ->
                case ( Protocol.sceneNo 2, Protocol.rev 7 ) of
                    ( Just n, Just r ) ->
                        Expect.all
                            [ \_ -> Expect.equal [ OrbitCmd PlaySong ] (stepSends (OrbitAct PlaySong) following |> Tuple.second)
                            , \_ -> Expect.equal [ OrbitCmd (SaveScene n (Over r)) ] (stepSends (OrbitAct (SaveScene n (Over r))) following |> Tuple.second)
                            , \_ -> Expect.equal [ OrbitCmd (ClearScene n r) ] (stepSends (OrbitAct (ClearScene n r)) following |> Tuple.second)
                            , -- Not yet following: nothing.
                              \_ -> Expect.equal [] (stepSends (OrbitAct StopSong) greeted |> Tuple.second)
                            ]
                            ()

                    _ ->
                        Expect.fail "a scene and a rev"
        ]


{-| A whole ORBIT section: playing scene 3, two slots, ORBIT's values, two
chords, the CUSTOM scale, two steps and a pool.
-}
orbitSection : List String
orbitSection =
    [ "orbit"
    , "song playing clock INT scene 3 queued - edited yes"
    , "slot 1 saved rev 4"
    , "slot 3 neutral rev 0"
    , "val ORBIT.KEY 0.0909 1 \"C#\""
    , "val TEMPO.BPM 0.33 120 \"120\""
    , "val RING1.HITS 0.25 8 \"8\""
    , "chord 1 1"
    , "chord 2 5"
    , "scale 2741"
    , "step 1 1 60"
    , "step 2 32 67"
    , "pool 1 60 64 67"
    ]


{-| A whole MAP section: Part 2 on map KEYSTEP, two Sound rows, a map row,
learn armed, the card's maps.
-}
mapSection : List String
mapSection =
    [ "map 2 \"KEYSTEP\" keep on"
    , "row S1 CC74 JUMP FILTER.CUTOFF 0 255 off live"
    , "row S2 CC75 TWOS-COMP - 0 255 off dead"
    , "row M1 CC74 PICKUP PART1.LEVEL 255 0 on shadowed"
    , "learn armed FILTER.CUTOFF \"MOVE A CONTROL\""
    , "mapfile \"KEYSTEP\""
    , "mapfile \"BEATSTEP PRO\""
    ]
