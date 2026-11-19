module View.Algo exposing (Drag, Msg(..), State, init, subscriptions, update, view)

{-| The ALGO panel: every parameter the unit describes, on one face. The
face shows only what the unit last reported; a hand turns a control by
asking for edits, at most one `ed set` a frame for each.
-}

import Browser.Events
import Dict exposing (Dict)
import Html exposing (Html, div, fieldset, legend, span, text)
import Html.Attributes exposing (class, style)
import Json.Decode as Decode
import Layout.Algo as Layout exposing (Cell, Control(..), Item)
import Model exposing (Model)
import Protocol exposing (Addr, Value(..), addrKey)
import Schema exposing (Param)
import Ui.Control as Control exposing (Want(..))
import View.Controls as Controls


{-| A control being dragged: where the pointer went down, and the position
(0 to 1, or an option) the drag started from.
-}
type alias Drag =
    { addr : Addr
    , param : Param
    , control : Control
    , startY : Float
    , from : Float
    }


{-| `at` is where a hand last put each control (a position, or an option),
shown until the unit's own report or the pending mark's end replaces it.
-}
type alias State =
    { drag : Maybe Drag
    , wanted : Control.Wanted
    , at : Dict String Float
    }


type Msg
    = Hand Controls.Msg
    | Moved Float
    | Released
    | Frame


init : State
init =
    { drag = Nothing, wanted = Control.empty, at = Dict.empty }


subscriptions : State -> Sub Msg
subscriptions s =
    Sub.batch
        [ if Control.isEmpty s.wanted then
            Sub.none

          else
            Browser.Events.onAnimationFrame (\_ -> Frame)
        , case s.drag of
            Just _ ->
                Sub.batch
                    [ Browser.Events.onMouseMove (Decode.map Moved (Decode.field "clientY" Decode.float))
                    , Browser.Events.onMouseUp (Decode.succeed Released)
                    ]

            Nothing ->
                Sub.none
        ]



-- WHAT A HAND DOES


isOptions : Param -> Bool
isOptions p =
    p.kind /= Protocol.Cont


optionCount : Param -> Int
optionCount p =
    round (p.max - p.min) + 1


{-| Where a control is now: held where a hand put it while an edit waits,
else the unit's. A position for a knob, the option for a choice.
-}
position : Model -> State -> Param -> Float
position m s p =
    let
        held : Maybe Float
        held =
            if (s.drag |> Maybe.map (\d -> d.addr == p.addr) |> Maybe.withDefault False) || Model.isPending p.addr m || Control.wants p.addr s.wanted then
                Dict.get (addrKey p.addr) s.at

            else
                Nothing
    in
    case held of
        Just h ->
            h

        Nothing ->
            Model.shown p.addr m
                |> Maybe.map
                    (\r ->
                        if isOptions p then
                            r.stored

                        else
                            r.norm
                    )
                |> Maybe.withDefault 0


travel : Control -> Float
travel c =
    case c of
        Hero ->
            240

        Slider ->
            60

        Knob ->
            150

        Choice ->
            150

        Toggle ->
            150


{-| The messages for the model, which are the edits a frame sends: `update`
only ever says them at `Frame`.
-}
update : Msg -> Model -> State -> ( State, List Model.Msg )
update msg m s =
    case msg of
        Hand (Controls.Grab a control y) ->
            case Schema.paramIn m.variants a m.schema of
                Just p ->
                    ( { s | drag = Just { addr = a, param = p, control = control, startY = y, from = position m s p } }, [] )

                Nothing ->
                    ( s, [] )

        Hand (Controls.Wheel a dy) ->
            case ( dy == 0, Schema.paramIn m.variants a m.schema ) of
                ( False, Just p ) ->
                    let
                        dir : Int
                        dir =
                            if dy < 0 then
                                1

                            else
                                -1

                        here : Float
                        here =
                            position m s p
                    in
                    ( if isOptions p then
                        setOption p (clamp 0 (optionCount p - 1) (round here + dir)) s

                      else
                        setNorm p (here + toFloat dir * Control.wheelNorm p) s
                    , []
                    )

                _ ->
                    ( s, [] )

        Hand (Controls.Pick a i) ->
            case Schema.paramIn m.variants a m.schema of
                Just p ->
                    ( setOption p i s, [] )

                Nothing ->
                    ( s, [] )

        Moved y ->
            case s.drag of
                Just d ->
                    if isOptions d.param then
                        ( setOption d.param (Control.dragIndex { n = optionCount d.param, from = round d.from } d.startY y) s, [] )

                    else
                        ( setNorm d.param (Control.dragNorm { px = travel d.control, from = d.from } d.startY y) s, [] )

                Nothing ->
                    ( s, [] )

        Released ->
            ( { s | drag = Nothing }, [] )

        Frame ->
            let
                ( wants, rest ) =
                    Control.flush s.wanted
            in
            ( { s | wanted = rest }
            , List.filterMap
                (\( a, w ) ->
                    case w of
                        Number v ->
                            Just (Model.Edit a (Num v))

                        Option i ->
                            Protocol.optIndex i |> Maybe.map (Model.Pick a)
                )
                wants
            )


setNorm : Param -> Float -> State -> State
setNorm p t s =
    let
        t1 : Float
        t1 =
            clamp 0 1 t
    in
    { s | wanted = Control.want p.addr (Number (Control.atNorm p t1)) s.wanted, at = Dict.insert (addrKey p.addr) t1 s.at }


setOption : Param -> Int -> State -> State
setOption p i s =
    { s | wanted = Control.want p.addr (Option i) s.wanted, at = Dict.insert (addrKey p.addr) (toFloat i) s.at }



-- THE FACE


view : Model -> State -> Html Msg
view m s =
    let
        all : List Item
        all =
            Layout.items m.schema m.variants
    in
    if List.isEmpty all then
        div [ class "waiting" ] [ text "WAITING FOR THE UNIT" ]

    else
        let
            routes : List String
            routes =
                Layout.routes m.schema m.variants
        in
        div []
            [ div [ class "face" ] (List.map (section m s all routes) Layout.sections)
            , unplacedStrip m s (Layout.unplaced all)
            ]


section : Model -> State -> List Item -> List String -> Layout.SectionBox -> Html Msg
section m s all routes sb =
    let
        mine : List ( Item, Layout.Placed )
        mine =
            all
                |> List.filterMap
                    (\i ->
                        i.placed
                            |> Maybe.andThen
                                (\p ->
                                    if p.section == sb.section then
                                        Just ( i, p )

                                    else
                                        Nothing
                                )
                    )

        screens : List Layout.Screen
        screens =
            Layout.screens |> List.filter (\x -> x.section == sb.section)

        cols : Int
        cols =
            sb.box.w

        rows : Int
        rows =
            sb.box.h
    in
    fieldset
        ([ class "sect"
         , style "grid-column" (String.fromInt sb.box.col ++ " / span " ++ String.fromInt cols)
         , style "grid-row" (String.fromInt sb.box.row ++ " / span " ++ String.fromInt rows)
         , style "grid-template-columns" ("repeat(" ++ String.fromInt cols ++ ", calc(var(--u) - " ++ String.fromFloat (6 / toFloat cols) ++ "px))")
         , style "grid-template-rows" ("repeat(" ++ String.fromInt rows ++ ", calc(var(--uh) - " ++ String.fromFloat (22 / toFloat rows) ++ "px))")
         ]
            ++ (if cols == 1 then
                    [ class "n1" ]

                else
                    []
               )
        )
        (legend [] [ text sb.title ]
            :: List.map screen screens
            ++ List.map (\( i, p ) -> slot p.cell (cellFor m s i p.control)) mine
            ++ (if sb.section == Layout.FilterMod then
                    [ slot (Cell 1 2 3 1) (modNote routes) ]

                else
                    []
               )
        )


slot : Cell -> Html msg -> Html msg
slot c inner =
    div
        [ class "slot"
        , style "grid-column" (String.fromInt c.col ++ " / span " ++ String.fromInt c.w)
        , style "grid-row" (String.fromInt c.row ++ " / span " ++ String.fromInt c.h)
        ]
        [ inner ]


screen : Layout.Screen -> Html msg
screen x =
    slot x.cell (div [ class "cell lcdcell" ] [ div [ class "lcd" ] [], span [ class "lab" ] [ text x.title ] ])


{-| Filter Mod's matrix routes are the next slice's; the unit names them.
-}
modNote : List String -> Html msg
modNote routes =
    div [ class "note" ]
        [ span [ class "lab" ] [ text "MOD: NEXT SLICE" ]
        , span [ class "val" ] [ text (String.join " / " routes) ]
        ]


face : Model -> State -> Item -> Controls.Face
face m s i =
    let
        r : Maybe Model.Reading
        r =
            Model.shown i.param.addr m

        pos : Float
        pos =
            position m s i.param
    in
    { addr = i.param.addr
    , label = i.label
    , text = r |> Maybe.map .text |> Maybe.withDefault ""
    , pos =
        if isOptions i.param then
            pos / toFloat (max 1 (optionCount i.param - 1))

        else
            pos
    , pending = Model.isPending i.param.addr m
    , options = List.indexedMap (\n ( _, name ) -> ( n, name )) i.param.options
    , current = round pos
    }


cellFor : Model -> State -> Item -> Control -> Html Msg
cellFor m s i c =
    Controls.view c (face m s i) |> Html.map Hand


{-| What the unit describes and the layout doesn't place: never hidden.
-}
unplacedStrip : Model -> State -> List Item -> Html Msg
unplacedStrip m s xs =
    if List.isEmpty xs then
        text ""

    else
        fieldset [ class "unplaced" ]
            (legend [] [ text "UNPLACED" ]
                :: List.map
                    (\i ->
                        cellFor m
                            s
                            i
                            (case i.param.kind of
                                Protocol.Cont ->
                                    Knob

                                Protocol.Choice ->
                                    Choice

                                Protocol.Toggle ->
                                    Toggle
                            )
                    )
                    xs
            )
