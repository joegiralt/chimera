module PatchBayTest exposing (suite)

import Contract exposing (opt)
import Expect
import Html.Attributes exposing (attribute)
import Json.Encode as Encode
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Control as Control exposing (Travel(..))
import Ui.MatrixGrid as MatrixGrid
import Ui.PatchBay as PatchBay exposing (Effect(..), Msg(..), View(..))
import Ui.Routes as Routes exposing (Dest, Routes, Sign(..), Source)
import Wire exposing (addr)


env1 : Source
env1 =
    Routes.source ( opt 0, "ENV1" )


velo : Source
velo =
    Routes.source ( opt 2, "VELO" )


freq : Dest
freq =
    Routes.dest ( opt 0, "FREQ" )


q : Dest
q =
    Routes.dest ( opt 1, "Q" )


routes : List Int -> Routes
routes amounts =
    Routes.new [ env1, Routes.source ( opt 1, "LFO1" ), velo ] [ freq, q, Routes.dest ( opt 2, "VCA" ) ]
        |> (\r ->
                List.foldl
                    (\( ( s, d ), a ) acc -> Routes.add { from = s, to = d, amount = Routes.amount a, addr = addr ("RT.A" ++ String.fromInt (abs a)) } acc)
                    r
                    (List.map2 Tuple.pair [ ( env1, freq ), ( velo, q ) ] amounts)
           )


bay : PatchBay.Bay
bay =
    PatchBay.init


suite : Test
suite =
    describe "Ui.PatchBay and Ui.MatrixGrid"
        [ describe "amounts"
            [ test "an_amount_is_kept_to_plus_minus_100" <|
                \_ -> List.map (Routes.amount >> Routes.amountValue) [ -300, -100, 0, 42, 101 ] |> Expect.equal [ -100, -100, 0, 42, 100 ]
            , test "an_amount_prints_its_sign" <|
                \_ -> List.map (Routes.amount >> Routes.amountText) [ -45, 0, 30 ] |> Expect.equal [ "-45", "0", "+30" ]
            , test "a_positive_amount_adds_a_negative_subtracts" <|
                \_ -> List.map (Routes.amount >> Routes.sign) [ -1, 0, 1 ] |> Expect.equal [ Subtracts, Adds, Adds ]
            , test "an_amount_rides_a_knob_from_its_middle" <|
                \_ -> List.map (Routes.amountFromNorm >> Routes.amountValue) [ 0, 0.5, 0.65, 1 ] |> Expect.equal [ -100, 0, 30, 100 ]
            , test "set_amount_moves_only_its_cable" <|
                \_ ->
                    routes [ 30, -45 ]
                        |> Routes.setAmount (addr "RT.A30") (Routes.amount 70)
                        |> Routes.cables
                        |> List.map (.amount >> Routes.amountValue)
                        |> Expect.equal [ 70, -45 ]
            , test "a_second_cable_between_the_same_jacks_replaces_the_first" <|
                \_ ->
                    routes [ 30 ]
                        |> Routes.add { from = env1, to = freq, amount = Routes.amount 5, addr = addr "RT.A5" }
                        |> Routes.cables
                        |> List.map (.amount >> Routes.amountValue)
                        |> Expect.equal [ 5 ]
            , test "unpatch_removes_the_cable" <|
                \_ -> routes [ 30, -45 ] |> Routes.remove env1 freq |> Routes.cables |> List.length |> Expect.equal 1
            ]
        , describe "PATCH"
            [ test "every_route_is_a_cable_with_an_attenuverter" <|
                \_ ->
                    PatchBay.view bay (routes [ 30, -45 ])
                        |> Query.fromHtml
                        |> Expect.all
                            [ Query.findAll [ Sel.class "att" ] >> Query.count (Expect.equal 2)
                            , Query.findAll [ Sel.tag "g" ] >> Query.count (Expect.equal 2)
                            , Query.find [ Sel.class "att", Sel.class "adds" ] >> Query.has [ Sel.text "+30" ]
                            , Query.find [ Sel.class "att", Sel.class "subtracts" ] >> Query.has [ Sel.text "-45" ]
                            ]
            , test "an_attenuverter_drags_its_amount" <|
                \_ ->
                    PatchBay.view bay (routes [ 30 ])
                        |> Query.fromHtml
                        |> Query.find [ Sel.class "att" ]
                        |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 5 ) ]))
                        |> Event.expect (Hand (Control.Grab (addr "RT.A30") Attenuverter 5))
            , test "a_double_click_on_an_attenuverter_unpatches" <|
                \_ ->
                    PatchBay.view bay (routes [ 30 ])
                        |> Query.fromHtml
                        |> Query.find [ Sel.class "att" ]
                        |> Event.simulate Event.doubleClick
                        |> Event.expect (Unpatch env1 freq)
            , test "a_drag_from_an_output_to_an_input_patches" <|
                \_ ->
                    Expect.all
                        [ \_ ->
                            PatchBay.view bay (routes [])
                                |> Query.fromHtml
                                |> Query.find [ Sel.class "out", Sel.containing [ Sel.text "VELO" ] ]
                                |> Event.simulate Event.mouseDown
                                |> Event.expect (Begin velo)
                        , \_ ->
                            PatchBay.update (Drop q) (PatchBay.update (Begin velo) bay |> Tuple.first)
                                |> Tuple.second
                                |> Expect.equal [ Patched velo q ]
                        , \_ ->
                            PatchBay.update (Drop q) bay |> Tuple.second |> Expect.equal []
                        ]
                        ()
            , test "used_jacks_are_lit" <|
                \_ ->
                    PatchBay.view bay (routes [ 30 ])
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.class "jack", Sel.class "on" ]
                        |> Query.count (Expect.equal 2)
            , test "the_toggle_shows_patch_or_matrix" <|
                \_ ->
                    Expect.all
                        [ \_ -> PatchBay.view bay (routes []) |> Query.fromHtml |> Query.find [ Sel.tag "button", Sel.containing [ Sel.text "MATRIX" ] ] |> Event.simulate Event.click |> Event.expect (ShowAs MatrixView)
                        , \_ -> PatchBay.view (PatchBay.update (ShowAs MatrixView) bay |> Tuple.first) (routes [ 30 ]) |> Query.fromHtml |> Query.has [ Sel.class "mx" ]
                        , \_ -> PatchBay.view bay (routes []) |> Query.fromHtml |> Query.find [ Sel.attribute (attribute "aria-pressed" "true") ] |> Query.has [ Sel.text "PATCH" ]
                        ]
                        ()
            , test "an_attenuverter_keeps_its_width_at_every_amount" <|
                \_ ->
                    Expect.all
                        (List.map
                            (\a _ ->
                                PatchBay.view bay (routes [ a ])
                                    |> Query.fromHtml
                                    |> Query.find [ Sel.class "att" ]
                                    |> Query.has [ Sel.style "width" (Control.widthText PatchBay.attenuverterWidth) ]
                            )
                            [ -100, -5, 0, 7, 100 ]
                        )
                        ()
            ]
        , describe "MATRIX"
            [ test "a_cell_per_source_and_destination" <|
                \_ ->
                    MatrixGrid.view { connect = Connect, hand = Hand } (routes [ 30, -45 ])
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.tag "td" ]
                        |> Query.count (Expect.equal 9)
            , test "a_patched_cell_prints_its_signed_amount_in_its_colour" <|
                \_ ->
                    MatrixGrid.view { connect = Connect, hand = Hand } (routes [ 30, -45 ])
                        |> Query.fromHtml
                        |> Expect.all
                            [ Query.find [ Sel.tag "td", Sel.class "adds" ] >> Query.has [ Sel.text "+30" ]
                            , Query.find [ Sel.tag "td", Sel.class "subtracts" ] >> Query.has [ Sel.text "-45" ]
                            ]
            , test "a_patched_cell_drags_its_amount" <|
                \_ ->
                    MatrixGrid.view { connect = Connect, hand = Hand } (routes [ 30 ])
                        |> Query.fromHtml
                        |> Query.find [ Sel.tag "td", Sel.class "adds" ]
                        |> Event.simulate (Event.custom "mousedown" (Encode.object [ ( "clientY", Encode.float 5 ) ]))
                        |> Event.expect (Hand (Control.Grab (addr "RT.A30") Attenuverter 5))
            , test "an_empty_cell_patches" <|
                \_ ->
                    MatrixGrid.view { connect = Connect, hand = Hand } (routes [])
                        |> Query.fromHtml
                        |> Query.findAll [ Sel.tag "td" ]
                        |> Query.first
                        |> Event.simulate Event.click
                        |> Event.expect (Connect env1 freq)
            , test "connect_says_patched" <|
                \_ -> PatchBay.update (Connect env1 q) bay |> Tuple.second |> Expect.equal [ Patched env1 q ]
            ]
        ]
