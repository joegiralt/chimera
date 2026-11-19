module EnginePageTest exposing (suite)

import Dict
import Expect
import Fixture
import FixtureData
import Html.Attributes exposing (attribute)
import Json.Encode as Encode
import Layout.Shared as Shared
import Model
import Protocol exposing (Addr(..), Value(..))
import Schema
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Time
import Transport exposing (Link(..))
import Ui.Control as Control exposing (Direction(..), Travel(..))
import Ui.Page
import Ui.Tokens exposing (Theme(..))
import Unit
import View.EnginePage as Page exposing (Msg(..))


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


{-| Connected over a link, greeted, and the whole ALGO schema described the
way the model reads it, then one `since` answer: following Part 1.
-}
connected : List String -> String -> Model.Model
connected answer engine =
    List.foldl (\( k, chunk ) m -> m |> tick (100 + 33 * k) |> feed chunk)
        (Model.init |> Model.step (Model.LinkChanged Open) |> Tuple.first |> tick 0 |> feed [ "ed hello 3 b part 1 " ++ engine, "OK" ])
        (List.indexedMap Tuple.pair (pagesOf answer))
        |> tick 50000


model : Model.Model
model =
    connected FixtureData.lines "ALGO"
        |> feed [ "val FILTER.CUTOFF 0.5 632.4555 \"632 Hz\"", "val FILTER.KIND 0 0 \"CLEAN\"", "panel FILTER CLEAN", "panel OP1 TABLE", "panel ENV1 A-FAST", "OK 1" ]


cutoff : Addr
cutoff =
    Addr "FILTER.CUTOFF"


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


{-| Each engine followed, with a value on its first control.
-}
engines : List ( String, Model.Model )
engines =
    [ ( "ALGO", model )
    , ( "VA", connected FixtureData.va "VA" |> feed [ "val VA.WAVE1 0.25 0.25 \"SAW\"", "panel FILTER SEM", "OK 1" ] )
    , ( "MODAL", connected FixtureData.modal "MODAL" |> feed [ "panel RESONATOR SYMP", "OK 1" ] )
    , ( "TAPE", connected FixtureData.tape "TAPE" |> feed [ "panel MACHINE STRCH", "OK 1" ] )
    ]


grab : Page.Msg
grab =
    Hand (Control.Grab cutoff Hero 300)


pick : String -> Int -> List Model.Msg
pick a i =
    Protocol.optIndex i |> Maybe.map (\o -> [ Model.Pick (Addr a) o ]) |> Maybe.withDefault []


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
                            run [ Hand (Control.Pick (Addr "FILTER.KIND") one), Hand (Control.Pick (Addr "FILTER.KIND") two), Frame ]
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
                    Page.view model (Page.init Day)
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.tag "chimera-screen", Sel.attribute (attribute "page" "22") ]
                        |> Query.count (Expect.equal 1)
            , test "before_following_it_offers_the_links" <|
                \_ ->
                    Page.view Model.init (Page.init Night)
                        |> Query.fromHtml
                        |> Query.find [ Sel.tag "button", Sel.containing [ Sel.text "CONNECT SIMULATOR" ] ]
                        |> Event.simulate Event.click
                        |> Event.expect (Unit Model.ConnectSocket)
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
        , test "the_folds_and_chips_are_the_pages_own" <|
            \_ ->
                Page.update (PageMsg (Ui.Page.ToggleChip "MOD")) model (Page.init Day)
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
