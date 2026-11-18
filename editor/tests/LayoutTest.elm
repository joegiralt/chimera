module LayoutTest exposing (suite)

import Dict exposing (Dict)
import Expect
import Fixture
import Layout.Algo as Algo exposing (Control(..), Section(..))
import Protocol exposing (Addr(..), Kind(..), addrKey)
import Schema
import Test exposing (Test, describe, test)


ctx : List ( String, String ) -> Dict String String
ctx =
    Dict.fromList


{-| Every panel variant the unit can show at once, spread over a few
Sounds: the default of each, then the others.
-}
sounds : List (Dict String String)
sounds =
    let
        base =
            [ ( "FILTER", "CLEAN" ), ( "FX", "SOFT" ), ( "ENV1", "A-FAST" ), ( "ENV2", "A-FAST" ), ( "ENV3", "A-FAST" ), ( "LFO1", "CLASSIC" ), ( "LFO2", "CLASSIC" ), ( "LFO3", "CLASSIC" ), ( "OP1", "TABLE" ), ( "OP2", "TABLE" ), ( "OP3", "TABLE" ), ( "OP4", "TABLE" ), ( "OP5", "TABLE" ), ( "OP6", "TABLE" ) ]

        over xs =
            Dict.union (ctx xs) (ctx base)
    in
    [ ctx base
    , over [ ( "FILTER", "SEM" ), ( "FX", "FUZZ" ), ( "ENV1", "B-LFO" ), ( "LFO1", "FUNC" ), ( "OP1", "FORMANT" ), ( "OP2", "NOISE" ) ]
    , over [ ( "FILTER", "2040" ), ( "FX", "FOLD" ), ( "ENV2", "B-BURST" ), ( "LFO2", "FUNC-LFV" ), ( "OP3", "NOISE" ) ]
    , over [ ( "FILTER", "LADDER" ), ( "FX", "OFF" ), ( "ENV3", "B-ENV" ), ( "ENV1", "A-SLOW" ), ( "OP4", "FORMANT" ) ]
    , over [ ( "FILTER", "DIRTY" ), ( "ENV3", "B-LFV" ), ( "ENV2", "A-MED" ) ]
    ]


overlap : Algo.Cell -> Algo.Cell -> Bool
overlap a b =
    a.col < b.col + b.w && b.col < a.col + a.w && a.row < b.row + b.h && b.row < a.row + a.h


suite : Test
suite =
    describe "Layout.Algo"
        [ test "every_schema_param_is_placed" <|
            \_ ->
                Schema.pages Fixture.schema
                    |> List.concatMap
                        (\pg ->
                            List.concatMap
                                (\pn ->
                                    pn.params
                                        |> List.filter (\p -> Algo.place (Dict.singleton pn.name pn.variant) p.addr == Nothing)
                                        |> List.map (\p -> addrKey p.addr ++ " under " ++ pn.name ++ " " ++ pn.variant)
                                )
                                pg.panels
                        )
                    |> Expect.equal []
        , test "no_placement_points_nowhere" <|
            \_ ->
                Algo.table
                    |> List.filter (\e -> Schema.param (Addr e.addr) Fixture.schema == Nothing)
                    |> List.map .addr
                    |> Expect.equal []
        , test "hero_is_cutoff" <|
            \_ ->
                Algo.place Dict.empty (Addr "FILTER.CUTOFF")
                    |> Maybe.map (\p -> ( p.section, p.control ))
                    |> Expect.equal (Just ( Filter, Hero ))
        , test "only_cutoff_and_morph_are_heroes" <|
            \_ ->
                Algo.table
                    |> List.filter (\e -> e.placed.control == Hero)
                    |> List.map .addr
                    |> List.sort
                    |> Expect.equal [ "ALGO.MORPH", "FILTER.CUTOFF" ]
        , test "sem_output_sits_in_filter_mod_and_the_others_mode_in_filter" <|
            \_ ->
                let
                    at v =
                        Algo.place (ctx [ ( "FILTER", v ) ]) (Addr "FILTER.MODE") |> Maybe.map .section
                in
                Expect.equal [ Just FilterMod, Just Filter, Just Filter ] [ at "SEM", at "CLEAN", at "LADDER" ]
        , test "no_two_controls_share_a_cell_in_any_sound" <|
            \_ ->
                let
                    clashes v =
                        let
                            placed =
                                Algo.items Fixture.schema v |> List.filterMap (\i -> i.placed |> Maybe.map (\p -> ( i.param.label, p )))
                        in
                        placed
                            |> List.concatMap
                                (\( la, a ) ->
                                    placed
                                        |> List.filter (\( lb, b ) -> la < lb && a.section == b.section && overlap a.cell b.cell)
                                        |> List.map (\( lb, _ ) -> la ++ "/" ++ lb)
                                )
                in
                List.concatMap clashes sounds |> Expect.equal []
        , test "the_screens_clear_the_controls" <|
            \_ ->
                let
                    clash v =
                        Algo.items Fixture.schema v
                            |> List.filterMap .placed
                            |> List.concatMap (\p -> Algo.screens |> List.filter (\s -> s.section == p.section && overlap s.cell p.cell))
                            |> List.map .title
                in
                List.concatMap clash sounds |> Expect.equal []
        , test "a_cell_stays_inside_its_section" <|
            \_ ->
                let
                    box s =
                        Algo.sections |> List.filter (\x -> x.section == s) |> List.head |> Maybe.map .box |> Maybe.withDefault { col = 0, row = 0, w = 0, h = 0 }

                    outside c s =
                        c.col < 1 || c.row < 1 || c.col + c.w - 1 > (box s).w || c.row + c.h - 1 > (box s).h
                in
                (List.map (\e -> ( e.addr, e.placed.section, e.placed.cell )) Algo.table
                    ++ List.map (\s -> ( s.title, s.section, s.cell )) Algo.screens
                )
                    |> List.filter (\( _, s, c ) -> outside c s)
                    |> List.map (\( n, _, _ ) -> n)
                    |> Expect.equal []
        , test "a_param_the_layout_lacks_goes_to_unplaced" <|
            \_ ->
                let
                    withExtra =
                        [ "page 0 ALG of 1", "panel ALG -", "param ALGO.MORPH \"MORPH\" cont lin 0 127 1 0", "param ALGO.NEW \"NEW\" cont lin 0 1 1 0" ]
                            |> List.foldl
                                (\raw s ->
                                    case Protocol.parse raw of
                                        Protocol.Answer a ->
                                            Schema.add a s

                                        _ ->
                                            s
                                )
                                Schema.empty
                in
                Algo.items withExtra Dict.empty
                    |> Algo.unplaced
                    |> List.map (\i -> addrKey i.param.addr)
                    |> Expect.equal [ "ALGO.NEW" ]
        , test "the_fixture_leaves_nothing_unplaced_in_any_sound" <|
            \_ ->
                sounds
                    |> List.concatMap (\v -> Algo.items Fixture.schema v |> Algo.unplaced)
                    |> List.map (\i -> addrKey i.param.addr)
                    |> Expect.equal []
        , test "only_the_shown_variants_params_are_in_play" <|
            \_ ->
                let
                    has v a =
                        Algo.items Fixture.schema v |> List.any (\i -> addrKey i.param.addr == a)
                in
                Expect.equal [ True, False, False, True ]
                    [ has (ctx [ ( "OP1", "FORMANT" ) ]) "OP_A.SKIRT"
                    , has (ctx [ ( "OP1", "TABLE" ) ]) "OP_A.SKIRT"
                    , has (ctx [ ( "FILTER", "CLEAN" ) ]) "FILTER.VOICE"
                    , has (ctx [ ( "FILTER", "2040" ) ]) "FILTER.VOICE"
                    ]
        , test "a_variant_panels_label_beats_a_page_of_all_six_operators" <|
            \_ ->
                let
                    label v a =
                        Algo.items Fixture.schema v
                            |> List.filter (\i -> addrKey i.param.addr == a)
                            |> List.head
                            |> Maybe.map .label
                in
                Expect.equal [ Just "WAVE", Just "COARSE", Just "CRS", Just "DET" ]
                    [ label (ctx [ ( "OP1", "FORMANT" ) ]) "OP_A.WAVE"
                    , label (ctx [ ( "OP1", "TABLE" ) ]) "OP_A.CRSE"
                    , label (ctx [ ( "OP1", "FORMANT" ) ]) "OP_A.CRSE"
                    , label Dict.empty "OP_A.DETUN"
                    ]
        , test "filter_mod_lists_the_routes_the_unit_names" <|
            \_ ->
                Algo.routes Fixture.schema (ctx [ ( "FILTER", "SEM" ) ])
                    |> Expect.equal [ "ENV AMT", "KEYBOARD", "FREQ MOD", "RES MOD" ]
        ]
