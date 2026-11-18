module View.Controls exposing (Face, Msg(..), view)

{-| One control of the face: label, a knob / slider / lit list / lever, and
the unit's own value text. Nothing here knows a value's maths: a control is
handed its position (0 to 1, or an option index) and its text.
-}

import Html exposing (Html, button, div, span, text)
import Html.Attributes exposing (attribute, class, classList, style, tabindex)
import Html.Events as Events
import Json.Decode as Decode exposing (Decoder)
import Layout.Algo exposing (Control(..))
import Protocol exposing (Addr)


{-| Everything a control shows. `pos` is where the knob points; `current` the
chosen option; `text` is the unit's.
-}
type alias Face =
    { addr : Addr
    , label : String
    , text : String
    , pos : Float
    , pending : Bool
    , options : List ( Int, String )
    , current : Int
    }


type Msg
    = Grab Addr Control Float
    | Wheel Addr Control Float
    | Pick Addr Int


view : Control -> Face -> Html Msg
view control f =
    div [ class "cell", classList [ ( "pending", f.pending ), ( "big", control == Hero ) ] ]
        (case control of
            Knob ->
                knob control f

            Hero ->
                knob control f

            Slider ->
                [ div (class "slider" :: handle control f) [ div [ class "cap", style "bottom" (px (f.pos * 25)) ] [] ], readout f, label f ]

            Choice ->
                if List.length f.options <= 5 && not (List.isEmpty f.options) then
                    [ div [ class "sw" ] (List.map (lit f) f.options), label f ]

                else
                    knob control f

            Toggle ->
                [ lever f, label f ]
        )


knob : Control -> Face -> List (Html Msg)
knob control f =
    [ div (class "knob" :: handle control f)
        [ div [ class "ind", style "transform" ("rotate(" ++ String.fromFloat (-135 + 270 * f.pos) ++ "deg)") ] [] ]
    , readout f
    , label f
    ]


{-| What hands do: mouse down starts a drag, the wheel and arrow keys step.
-}
handle : Control -> Face -> List (Html.Attribute Msg)
handle control f =
    [ tabindex 0
    , attribute "role" "slider"
    , attribute "aria-label" f.label
    , attribute "aria-valuetext" f.text
    , Events.preventDefaultOn "mousedown" (Decode.map (\y -> ( Grab f.addr control y, True )) (Decode.field "clientY" Decode.float))
    , Events.preventDefaultOn "wheel" (Decode.map (\dy -> ( Wheel f.addr control dy, True )) (Decode.field "deltaY" Decode.float))
    , Events.preventDefaultOn "keydown" (Decode.andThen (arrow f.addr control) (Decode.field "key" Decode.string))
    ]


arrow : Addr -> Control -> String -> Decoder ( Msg, Bool )
arrow addr control key =
    case key of
        "ArrowUp" ->
            Decode.succeed ( Wheel addr control -1, True )

        "ArrowDown" ->
            Decode.succeed ( Wheel addr control 1, True )

        _ ->
            Decode.fail "not an arrow"


readout : Face -> Html msg
readout f =
    span [ class "val" ]
        [ text
            (if f.text == "" then
                "--"

             else
                f.text
            )
        ]


label : Face -> Html msg
label f =
    span [ class "lab" ] [ text f.label ]


lit : Face -> ( Int, String ) -> Html Msg
lit f ( i, name ) =
    button [ class "opt", classList [ ( "on", i == f.current ) ], Events.onClick (Pick f.addr i) ]
        [ Html.i [] [], text name ]


{-| A lever between printed positions; a click goes to the next.
-}
lever : Face -> Html Msg
lever f =
    let
        n =
            List.length f.options

        next =
            if n == 0 then
                f.current

            else
                modBy n (f.current + 1)

        h =
            toFloat (max 2 n) * 13

        top =
            1 + toFloat f.current * (h - 13) / toFloat (max 1 (n - 1))
    in
    button [ class "tog", Events.onClick (Pick f.addr next), attribute "aria-label" f.label ]
        [ div [ class "pos" ] (List.map (\( i, name ) -> span [ classList [ ( "on", i == f.current ) ] ] [ text name ]) f.options)
        , div [ class "body", style "height" (px h) ] [ div [ class "lever", style "top" (px top) ] [] ]
        ]


px : Float -> String
px v =
    String.fromFloat v ++ "px"
