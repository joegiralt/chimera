module LayoutTest exposing (suite)

import Dict exposing (Dict)
import Expect
import Fixture
import Layout.Shared as Shared exposing (Module)
import Protocol exposing (Addr(..), addrKey)
import Schema exposing (Schema)
import Set exposing (Set)
import Test exposing (Test, describe, test)
import Unit exposing (Engine(..))
import View.EnginePage as EnginePage


{-| The engines with a page, and the views each has.
-}
paged : List Engine
paged =
    [ Algo ]


described : Schema -> Set String
described sch =
    Schema.pages sch
        |> List.concatMap (.panels >> List.concatMap (.params >> List.map (.addr >> addrKey)))
        |> Set.fromList


placed : List Module -> List String
placed =
    List.concatMap (Shared.addresses >> List.map addrKey)


{-| Every variant of every panel, one Sound each: a panel shows its first
variant unless this Sound picks another.
-}
sounds : Schema -> List (Dict String String)
sounds sch =
    let
        variants : Dict String (List String)
        variants =
            Schema.pages sch
                |> List.concatMap .panels
                |> List.filter (\pn -> pn.variant /= "-")
                |> List.foldl (\pn -> Dict.update pn.name (\vs -> Just (Maybe.withDefault [] vs ++ [ pn.variant ]))) Dict.empty

        deepest : Int
        deepest =
            Dict.values variants |> List.map List.length |> List.maximum |> Maybe.withDefault 1
    in
    List.range 0 (deepest - 1)
        |> List.map (\k -> Dict.map (\_ vs -> List.drop (modBy (List.length vs) k) vs |> List.head |> Maybe.withDefault "") variants)


dupes : List String -> List String
dupes xs =
    xs
        |> List.foldl
            (\x ( seen, out ) ->
                if Set.member x seen then
                    ( seen, x :: out )

                else
                    ( Set.insert x seen, out )
            )
            ( Set.empty, [] )
        |> Tuple.second


withExtra : Schema
withExtra =
    [ "page 0 ALG of 1", "panel ALG -", "param ALGO.MORPH \"MORPH\" cont lin 0 127 1 0", "param ALGO.NEW \"NEW\" cont lin 0 1 1 0" ]
        |> List.foldl
            (\raw s ->
                case Protocol.parse raw of
                    Protocol.Answer a ->
                        Schema.add a s

                    Protocol.Ok_ _ ->
                        s

                    Protocol.Queued ->
                        s

                    Protocol.Err_ _ ->
                        s

                    Protocol.SinceTooLarge ->
                        s

                    Protocol.Unknown ->
                        s
            )
            Schema.empty


shownLabels : Engine -> List ( String, String ) -> String -> List ( String, String )
shownLabels e vs title =
    let
        items : List Shared.Item
        items =
            Shared.active (Fixture.schemaFor e) (Dict.fromList vs)
    in
    (EnginePage.layout e).shown
        |> List.filter (\m -> m.title == title)
        |> List.concatMap (Shared.fill (Shared.index items) >> List.concatMap Tuple.second)
        |> List.map (\( i, _ ) -> ( addrKey i.param.addr, i.label ))


suite : Test
suite =
    describe "the engine pages' layouts"
        [ describe "against each engine's own schema"
            (List.concatMap
                (\e ->
                    let
                        name : String
                        name =
                            Unit.engineName e
                    in
                    [ test (name ++ " places every described address in some view") <|
                        \_ ->
                            Set.diff (described (Fixture.schemaFor e)) (Set.fromList (placed (EnginePage.layout e).every))
                                |> Set.toList
                                |> Expect.equal []
                    , test (name ++ " places no address twice") <|
                        \_ ->
                            dupes (placed (EnginePage.layout e).shown)
                                |> Expect.equal []
                    , test (name ++ " leaves nothing to OTHER in any Sound") <|
                        \_ ->
                            sounds (Fixture.schemaFor e)
                                |> List.concatMap (\v -> EnginePage.other (Shared.active (Fixture.schemaFor e) v) (EnginePage.layout e).every)
                                |> List.map (.param >> .addr >> addrKey)
                                |> Expect.equal []
                    ]
                )
                paged
            )
        , test "a_layout_never_names_an_address_no_engine_describes" <|
            \_ ->
                let
                    any : Set String
                    any =
                        List.foldl (\e -> Set.union (described (Fixture.schemaFor e))) Set.empty Unit.allEngines
                in
                paged
                    |> List.concatMap (\e -> placed (EnginePage.layout e).every)
                    |> List.filter (\a -> not (Set.member a any))
                    |> Expect.equal []
        , test "a_param_the_layout_lacks_lands_in_other" <|
            \_ ->
                EnginePage.other (Shared.active withExtra Dict.empty) (EnginePage.layout Algo).every
                    |> List.map (.param >> .addr)
                    |> Expect.equal [ Addr "ALGO.NEW" ]
        , describe "variants are followed"
            [ test "an_operators_formant_band_shows_only_while_its_wave_is_formant" <|
                \_ ->
                    ( shownLabels Algo [ ( "OP1", "FORMANT" ) ] "OP 1" |> List.member ( "OP_A.SKIRT", "SKIRT" )
                    , shownLabels Algo [ ( "OP1", "TABLE" ) ] "OP 1" |> List.map Tuple.first |> List.member "OP_A.SKIRT"
                    )
                        |> Expect.equal ( True, False )
            , test "the_variant_panel_names_the_control" <|
                \_ ->
                    ( shownLabels Algo [ ( "FILTER", "SEM" ) ] "FILTER" |> List.filter (\( a, _ ) -> a == "FILTER.MODE" || a == "FILTER.MORPH")
                    , shownLabels Algo [ ( "FILTER", "CLEAN" ) ] "FILTER" |> List.filter (\( a, _ ) -> a == "FILTER.MODE" || a == "FILTER.MORPH")
                    )
                        |> Expect.equal ( [ ( "FILTER.MODE", "OUTPUT" ), ( "FILTER.MORPH", "MODE" ) ], [ ( "FILTER.MODE", "MODE" ) ] )
            , test "an_envelope_shows_its_types_stages" <|
                \_ ->
                    ( shownLabels Algo [ ( "ENV1", "A-FAST" ) ] "ENVELOPE 1" |> List.map Tuple.second
                    , shownLabels Algo [ ( "ENV1", "B-LFO" ) ] "ENVELOPE 1" |> List.map Tuple.second
                    )
                        |> Expect.equal ( [ "ATTACK", "HOLD", "DECAY", "SUSTAIN", "RELEASE", "E1 SPEED", "E1 HOLD" ], [ "RATE", "PHASE", "TILT", "MODE", "FORM", "E1 SPEED", "E1 HOLD" ] )
            ]
        , test "the_algo_page_is_in_v6s_order" <|
            \_ ->
                (EnginePage.layout Algo).shown
                    |> List.map .title
                    |> Expect.equal [ "ALGORITHM", "PITCH", "OP 1", "OP 2", "OP 3", "OP 4", "OP 5", "OP 6", "FILTER", "FILTER MOD", "VOICE FX", "AMP · OUT", "ENVELOPE 1", "ENVELOPE 2", "ENVELOPE 3", "LFO 1", "LFO 2", "LFO 3", "MOD" ]
        ]
