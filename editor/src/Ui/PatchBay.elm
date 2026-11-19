module Ui.PatchBay exposing (Bay, Effect(..), Msg(..), View(..), attenuverterWidth, init, subscriptions, update, view)

{-| The mod matrix as a patch bay: sources as jacks on the left,
destinations on the right, a cable per route with an attenuverter at its
middle (amber adds, blue subtracts), and the PATCH | MATRIX toggle.
-}

import Browser.Events
import Html exposing (Html, b, button, div, span, text)
import Html.Attributes as Attr exposing (attribute, class, classList, style)
import Html.Events as Events exposing (onClick)
import Json.Decode as Decode
import Svg
import Svg.Attributes as S
import Ui.Control as Control exposing (Travel(..), Width)
import Ui.MatrixGrid as MatrixGrid
import Ui.Routes as Routes exposing (Cable, Dest, Routes, Sign(..), Source)
import Ui.Tokens as Tokens exposing (Chars)


type View
    = PatchView
    | MatrixView


{-| The bay's own state: which view, and the output a hand is patching from.
-}
type Bay
    = Bay { view : View, patching : Maybe Source }


init : Bay
init =
    Bay { view = PatchView, patching = Nothing }


type Msg
    = ShowAs View
    | Begin Source
    | Drop Dest
    | Cancel
    | Connect Source Dest
    | Unpatch Source Dest
    | Hand Control.Msg


{-| What the page does with a hand on the bay.
-}
type Effect
    = Patched Source Dest
    | Unpatched Source Dest
    | Handed Control.Msg


update : Msg -> Bay -> ( Bay, List Effect )
update msg (Bay b) =
    case msg of
        ShowAs v ->
            ( Bay { b | view = v }, [] )

        Begin s ->
            ( Bay { b | patching = Just s }, [] )

        Drop d ->
            ( Bay { b | patching = Nothing }
            , case b.patching of
                Just s ->
                    [ Patched s d ]

                Nothing ->
                    []
            )

        Cancel ->
            ( Bay { b | patching = Nothing }, [] )

        Connect s d ->
            ( Bay b, [ Patched s d ] )

        Unpatch s d ->
            ( Bay b, [ Unpatched s d ] )

        Hand m ->
            ( Bay b, [ Handed m ] )


{-| A patch let go anywhere but an input is no patch.
-}
subscriptions : Bay -> Sub Msg
subscriptions (Bay b) =
    case b.patching of
        Just _ ->
            Browser.Events.onMouseUp (Decode.succeed Cancel)

        Nothing ->
            Sub.none


{-| An amount's readout: `+100` and a sign's room.
-}
attenuverterChars : Chars
attenuverterChars =
    Tokens.oneMore Tokens.readoutChars


{-| The attenuverter's box, the same at every amount.
-}
attenuverterWidth : Width
attenuverterWidth =
    Control.width { px = 78, ch = attenuverterChars }


pitch : Float
pitch =
    32


view : Bay -> Routes -> Html Msg
view (Bay bay) routes =
    div [ class "bay" ]
        [ div [ class "bayhead" ]
            [ div [ class "words" ]
                (List.map
                    (\( v, l ) ->
                        button [ classList [ ( "on", v == bay.view ) ], attribute "aria-pressed" (ariaBool (v == bay.view)), onClick (ShowAs v) ] [ text l ]
                    )
                    [ ( PatchView, "PATCH" ), ( MatrixView, "MATRIX" ) ]
                )
            ]
        , case bay.view of
            PatchView ->
                patch bay.patching routes

            MatrixView ->
                MatrixGrid.view { connect = Connect, hand = Hand } routes
        , div [ class "baynote" ]
            [ text "Drag from an output to an input to patch. Drag a knob up or down for its amount, double-click it to unpatch. Cable colour is the sign: "
            , b [] [ text "amber" ]
            , text " adds, "
            , Html.i [] [ text "blue" ]
            , text " subtracts."
            ]
        ]


type alias Wire =
    { cable : Cable, y1 : Float, y2 : Float, t : Float }


patch : Maybe Source -> Routes -> Html Msg
patch patching routes =
    let
        srcs : List Source
        srcs =
            Routes.sources routes

        dsts : List Dest
        dsts =
            Routes.dests routes

        height : Float
        height =
            pitch * toFloat (max (List.length srcs) (List.length dsts))

        rowOf : a -> List a -> Maybe Float
        rowOf x xs =
            List.indexedMap Tuple.pair xs
                |> List.filter (\( _, y ) -> y == x)
                |> List.head
                |> Maybe.map (\( i, _ ) -> toFloat i * pitch + 15)

        ordered : List Cable
        ordered =
            Routes.cables routes
                |> List.sortBy (\c -> ( rowOf c.to dsts |> Maybe.withDefault 0, rowOf c.from srcs |> Maybe.withDefault 0 ))

        n : Int
        n =
            List.length ordered

        wires : List Wire
        wires =
            List.indexedMap
                (\k c ->
                    Maybe.map2 (\y1 y2 -> { cable = c, y1 = y1, y2 = y2, t = spread n k }) (rowOf c.from srcs) (rowOf c.to dsts)
                )
                ordered
                |> List.filterMap identity

        lit : List Source
        lit =
            List.map (.cable >> .from) wires

        litDst : List Dest
        litDst =
            List.map (.cable >> .to) wires

        px : Float -> String
        px v =
            String.fromFloat v ++ "px"
    in
    div [ class "pb" ]
        [ div [ class "jcol srcs" ]
            (List.map
                (\src ->
                    div
                        [ classList [ ( "jack", True ), ( "out", True ), ( "on", List.member src lit ), ( "hot", patching == Just src ) ]
                        , Events.preventDefaultOn "mousedown" (Decode.succeed ( Begin src, True ))
                        ]
                        [ span [ class "hole" ] [], b [] [ text (Routes.sourceName src) ] ]
                )
                srcs
            )
        , div [ class "wires", style "height" (px height) ]
            (Svg.svg [ S.viewBox ("0 0 100 " ++ String.fromFloat height), S.preserveAspectRatio "none" ]
                (List.map cable wires)
                :: List.map attenuverter (unstack (List.map (\w -> ( w, midpoint w )) wires))
            )
        , div [ class "jcol dsts" ]
            (List.map
                (\dst ->
                    div
                        [ classList [ ( "jack", True ), ( "in", True ), ( "on", List.member dst litDst ) ]
                        , Events.onMouseUp (Drop dst)
                        ]
                        [ b [] [ text (Routes.destName dst) ], span [ class "hole" ] [] ]
                )
                dsts
            )
        ]


{-| Where along its length a cable carries its knob: spread so the knobs
of many cables don't sit on each other.
-}
spread : Int -> Int -> Float
spread n k =
    if n < 2 then
        0.5

    else
        0.3 + 0.4 * toFloat k / toFloat (n - 1)


type alias Curve =
    { x1 : Float, y1 : Float, c1x : Float, c1y : Float, c2x : Float, c2y : Float, x2 : Float, y2 : Float }


curve : Wire -> Curve
curve w =
    let
        sag : Float
        sag =
            18 + abs (w.y2 - w.y1) * 0.12
    in
    { x1 = 0, y1 = w.y1, c1x = 38, c1y = w.y1 + sag, c2x = 62, c2y = w.y2 + sag, x2 = 100, y2 = w.y2 }


midpoint : Wire -> ( Float, Float )
midpoint w =
    let
        c : Curve
        c =
            curve w

        u : Float
        u =
            1 - w.t

        bez : Float -> Float -> Float -> Float -> Float
        bez p0 p1 p2 p3 =
            u * u * u * p0 + 3 * u * u * w.t * p1 + 3 * u * w.t * w.t * p2 + w.t * w.t * w.t * p3
    in
    ( bez c.x1 c.c1x c.c2x c.x2, bez c.y1 c.c1y c.c2y c.y2 )


{-| Knobs closer than a knob's height on nearly the same line move down.
-}
unstack : List ( Wire, ( Float, Float ) ) -> List ( Wire, ( Float, Float ) )
unstack =
    List.sortBy (\( _, ( _, y ) ) -> y)
        >> List.foldl
            (\( w, ( x, y ) ) placed ->
                let
                    y1 : Float
                    y1 =
                        List.foldl
                            (\( _, ( px, py ) ) acc ->
                                if abs (px - x) < 14 && acc - py < 34 then
                                    py + 34

                                else
                                    acc
                            )
                            y
                            placed
                in
                placed ++ [ ( w, ( x, y1 ) ) ]
            )
            []


signClass : Cable -> String
signClass c =
    case Routes.sign c.amount of
        Adds ->
            "adds"

        Subtracts ->
            "subtracts"


cable : Wire -> Svg.Svg msg
cable w =
    let
        c : Curve
        c =
            curve w

        n : Float -> String
        n =
            String.fromFloat

        d : String
        d =
            "M" ++ n c.x1 ++ " " ++ n c.y1 ++ "C" ++ n c.c1x ++ " " ++ n c.c1y ++ " " ++ n c.c2x ++ " " ++ n c.c2y ++ " " ++ n c.x2 ++ " " ++ n c.y2
    in
    Svg.g [ S.class ("cable " ++ signClass w.cable) ]
        [ Svg.path [ S.class "ws", S.d d ] []
        , Svg.path [ S.class "w", S.d d, S.strokeOpacity (n (0.45 + 0.55 * toFloat (abs (Routes.amountValue w.cable.amount)) / 100)) ] []
        ]


attenuverter : ( Wire, ( Float, Float ) ) -> Html Msg
attenuverter ( w, ( x, y ) ) =
    let
        c : Cable
        c =
            w.cable

        f : Float
        f =
            toFloat (abs (Routes.amountValue c.amount)) / 100

        ring : Float
        ring =
            2 * pi * 7
    in
    div
        ([ class ("att " ++ signClass c)
         , Control.widthStyle attenuverterWidth
         , style "left" (String.fromFloat x ++ "%")
         , style "top" (String.fromFloat y ++ "px")
         , Control.ariaRole "slider"
         , Control.ariaLabel (sourceName c.from ++ " to " ++ destName c.to)
         , Control.ariaValueText (Routes.amountText c.amount)
         , Events.onDoubleClick (Unpatch c.from c.to)
         ]
            ++ List.map (Attr.map Hand) (Control.drag c.addr Attenuverter)
        )
        [ Svg.svg [ S.width "20", S.height "20", S.viewBox "0 0 20 20" ]
            [ Svg.circle [ S.class "track", S.cx "10", S.cy "10", S.r "7", S.fill "none", S.strokeWidth "3" ] []
            , Svg.circle
                [ S.class "amt"
                , S.cx "10"
                , S.cy "10"
                , S.r "7"
                , S.fill "none"
                , S.strokeWidth "3"
                , S.strokeDasharray (String.fromFloat (f * ring) ++ " " ++ String.fromFloat ring)
                , S.transform "rotate(-90 10 10)"
                ]
                []
            ]
        , span [ class "v", style "width" "4ch" ] [ text (Routes.amountText c.amount) ]
        ]


sourceName : Source -> String
sourceName =
    Routes.sourceName


destName : Dest -> String
destName =
    Routes.destName


ariaBool : Bool -> String
ariaBool on =
    if on then
        "true"

    else
        "false"
