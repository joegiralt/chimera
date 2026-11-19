module Ui.PadStrip exposing (Config, Msg(..), Tab(..), icons, new, view)

{-| DRUM as on the box: a KIT tab, then PAD 1–12 as the unit's pad grid
(machine icon, number, name), Analog Rytm Overbridge-style.
-}

import Html exposing (Html, b, button, div, span, text)
import Html.Attributes exposing (attribute, class, classList)
import Html.Events exposing (onClick)
import Ui.Pad as Pad exposing (Pad)
import Ui.Screen as Screen exposing (Glyph(..))
import Unit exposing (Engine(..))


type Tab
    = Kit
    | PadTab Pad


type Msg
    = Select Tab


type Config
    = Config { kit : String, name : Pad -> String, icon : Pad -> Screen.Config }


{-| The kit's name, and each pad's name as the unit gives it.
-}
new : String -> (Pad -> String) -> Config
new kit name =
    Config { kit = kit, name = name, icon = \p -> Screen.new Drum (PadIcon p) }


{-| Each pad's icon: the unit's own for its machine (`Screen.padIcon`).
-}
icons : (Pad -> Screen.Config) -> Config -> Config
icons f (Config c) =
    Config { c | icon = f }


view : Config -> Tab -> Html Msg
view (Config c) shown =
    div [ class "padstrip", attribute "role" "tablist", attribute "aria-label" "pads" ]
        (tab (shown == Kit)
            Kit
            [ class "kit" ]
            [ b [] [ text "KIT" ], span [] [ text c.kit ] ]
            :: List.map
                (\p ->
                    tab (shown == PadTab p)
                        (PadTab p)
                        []
                        [ Screen.view (c.icon p)
                        , span [] [ b [] [ text (Pad.label p) ], text (c.name p) ]
                        ]
                )
                Pad.all
        )


tab : Bool -> Tab -> List (Html.Attribute Msg) -> List (Html Msg) -> Html Msg
tab on t attrs =
    button
        ([ classList [ ( "pst", True ), ( "on", on ) ]
         , attribute "role" "tab"
         , attribute "aria-selected"
            (if on then
                "true"

             else
                "false"
            )
         , onClick (Select t)
         ]
            ++ attrs
        )
