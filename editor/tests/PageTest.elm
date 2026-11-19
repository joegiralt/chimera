module PageTest exposing (suite)

import Expect
import Html exposing (text)
import Html.Attributes exposing (attribute)
import Test exposing (Test, describe, test)
import Test.Html.Event as Event
import Test.Html.Query as Query
import Test.Html.Selector as Sel
import Ui.Module as Module exposing (Fold(..))
import Ui.Page as Page exposing (Current(..), Effect(..), Link(..), Msg(..))
import Ui.Tokens exposing (Theme(..))


type Go
    = GoP1
    | GoP2
    | GoMixer
    | Page Page.Msg


m : String -> Module.Config Go
m t =
    Module.new t |> Module.body [ text (t ++ " body") ]


page : Page.Config Go
page =
    Page.new "GLASS BELLS"
        |> Page.meta "PART 2 · ALGO"
        |> Page.parts [ { kicker = "P1 · VA", name = "WARM SAW PAD", current = Elsewhere, go = GoP1 }, { kicker = "P2 · ALGO", name = "GLASS BELLS", current = Here, go = GoP2 } ]
        |> Page.units [ { kicker = "UNIT", name = "MIXER", current = Elsewhere, go = GoMixer } ]
        |> Page.link (Linked "PREENFM3 · 0.4.0 · LIVE")
        |> Page.modules [ m "FILTER", m "AMP", Module.chip "ENVELOPES" (m "ENV 1"), Module.chip "ENVELOPES" (m "ENV 2") ]


{-| What the page asked of its own update, if anything.
-}
pageMsg : Go -> Maybe Page.Msg
pageMsg go =
    case go of
        Page pm ->
            Just pm

        GoP1 ->
            Nothing

        GoP2 ->
            Nothing

        GoMixer ->
            Nothing


render : Page.State -> Query.Single Go
render s =
    Page.view Page s page |> Query.fromHtml


suite : Test
suite =
    describe "Ui.Page"
        [ test "the_page_wears_its_theme" <|
            \_ ->
                Expect.all
                    [ \_ -> render (Page.init Day) |> Query.has [ Sel.attribute (attribute "data-theme" "day"), Sel.class "ui" ]
                    , \_ -> render (Page.init Night) |> Query.has [ Sel.attribute (attribute "data-theme" "night") ]
                    ]
                    ()
        , test "the_switch_says_the_theme_and_the_update_stores_it" <|
            \_ ->
                Expect.all
                    [ \_ -> render (Page.init Day) |> Query.find [ Sel.class "themesw" ] |> Query.find [ Sel.containing [ Sel.text "NIGHT" ], Sel.tag "button" ] |> Event.simulate Event.click |> Event.toResult |> Result.map pageMsg |> Expect.equal (Ok (Just (ChoseTheme Night)))
                    , \_ -> Page.update (ChoseTheme Night) (Page.init Day) |> Tuple.mapFirst Page.theme |> Expect.equal ( Night, StoreTheme Night )
                    ]
                    ()
        , test "a_chip_hides_its_modules_and_shows_them_again" <|
            \_ ->
                let
                    hidden : Page.State
                    hidden =
                        Page.init Day |> Page.update (ToggleChip "ENVELOPES") |> Tuple.first
                in
                Expect.all
                    [ \_ -> render hidden |> Query.findAll [ Sel.class "sec" ] |> Query.count (Expect.equal 2)
                    , \_ -> render hidden |> Query.find [ Sel.class "chip", Sel.containing [ Sel.text "ENVELOPES" ] ] |> Query.has [ Sel.class "off", Sel.attribute (attribute "aria-pressed" "false") ]
                    , \_ -> render (hidden |> Page.update (ToggleChip "ENVELOPES") |> Tuple.first) |> Query.findAll [ Sel.class "sec" ] |> Query.count (Expect.equal 4)
                    ]
                    ()
        , test "one_chip_per_group_in_page_order" <|
            \_ ->
                render (Page.init Day)
                    |> Query.findAll [ Sel.class "chip", Sel.attribute (attribute "aria-pressed" "true") ]
                    |> Query.count (Expect.equal 3)
        , test "fold_all_folds_every_module" <|
            \_ ->
                render (Page.init Day)
                    |> Query.find [ Sel.class "chip", Sel.containing [ Sel.text "FOLD ALL" ] ]
                    |> Event.simulate Event.click
                    |> Event.expect (Page (Folding (Module.SetFold (List.map Module.idOf [ m "FILTER", m "AMP", m "ENV 1", m "ENV 2" ]) Folded)))
        , test "modules_sit_in_rows_of_two" <|
            \_ ->
                render (Page.init Day)
                    |> Query.findAll [ Sel.class "prow" ]
                    |> Query.count (Expect.equal 2)
        , test "the_current_tab_is_lit_and_a_tab_goes" <|
            \_ ->
                Expect.all
                    [ \_ -> render (Page.init Day) |> Query.find [ Sel.class "tab", Sel.class "on" ] |> Query.has [ Sel.text "GLASS BELLS" ]
                    , \_ -> render (Page.init Day) |> Query.find [ Sel.class "tab", Sel.containing [ Sel.text "MIXER" ] ] |> Event.simulate Event.click |> Event.expect GoMixer
                    ]
                    ()
        , test "the_link_is_printed" <|
            \_ ->
                render (Page.init Day) |> Query.has [ Sel.text "PREENFM3 · 0.4.0 · LIVE" ]
        ]
