module Ui.Row exposing (Row, members, pairUp, view)

{-| Modules in rows of two. Both modules of a row share its height (one CSS
grid row), so their borders line up; a full module (MOD) takes a row of its
own; folding either module of a pair folds both.
-}

import Html exposing (Html, div)
import Html.Attributes exposing (class, classList)
import Ui.Module as Module exposing (Fold(..), Folds)


type Row msg
    = Pair (Module.Config msg) (Module.Config msg)
    | Lone (Module.Config msg)
    | Whole (Module.Config msg)


{-| Signal order, two at a time: a full module ends the pair before it.
-}
pairUp : List (Module.Config msg) -> List (Row msg)
pairUp modules =
    case modules of
        [] ->
            []

        a :: rest ->
            if Module.isFull a then
                Whole a :: pairUp rest

            else
                case rest of
                    b :: more ->
                        if Module.isFull b then
                            Lone a :: pairUp rest

                        else
                            Pair a b :: pairUp more

                    [] ->
                        [ Lone a ]


members : Row msg -> List (Module.Config msg)
members row =
    case row of
        Pair a b ->
            [ a, b ]

        Lone a ->
            [ a ]

        Whole a ->
            [ a ]


view : (Module.Msg -> msg) -> Folds -> Row msg -> Html msg
view toMsg folds row =
    let
        ms : List (Module.Config msg)
        ms =
            members row

        fold : Fold
        fold =
            if List.any (\m -> Module.foldOf (Module.idOf m) folds == Folded) ms then
                Folded

            else
                Open

        toggle : msg
        toggle =
            toMsg
                (Module.SetFold (List.map Module.idOf ms)
                    (case fold of
                        Folded ->
                            Open

                        Open ->
                            Folded
                    )
                )

        one : Module.Config msg -> Html msg
        one =
            Module.view { fold = fold, toggle = toggle }
    in
    case row of
        Pair a b ->
            div [ class "prow" ] [ one a, one b ]

        Lone a ->
            div [ class "prow" ] [ one a, div [ class "blank" ] [] ]

        Whole a ->
            div [ classList [ ( "prow", True ), ( "one", True ) ] ] [ one a ]
