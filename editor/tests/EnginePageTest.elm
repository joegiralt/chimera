module EnginePageTest exposing (suite)

import Addr exposing (Addr)
import Contract exposing (opt)
import Dict
import Expect
import Fixture
import FixtureData
import Hash
import Html.Attributes exposing (attribute)
import Json.Encode as Encode
import Layout.Algo
import Layout.Page exposing (PadPage(..), UnitPage(..))
import Layout.Shared as Shared
import Model
import Protocol exposing (Value(..))
import Schema
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Time
import Transport exposing (Link(..))
import Ui.Chip as Chip
import Ui.Control as Control exposing (Direction(..), Travel(..))
import Ui.Pad as Pad
import Ui.PadStrip as PadStrip exposing (Tab(..))
import Ui.Page
import Ui.PatchBay as PatchBay
import Ui.Routes as Routes
import Ui.Screen as Screen
import Ui.Tokens exposing (Theme(..))
import Unit
import View.EnginePage as Page exposing (BayId(..), Msg(..))
import Wire exposing (addr)


feed : List String -> Model.Model -> Model.Model
feed ls m =
    List.foldl (\l -> Model.step (Model.Line l) >> Tuple.first) m ls


tick : Int -> Model.Model -> Model.Model
tick ms =
    Model.step (Model.Tick (Time.millisToPosix ms)) >> Tuple.first


{-| The fixture's answer split at each OK: one chunk a page.
-}
pagesOf : List String -> List (List String)
pagesOf ls =
    List.foldl
        (\l ( cur, out ) ->
            if l == "OK" then
                ( [], out ++ [ cur ++ [ l ] ] )

            else
                ( cur ++ [ l ], out )
        )
        ( [], [] )
        ls
        |> Tuple.second


{-| Connected over a link, greeted, and the engine's, the MIXER's, a
one-page matrix schema, SETTINGS', a one-page MAP schema and ORBIT's read the
way the model reads them (no cache), then one `since` answer: following Part 1.
-}
connected : List String -> String -> Model.Model
connected answer engine =
    Model.init
        |> Model.step (Model.LinkChanged Open)
        |> Tuple.first
        |> tick 0
        |> feed [ "ed hello 4 b part 1 " ++ engine, "OK" ]
        |> readSchema 1 engine answer
        |> readSchema 2 "MIXER" FixtureData.mixer
        |> readSchema 3 ("MTX-" ++ engine) [ "page 0 MTX of 1", "OK" ]
        |> readSchema 4 "SETTINGS" FixtureData.settings
        |> readSchema 5 "MAP" FixtureData.map
        |> readSchema 6 "ORBIT" FixtureData.orbit
        |> tick 50000


{-| `ed schema` answered with the hash of the answer's own text, the cache
missing, then every page described.
-}
readSchema : Int -> String -> List String -> Model.Model -> Model.Model
readSchema k token answer m0 =
    let
        hash : String
        hash =
            Hash.toHex (Hash.ofLines (List.filter (\l -> l /= "OK") answer))

        pages : List (List String)
        pages =
            pagesOf answer
    in
    List.foldl (\( i, chunk ) m -> m |> tick (10000 * k + 33 * (i + 1)) |> feed chunk)
        (m0
            |> tick (10000 * k)
            |> feed [ "schema " ++ token ++ " " ++ String.fromInt (List.length pages) ++ " " ++ hash, "OK" ]
            |> Model.step (Model.Recalled { key = "4/" ++ token ++ "/" ++ hash, lines = Nothing })
            |> Tuple.first
        )
        (List.indexedMap Tuple.pair pages)


model : Model.Model
model =
    connected FixtureData.lines "ALGO"
        |> feed [ "val FILTER.CUTOFF 0.5 632.4555 \"632 Hz\"", "val FILTER.KIND 0 0 \"CLEAN\"", "panel FILTER CLEAN", "panel OP1 TABLE", "panel ENV1 A-FAST", "OK 1" ]


cutoff : Addr
cutoff =
    addr "FILTER.CUTOFF"


cutoffParam : Schema.Param
cutoffParam =
    Fixture.param "FILTER.CUTOFF"


run : List Page.Msg -> List Model.Msg
run =
    runOn model


runOn : Model.Model -> List Page.Msg -> List Model.Msg
runOn unit =
    List.foldl
        (\msg ( s, out ) ->
            Page.update msg unit s |> (\( s1, m, _ ) -> ( s1, out ++ m ))
        )
        ( Page.init Day, [] )
        >> Tuple.second


{-| Each engine followed, every param at its default, as the unit reports
it after loading INIT.
-}
engines : List ( String, Model.Model )
engines =
    [ ( "ALGO", model |> reported )
    , ( "VA", connected FixtureData.va "VA" |> feed [ "panel FILTER SEM", "OK 1" ] |> reported )
    , ( "MODAL", connected FixtureData.modal "MODAL" |> feed [ "panel RESONATOR SYMP", "OK 1" ] |> reported )
    , ( "TAPE", connected FixtureData.tape "TAPE" |> feed [ "panel MACHINE STRCH", "OK 1" ] |> reported )
    ]


{-| Every param the schema holds, reported at its default.
-}
reported : Model.Model -> Model.Model
reported m =
    let
        line : Schema.Param -> String
        line p =
            "val "
                ++ Addr.key p.addr
                ++ " "
                ++ String.fromFloat
                    (if p.max > p.min then
                        clamp 0 1 ((p.default - p.min) / (p.max - p.min))

                     else
                        0
                    )
                ++ " "
                ++ String.fromFloat p.default
                ++ " \"x\""
    in
    feed (List.filterMap (\a -> Schema.param a m.schema |> Maybe.map line) (Schema.addresses m.schema) ++ [ "OK 2" ]) m


grab : Page.Msg
grab =
    Hand (Control.Grab cutoff Hero 300)


pick : String -> Int -> List Model.Msg
pick a i =
    Protocol.optIndex i |> Maybe.map (\o -> [ Model.Pick (addr a) o ]) |> Maybe.withDefault []


suite : Test
suite =
    describe "View.EnginePage"
        [ describe "a hand reaches the unit as the old face's did"
            [ test "a_drag_asks_nothing_until_the_frame" <|
                \_ ->
                    run [ grab, Hand (Control.Moved 290), Hand (Control.Moved 280) ]
                        |> Expect.equal []
            , test "a_whole_drag_in_one_frame_sends_one_set_for_the_control" <|
                \_ ->
                    run [ grab, Hand (Control.Moved 290), Hand (Control.Moved 280), Hand (Control.Moved 270), Frame ]
                        |> List.length
                        |> Expect.equal 1
            , test "that_one_set_is_the_last_position" <|
                \_ ->
                    -- 0.5 + 60 / 240 = 0.75 of ten octaves above 20 Hz
                    run [ grab, Hand (Control.Moved 290), Hand (Control.Moved 240), Frame ]
                        |> Expect.equal [ Model.Edit cutoff (Num (Control.atNorm cutoffParam 0.75)) ]
            , test "the_next_frame_sends_nothing_new" <|
                \_ ->
                    run [ grab, Hand (Control.Moved 290), Frame, Frame ]
                        |> List.length
                        |> Expect.equal 1
            , test "a_released_drag_still_flushes_its_last_want" <|
                \_ ->
                    run [ grab, Hand (Control.Moved 250), Hand Control.Released, Frame ]
                        |> List.length
                        |> Expect.equal 1
            , test "a_choice_click_sends_a_pick_once_per_frame" <|
                \_ ->
                    case ( Protocol.optIndex 1, Protocol.optIndex 2 ) of
                        ( Just one, Just two ) ->
                            run [ Hand (Control.Pick (addr "FILTER.KIND") one), Hand (Control.Pick (addr "FILTER.KIND") two), Frame ]
                                |> Expect.equal (pick "FILTER.KIND" 2)

                        _ ->
                            Expect.fail "no option"
            , test "a_wheel_notch_steps_by_the_params_own_step" <|
                \_ ->
                    run [ Hand (Control.Step cutoff Up), Frame ]
                        |> Expect.equal [ Model.Edit cutoff (Num (Control.atNorm cutoffParam (0.5 + Control.wheelNorm cutoffParam))) ]
            , test "a_double_click_restores_the_units_default" <|
                \_ ->
                    run [ Hand (Control.Reset cutoff), Frame ]
                        |> Expect.equal [ Model.Edit cutoff (Num cutoffParam.default) ]
            , describe "every shown control answers a hand with the unit's edit"
                (List.map
                    (\( name, unit ) ->
                        test name <|
                            \_ ->
                                Shared.active unit.schema unit.variants
                                    |> List.filter (\i -> runOn unit (hands i) /= expected unit i)
                                    |> List.map (.param >> .addr)
                                    |> Expect.equal []
                    )
                    engines
                )
            ]
        , describe "a hand lets go when the box takes another Part"
            [ test "a_held_drag_writes_nothing_after_the_part_changes" <|
                \_ ->
                    let
                        ( held, _, _ ) =
                            Page.update grab model (Page.init Day)

                        ( after, moved, _ ) =
                            Page.update (Hand (Control.Moved 250)) elsewhere held

                        ( _, flushed, _ ) =
                            Page.update Frame elsewhere after
                    in
                    Expect.equal [] (moved ++ flushed)
            , test "wants_made_before_the_part_changed_are_dropped" <|
                \_ ->
                    let
                        ( held, _, _ ) =
                            Page.update grab model (Page.init Day)

                        ( wanting, _, _ ) =
                            Page.update (Hand (Control.Moved 250)) model held

                        ( _, flushed, _ ) =
                            Page.update Frame elsewhere wanting
                    in
                    Expect.equal [] flushed
            , test "the_page_follows_the_box_through_the_model_alone" <|
                \_ ->
                    let
                        ( held, _, _ ) =
                            Page.update grab model (Page.init Day)

                        ( wanting, _, _ ) =
                            Page.update (Hand (Control.Moved 250)) model held
                    in
                    Page.follow elsewhere wanting
                        |> .wanted
                        |> Control.isEmpty
                        |> Expect.equal True
            ]
        , describe "every control shows the unit's value, never a guess"
            [ test "a_drag_moves_nothing_on_the_page_until_the_unit_reports" <|
                \_ ->
                    let
                        ( held, _, _ ) =
                            Page.update grab model (Page.init Day)

                        ( wanting, _, _ ) =
                            Page.update (Hand (Control.Moved 200)) model held
                    in
                    Page.view model wanting
                        |> Query.fromHtml
                        |> Query.find [ Sel.attribute (attribute "role" "slider"), Sel.attribute (attribute "aria-label" "FREQ") ]
                        |> Query.has [ Sel.attribute (attribute "aria-valuetext" "632 Hz") ]
            , test "a_refusal_shows_on_its_control_with_the_units_reason" <|
                \_ ->
                    Page.view (feed [ "refused FILTER.CUTOFF \"FILTER IS OFF\"", "OK 2" ] model) (Page.init Day)
                        |> Query.fromHtml
                        |> Query.find [ Sel.class "c", Sel.class "refused", Sel.containing [ Sel.text "FREQ" ] ]
                        |> Query.has [ Sel.attribute (attribute "title" "THE UNIT REFUSED: FILTER IS OFF"), Sel.text "632 Hz" ]
            , test "an_err_to_a_set_shows_on_its_control_the_same_way" <|
                \_ ->
                    Page.view (feed [ "ERR ed set FILTER.CUTOFF takes a number" ] model) (Page.init Day)
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.class "c", Sel.class "refused", Sel.containing [ Sel.text "FREQ" ] ]
                        |> Query.count (Expect.equal 1)
            , test "a_refusal_that_is_no_controls_is_said_on_the_page_in_the_units_words" <|
                \_ ->
                    Page.view (feed [ "ERR busy", "ERR busy", "wibble 3" ] model) (Page.init Day)
                        |> Query.fromHtml
                        |> Query.find [ Sel.class "said" ]
                        |> Query.has [ Sel.text "THE UNIT REFUSED: busy", Sel.text "×2", Sel.text "THE UNIT SAID A LINE THIS EDITOR CAN'T READ: wibble 3" ]
            , test "seeing_the_notices_tells_the_model" <|
                \_ ->
                    Page.view (feed [ "ERR busy" ] model) (Page.init Day)
                        |> Query.fromHtml
                        |> Query.find [ Sel.class "said" ]
                        |> Query.find [ Sel.tag "button" ]
                        |> Event.simulate Event.click
                        |> Event.expect (Unit Model.SeenNotices)
            , test "nothing_kept_says_nothing" <|
                \_ ->
                    Page.view model (Page.init Day)
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.class "said" ]
                        |> Query.count (Expect.equal 0)
            , test "a_notice_is_said_on_the_connect_page_too_with_how_many_were_lost" <|
                \_ ->
                    Page.view (feed (List.range 1 17 |> List.map (\n -> "wibble " ++ String.fromInt n)) Model.init) (Page.init Day)
                        |> Query.fromHtml
                        |> Query.find [ Sel.class "said" ]
                        |> Query.has [ Sel.text "THE UNIT SAID A LINE THIS EDITOR CAN'T READ: wibble 17", Sel.text "AND 1 MORE NO LONGER KEPT" ]
            , test "the_next_value_clears_the_refusal" <|
                \_ ->
                    Page.view (feed [ "refused FILTER.CUTOFF \"FILTER IS OFF\"", "OK 2", "val FILTER.CUTOFF 0.5 632.4555 \"632 Hz\"", "OK 3" ] model) (Page.init Day)
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.class "refused" ]
                        |> Query.count (Expect.equal 0)
            , test "a_value_not_reported_is_a_missing_cell" <|
                \_ ->
                    Page.view model (Page.init Day)
                        |> Query.fromHtml
                        |> Query.find [ Sel.attribute (attribute "role" "slider"), Sel.attribute (attribute "aria-label" "Q") ]
                        |> Query.has [ Sel.attribute (attribute "aria-valuetext" "--") ]
            , test "a_value_not_reported_cannot_be_dragged" <|
                \_ ->
                    run [ Hand (Control.Grab (addr "FILTER.RESO") Knob 300), Hand (Control.Moved 250), Frame ]
                        |> Expect.equal []
            , test "a_choice_off_its_list_says_so" <|
                \_ ->
                    Page.view (feed [ "val FILTER.KIND 1 99 \"??\"", "OK 2" ] model) (Page.init Day)
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.class "hmn", Sel.containing [ Sel.text "??" ] ]
                        |> Query.count (Expect.equal 1)
            ]
        , describe "the screens' spec"
            [ test "the_algo_chart_is_drawn_with_morphs_cell_in_focus" <|
                \_ ->
                    Shared.screen algorithmModule
                        |> Maybe.andThen (Page.screenSpec model Unit.Algo)
                        |> Maybe.andThen Screen.spec
                        |> Maybe.andThen .focus
                        |> Expect.equal (Just Layout.Algo.chartFocus)
            , test "the_chart_focus_is_never_alg_a_or_b" <|
                \_ ->
                    List.member Layout.Algo.chartFocus [ Screen.CellA, Screen.CellB ] |> Expect.equal False
            , test "a_screen_carries_only_its_pages_addresses_and_selectors_in_describe_order" <|
                \_ ->
                    Shared.screen algorithmModule
                        |> Maybe.andThen (Page.screenSpec (reported model) Unit.Algo)
                        |> Maybe.andThen Screen.spec
                        |> Maybe.map (.vals >> List.map (Tuple.first >> Addr.key))
                        |> Expect.equal (Just [ "ALGO.ALG_A", "ALGO.ALG_B", "ALGO.MORPH", "ALGO.TRNSP", "OUT.LEVEL", "ALGO.VOWEL" ])
            , test "a_variant_pages_screen_carries_its_selector" <|
                \_ ->
                    Page.screenSpec (reported model) Unit.Algo (Shared.tall Flt)
                        |> Maybe.andThen Screen.spec
                        |> Maybe.map (.vals >> List.map (Tuple.first >> Addr.key) >> List.member "FILTER.KIND")
                        |> Expect.equal (Just True)
            , describe "every engine's screens hold nothing from another page"
                (List.map
                    (\( name, unit ) ->
                        test name <|
                            \_ ->
                                (Page.layout (engineOf name) Kit).every
                                    |> List.filterMap Shared.screen
                                    |> List.filterMap (Page.screenSpec unit (engineOf name) >> Maybe.andThen Screen.spec)
                                    |> List.concatMap (\sp -> List.filter (\( a, _ ) -> not (onPage unit sp.page a)) sp.vals)
                                    |> List.map (Tuple.first >> Addr.key)
                                    |> Expect.equal []
                    )
                    engines
                )
            , test "the_pad_strip_draws_each_pads_own_machine_icon" <|
                \_ ->
                    Page.view drum (Page.init Day)
                        |> Query.fromHtml
                        |> Query.find [ Sel.class "padstrip" ]
                        |> Query.findAll [ Sel.tag "chimera-screen" ]
                        |> Query.index 2
                        |> Query.has [ Sel.attribute (attribute "spec" "{\"engine\":\"DRUM\",\"page\":37,\"region\":\"padicon\",\"focus\":null,\"pad\":3,\"vals\":[[\"PAD3.MACHINE\",4]]}") ]
            , test "a_kit_screen_draws_for_its_own_pad" <|
                \_ ->
                    Page.screenSpec drum Unit.Drum (Shared.strip (PadPage Pad.Pad7 PadKit))
                        |> Maybe.andThen Screen.spec
                        |> Maybe.andThen .pad
                        |> Expect.equal (Just Pad.Pad7)
            ]
        , describe "the page"
            [ test "the_hero_cutoff_knob_grabs_with_the_hero_travel" <|
                \_ ->
                    Page.view model (Page.init Day)
                        |> Query.fromHtml
                        |> Query.find [ Sel.attribute (attribute "role" "slider"), Sel.attribute (attribute "aria-label" "FREQ") ]
                        |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 300 ) ]))
                        |> Event.expect (Hand (Control.Grab cutoff Hero 300))
            , test "a_part_tab_asks_the_box_to_follow_it" <|
                \_ ->
                    case Unit.partFromInt 3 of
                        Just p3 ->
                            Page.view model (Page.init Day)
                                |> Query.fromHtml
                                |> Query.find [ Sel.tag "button", Sel.containing [ Sel.text "P3" ] ]
                                |> Event.simulate Event.click
                                |> Event.expect (Unit (Model.FollowPart p3))

                        Nothing ->
                            Expect.fail "no part 3"
            , test "the_mod_module_says_the_unit_does_not_describe_its_matrix" <|
                \_ ->
                    Page.view model (Page.init Day)
                        |> Query.fromHtml
                        |> Query.has [ Sel.text "THE UNIT DOESN'T DESCRIBE ITS MOD MATRIX YET" ]
            , test "the_screens_are_the_units_pages_by_number" <|
                \_ ->
                    Page.screenSpec model Unit.Algo (Shared.tall Flt)
                        |> Maybe.andThen Screen.spec
                        |> Maybe.map (.page >> Protocol.pageNoToInt)
                        |> Expect.equal (Just 22)
            , test "the_algo_page_stays_in_params_order_whatever_the_units_algo_pages_setting" <|
                \_ ->
                    -- PERSONALIZE › ALGO PAGES (THEME.ALGO_FIRST) orders the
                    -- unit's own page walk; the editor's is PARAMS first by design.
                    let
                        opsFirst : Model.Model
                        opsFirst =
                            feed [ "settings", "val THEME.ALGO_FIRST 1 1 \"OPS\"", "OK 2" ] model
                    in
                    Expect.all
                        [ \_ -> Expect.equal (Just [ "PARAMS", "OPS" ]) (Schema.param (addr "THEME.ALGO_FIRST") opsFirst.settings |> Maybe.map (.options >> List.map Tuple.second))
                        , \_ -> Expect.equal (Just "OPS") (Model.shown (addr "THEME.ALGO_FIRST") opsFirst |> Maybe.map .text)
                        , \_ ->
                            Page.view opsFirst (Page.init Day)
                                |> Query.fromHtml
                                |> Query.findAll [ Sel.tag "h2" ]
                                |> Expect.all
                                    [ Query.index 0 >> Query.has [ Sel.text "ALGORITHM" ]
                                    , Query.index 1 >> Query.has [ Sel.text "PITCH" ]
                                    , Query.index 2 >> Query.has [ Sel.text "OP 1" ]
                                    ]
                        ]
                        ()
            , test "before_following_it_offers_the_links" <|
                \_ ->
                    Page.view Model.init (Page.init Night)
                        |> Query.fromHtml
                        |> Query.find [ Sel.tag "button", Sel.containing [ Sel.text "CONNECT SIMULATOR" ] ]
                        |> Event.simulate Event.click
                        |> Event.expect (Unit (Model.Connect Model.ViaSimulator))
            , test "the_schema_is_all_on_the_page_nothing_in_other" <|
                \_ ->
                    Page.view model (Page.init Day)
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.tag "h2", Sel.containing [ Sel.text "OTHER" ] ]
                        |> Query.count (Expect.equal 0)
            ]
        , test "tapes_source_is_printed_read_only" <|
            \_ ->
                Page.view (connected FixtureData.tape "TAPE" |> feed [ "source CHOIR", "panel MACHINE WALK", "OK 1" ]) (Page.init Day)
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "CHOIR" ]
        , test "a_module_with_nothing_for_this_type_says_so" <|
            \_ ->
                Page.view (connected FixtureData.modal "MODAL" |> feed [ "panel RESONATOR BOWED", "OK 1" ]) (Page.init Day)
                    |> Query.fromHtml
                    |> Query.has [ Sel.text "NOTHING TO SET FOR THIS TYPE" ]
        , describe "DRUM"
            [ test "the_pad_strip_opens_a_pads_view" <|
                \_ ->
                    let
                        ( s, _, _ ) =
                            Page.update (PadMsg (PadStrip.Select (PadTab Pad.Pad5))) drum (Page.init Day)
                    in
                    Page.view drum s
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.attribute (attribute "aria-label" "LEVEL") ]
                        |> Query.count (Expect.equal 1)
            , test "the_kit_view_shows_every_pads_level" <|
                \_ ->
                    Page.view drum (Page.init Day)
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.attribute (attribute "role" "slider"), Sel.attribute (attribute "aria-label" "LEVEL") ]
                        |> Query.count (Expect.equal 12)
            , test "a_patched_route_is_a_cable" <|
                \_ ->
                    Page.view drum (onPad Pad.Pad1)
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.attribute (attribute "aria-label" "LFO to RESO") ]
                        |> Query.count (Expect.equal 1)
            , test "patching_takes_the_first_free_route" <|
                \_ ->
                    runOn drum [ BayMsg (PadBay Pad.Pad1) (PatchBay.Connect (Routes.source ( opt 2, "VEL" )) (Routes.dest ( opt 2, "VOLUME" ))), Frame ]
                        |> Expect.equal
                            (Model.Edit (addr "PAD1_RT2.AMOUNT") (Num (Control.atNorm (Fixture.paramIn drumSchema "PAD1_RT2.AMOUNT") 0.75))
                                :: pick "PAD1_RT2.DEST" 2
                                ++ pick "PAD1_RT2.SOURCE" 2
                            )
            , test "unpatching_sets_the_amount_to_nothing" <|
                \_ ->
                    runOn drum [ BayMsg (PadBay Pad.Pad1) (PatchBay.Unpatch (Routes.source ( opt 1, "LFO" )) (Routes.dest ( opt 1, "RESO" ))), Frame ]
                        |> Expect.equal [ Model.Edit (addr "PAD1_RT1.AMOUNT") (Num 0) ]
            , test "a_cables_attenuverter_turns_its_amount" <|
                \_ ->
                    runOn drum [ BayMsg (PadBay Pad.Pad1) (PatchBay.Hand (Control.Grab (addr "PAD1_RT1.AMOUNT") Attenuverter 300)), Hand (Control.Moved 276), Frame ]
                        |> Expect.equal [ Model.Edit (addr "PAD1_RT1.AMOUNT") (Num (Control.atNorm (Fixture.paramIn drumSchema "PAD1_RT1.AMOUNT") 0.85)) ]
            ]
        , test "the_folds_and_chips_are_the_pages_own" <|
            \_ ->
                Page.update (PageMsg (Ui.Page.ToggleChip Chip.Mod)) model (Page.init Day)
                    |> (\( _, out, _ ) -> out)
                    |> Expect.equal []
        , test "the_theme_switch_is_stored" <|
            \_ ->
                Page.update (PageMsg (Ui.Page.ChoseTheme Night)) model (Page.init Day)
                    |> (\( _, _, store ) -> store)
                    |> Expect.equal (Just Night)
        , test "values_of_another_part_are_not_shown" <|
            \_ ->
                Dict.size model.values |> Expect.equal 2
        ]


{-| The box on Part 2, the same engine: a new seat.
-}
elsewhere : Model.Model
elsewhere =
    model |> feed [ "part 2 ALGO", "val FILTER.CUTOFF 0.5 632.4555 \"632 Hz\"", "OK 2" ]


algorithmModule : Shared.Module
algorithmModule =
    Layout.Algo.modules |> List.filter (\md -> Shared.title md == "ALGORITHM") |> List.head |> Maybe.withDefault Shared.mod


engineOf : String -> Unit.Engine
engineOf name =
    Unit.engineFromName name |> Maybe.withDefault Unit.Algo


{-| Whether `a` is on page `n`, or picks a variant of a panel there.
-}
onPage : Model.Model -> Protocol.PageNo -> Addr -> Bool
onPage unit n a =
    List.member a (Schema.pageAddresses n unit.schema)
        || (Schema.pages unit.schema
                |> List.filter (\pg -> pg.n == n)
                |> List.concatMap .panels
                |> List.filterMap (.name >> Layout.Page.variantSelector)
                |> List.member a
           )


drumSchema : Schema.Schema
drumSchema =
    Fixture.schemaFor Unit.Drum


{-| DRUM followed: pad 1's route 1 patched LFO to RESO at +64 (norm 0.75).
-}
drum : Model.Model
drum =
    connected FixtureData.drum "DRUM"
        |> feed
            [ "val PAD1_RT1.SOURCE 0.2 1 \"LFO\""
            , "val PAD1_RT1.DEST 0.25 1 \"RESO\""
            , "val PAD1_RT1.AMOUNT 0.75 63.5 \"+64\""
            , "val PAD1_RT2.AMOUNT 0.5 0 \"0\""
            , "val PAD3.MACHINE 0.2 4 \"TONE CLAP\""
            , "OK 1"
            ]


onPad : Pad.Pad -> Page.State
onPad p =
    Page.update (PadMsg (PadStrip.Select (PadTab p))) drum (Page.init Day) |> (\( s, _, _ ) -> s)


{-| A hand on item `i`: a choice picks its last option; a value drags up 30
pixels at the knob's travel.
-}
hands : Shared.Item -> List Page.Msg
hands i =
    case List.reverse i.param.options of
        ( o, _ ) :: _ ->
            [ Hand (Control.Pick i.param.addr o), Frame ]

        [] ->
            [ Hand (Control.Grab i.param.addr Knob 300), Hand (Control.Moved 270), Frame ]


expected : Model.Model -> Shared.Item -> List Model.Msg
expected unit i =
    case List.reverse i.param.options of
        ( o, _ ) :: _ ->
            [ Model.Pick i.param.addr o ]

        [] ->
            [ Model.Edit i.param.addr (Num (Control.atNorm i.param ((Model.shown i.param.addr unit |> Maybe.map .norm |> Maybe.withDefault 0) + 30 / 150))) ]
