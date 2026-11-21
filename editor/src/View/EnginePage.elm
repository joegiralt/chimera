module View.EnginePage exposing (BayId(..), Layout, Msg(..), State, follow, init, layout, other, screenSpec, subscriptions, update, view)

{-| An engine's page: the unit's schema, placed by the engine's layout,
read from `Model`, drawn with the Ui components. What the unit describes and
the layout doesn't place is shown in OTHER, never hidden.

A hand never changes a value here: it asks for edits, at most one `ed set`
a frame for each control. Every control shows the unit's stored value, and a
refusal with the unit's reason; nothing is guessed.

-}

import Addr exposing (Addr, PadBlock(..))
import Browser.Events
import Dict exposing (Dict)
import Html exposing (Html, button, div, p, text)
import Html.Attributes exposing (class)
import Html.Events exposing (onClick)
import Layout.Algo
import Layout.Drum
import Layout.Modal
import Layout.Page as UnitPage exposing (PadPage(..), UnitPage(..))
import Layout.Shared as Shared exposing (As(..), Extra(..), Item, Module, Placement(..))
import Layout.Tape
import Layout.Va
import Model exposing (Model, Seat)
import Protocol exposing (Kind(..), OptIndex, PageNo, Scale(..), Value(..))
import Schema exposing (Param)
import Set exposing (Set)
import Transport
import Ui.Chip as Chip
import Ui.Control as Control exposing (Chosen, Intent(..), Reading, Target(..), Want(..))
import Ui.Fader as Fader
import Ui.Knob as Knob
import Ui.Menu as Menu
import Ui.Module as Module
import Ui.Pad as Pad exposing (Pad)
import Ui.PadStrip as PadStrip exposing (Tab(..))
import Ui.Page as Page exposing (Current(..), Effect(..))
import Ui.PatchBay as PatchBay
import Ui.Routes as Routes exposing (Dest, Source)
import Ui.Screen as Screen
import Ui.Selector as Selector
import Ui.Toggle as Toggle
import Ui.Tokens exposing (Theme)
import Unit exposing (Engine(..))



-- STATE


{-| A patch bay on the page: the Part's mod matrix, or a DRUM pad's routes.
-}
type BayId
    = PartBay
    | PadBay Pad


type alias State =
    { page : Page.State
    , drag : Control.Drag
    , wanted : Control.Wanted
    , menu : Menu.State
    , bays : Dict String ( BayId, PatchBay.Bay )
    , tab : Tab
    , seat : Maybe Seat
    }


type Msg
    = Hand Control.Msg
    | MenuMsg Menu.Msg
    | PageMsg Page.Msg
    | BayMsg BayId PatchBay.Msg
    | PadMsg PadStrip.Msg
    | Frame
    | Unit Model.Msg


init : Theme -> State
init t =
    { page = Page.init t
    , drag = Control.idle
    , wanted = Control.empty
    , menu = Menu.shut
    , bays = Dict.empty
    , tab = Kit
    , seat = Nothing
    }


subscriptions : State -> Sub Msg
subscriptions s =
    Sub.batch
        [ Sub.map Hand (Control.subscriptions s.drag)
        , if Control.isEmpty s.wanted then
            Sub.none

          else
            Browser.Events.onAnimationFrame (\_ -> Frame)
        , Sub.batch (List.map (\( id, bay ) -> Sub.map (BayMsg id) (PatchBay.subscriptions bay)) (Dict.values s.bays))
        ]


{-| The page follows the box: when the box takes another Part (a new
seat), a hand on a control lets go and its unsent wants are dropped, so
nothing meant for the old Part reaches the new one.
-}
follow : Model -> State -> State
follow m s =
    if s.seat == Just (Model.seat m) then
        s

    else
        { s | drag = Control.idle, wanted = Control.empty, menu = Menu.shut, seat = Just (Model.seat m) }



-- THE LAYOUT


{-| The modules a view shows, and every module of every view (an address
placed in any of them is not OTHER's).
-}
type alias Layout =
    { shown : List Module, every : List Module }


layout : Engine -> Tab -> Layout
layout e tab =
    let
        one : List Module -> Layout
        one ms =
            { shown = ms, every = ms }
    in
    case e of
        Algo ->
            one Layout.Algo.modules

        Va ->
            one Layout.Va.modules

        Modal ->
            one Layout.Modal.modules

        Tape ->
            one Layout.Tape.modules

        Drum ->
            { shown =
                case tab of
                    Kit ->
                        Layout.Drum.kit

                    PadTab p ->
                        Layout.Drum.pad p
            , every = Layout.Drum.every
            }


{-| The Sound's items no module of the layout places.
-}
other : List Item -> List Module -> List Item
other items every =
    let
        placed : Item -> Shared.Placement
        placed =
            Shared.place every
    in
    List.filter
        (\i ->
            case placed i of
                Placed _ ->
                    False

                Unplaced ->
                    True
        )
        items



-- WHAT A HAND DOES


update : Msg -> Model -> State -> ( State, List Model.Msg, Maybe Theme )
update msg m s0 =
    let
        s : State
        s =
            follow m s0
    in
    case msg of
        Hand h ->
            ( hand m h s, [], Nothing )

        MenuMsg mm ->
            let
                ( menu, picked ) =
                    Menu.update mm

                shut : State
                shut =
                    { s | menu = menu }
            in
            ( picked |> Maybe.map (\h -> hand m h shut) |> Maybe.withDefault shut, [], Nothing )

        PageMsg pm ->
            let
                ( page, effect ) =
                    Page.update pm s.page
            in
            ( { s | page = page }
            , []
            , case effect of
                StoreTheme t ->
                    Just t

                NoEffect ->
                    Nothing
            )

        BayMsg id bm ->
            let
                ( bay, effects ) =
                    PatchBay.update bm (bayOf id s)
            in
            ( List.foldl (bayEffect m id) { s | bays = Dict.insert (bayKey id) ( id, bay ) s.bays } effects, [], Nothing )

        PadMsg (PadStrip.Select t) ->
            ( { s | tab = t }, [], Nothing )

        Frame ->
            let
                ( wants, rest ) =
                    Control.flush s.wanted
            in
            ( { s | wanted = rest }
            , List.map
                (\( a, w ) ->
                    case w of
                        Number v ->
                            Model.Edit a (Num v)

                        Option i ->
                            Model.Pick a i
                )
                wants
            , Nothing
            )

        Unit um ->
            ( s, [ um ], Nothing )


bayKey : BayId -> String
bayKey id =
    case id of
        PartBay ->
            "MOD"

        PadBay p ->
            "PAD" ++ String.fromInt (Pad.number p)


bayOf : BayId -> State -> PatchBay.Bay
bayOf id s =
    Dict.get (bayKey id) s.bays |> Maybe.map Tuple.second |> Maybe.withDefault PatchBay.init


{-| A hand on a bay. The Part's has no routes yet, so only its own state
moves; a pad's patches its routes: a new cable takes the first route with
no amount, and unpatching sets the amount to nothing.
-}
bayEffect : Model -> BayId -> PatchBay.Effect -> State -> State
bayEffect m id effect s =
    case ( id, effect ) of
        ( _, PatchBay.Handed h ) ->
            hand m h s

        ( PartBay, PatchBay.Patched _ _ ) ->
            s

        ( PartBay, PatchBay.Unpatched _ _ ) ->
            s

        ( PadBay p, PatchBay.Patched from to ) ->
            let
                routes : List RouteNow
                routes =
                    routesNow m p
            in
            if List.any (\r -> r.on && r.from == Just from && r.to == Just to) routes then
                s

            else
                case List.filter (.on >> not) routes |> List.head of
                    Just r ->
                        s
                            |> want r.route.source (Option (Routes.sourceOption from))
                            |> want r.route.dest (Option (Routes.destOption to))
                            |> (\st -> paramOf m r.route.amount |> Maybe.map (\ap -> want ap.addr (Number (Control.atNorm ap newCable)) st) |> Maybe.withDefault st)

                    Nothing ->
                        s

        ( PadBay p, PatchBay.Unpatched from to ) ->
            routesNow m p
                |> List.filter (\r -> r.on && r.from == Just from && r.to == Just to)
                |> List.foldl (\r -> want r.route.amount (Number 0)) s


{-| Where a new cable's attenuverter starts: half way up.
-}
newCable : Float
newCable =
    0.75


{-| A pad route as the unit has it now: patched while its amount is not
nothing.
-}
type alias RouteNow =
    { route : Shared.Route, on : Bool, from : Maybe Source, to : Maybe Dest, amount : Maybe Param }


routesNow : Model -> Pad -> List RouteNow
routesNow m p =
    List.map
        (\r ->
            let
                amount : Maybe Param
                amount =
                    paramOf m r.amount

                option : Addr -> Maybe ( OptIndex, String )
                option a =
                    paramOf m a |> Maybe.andThen (\q -> Control.current (chosen m q) |> Maybe.andThen (\i -> q.options |> List.filter (\( k, _ ) -> k == i) |> List.head))
            in
            { route = r
            , on = Maybe.map2 (\v ap -> abs v.stored >= ap.step / 2) (Model.shown r.amount m) amount |> Maybe.withDefault False
            , from = option r.source |> Maybe.map Routes.source
            , to = option r.dest |> Maybe.map Routes.dest
            , amount = amount
            }
        )
        (Shared.padRoutes p)


padBay : Model -> Pad -> Routes.Routes
padBay m p =
    let
        routes : List RouteNow
        routes =
            routesNow m p

        options : (Shared.Route -> Addr) -> List ( OptIndex, String )
        options f =
            routes |> List.head |> Maybe.andThen (\r -> paramOf m (f r.route)) |> Maybe.map .options |> Maybe.withDefault []
    in
    routes
        |> List.filterMap
            (\r ->
                case ( r.on, ( r.from, r.to ), Maybe.andThen (\ap -> Model.shown ap.addr m) r.amount ) of
                    ( True, ( Just from, Just to ), Just v ) ->
                        Just { from = from, to = to, amount = Routes.amountFromNorm v.norm, addr = r.route.amount }

                    _ ->
                        Nothing
            )
        |> List.foldl Routes.add (Routes.new (List.map Routes.source (options .source)) (List.map Routes.dest (options .dest)))


hand : Model -> Control.Msg -> State -> State
hand m h s =
    let
        ( drag, intents ) =
            Control.update (target m) h s.drag
    in
    List.foldl (intend m) { s | drag = drag } intents


paramOf : Model -> Addr -> Maybe Param
paramOf m a =
    Schema.paramIn m.variants a m.schema


isChoice : Param -> Bool
isChoice p =
    case p.kind of
        Cont ->
            False

        Stepped ->
            False

        Choice ->
            True

        Toggle ->
            True


{-| What a hand reaches: the unit's own value. A control the unit has not
reported cannot be taken hold of.
-}
target : Model -> Addr -> Maybe Target
target m a =
    paramOf m a
        |> Maybe.andThen
            (\p ->
                if isChoice p then
                    Control.current (chosen m p) |> Maybe.map (\i -> Discrete { options = List.map Tuple.first p.options, current = i })

                else
                    Model.shown p.addr m |> Maybe.map (\v -> Continuous { norm = v.norm, wheel = Control.wheelNorm p })
            )


intend : Model -> Intent -> State -> State
intend m intent s =
    let
        on : Addr -> (Param -> State) -> State
        on a f =
            paramOf m a |> Maybe.map f |> Maybe.withDefault s
    in
    case intent of
        SetNorm a t ->
            on a (\p -> want p.addr (Number (Control.atNorm p t)) s)

        Nudge a n ->
            on a (\p -> want p.addr (Number (Control.nudged p n)) s)

        SetOption a i ->
            want a (Option i) s

        Restore a ->
            on a
                (\p ->
                    if isChoice p then
                        Protocol.optIndex (round p.default)
                            |> Maybe.map (\i -> want p.addr (Option i) s)
                            |> Maybe.withDefault s

                    else
                        want p.addr (Number p.default) s
                )


{-| The last want of a control this frame.
-}
want : Addr -> Want -> State -> State
want a w s =
    { s | wanted = Control.want a w s.wanted }



-- WHERE A CONTROL IS NOW: THE UNIT'S VALUE


reading : Model -> Param -> Reading
reading m p =
    let
        r : Reading
        r =
            case Model.shown p.addr m of
                Just v ->
                    Control.reading v.norm v.text

                Nothing ->
                    Control.missing
    in
    case Model.refusal p.addr m of
        Just why ->
            Control.refused why r

        Nothing ->
            r


chosen : Model -> Param -> Chosen
chosen m p =
    let
        c : Chosen
        c =
            case Model.shown p.addr m of
                Just v ->
                    case Protocol.optIndex (round v.stored) |> Maybe.andThen (\i -> p.options |> List.filter (\( k, _ ) -> k == i) |> List.head) of
                        Just ( i, _ ) ->
                            Control.chosen i

                        Nothing ->
                            Control.notInList

                Nothing ->
                    Control.noValue
    in
    case Model.refusal p.addr m of
        Just why ->
            Control.refusedChoice why c

        Nothing ->
            c


{-| A choice turned like a knob (an operator's ratio): its option's place
along the list, and the option's name.
-}
choiceReading : Model -> Param -> Reading
choiceReading m p =
    let
        ch : Chosen
        ch =
            chosen m p

        r : Reading
        r =
            case Control.current ch of
                Just i ->
                    p.options
                        |> List.indexedMap (\k ( o, _ ) -> ( k, o ))
                        |> List.filter (\( _, o ) -> o == i)
                        |> List.head
                        |> Maybe.map (\( k, _ ) -> Control.reading (toFloat k / toFloat (max 1 (List.length p.options - 1))) (Control.choiceText p.options ch))
                        |> Maybe.withDefault Control.missing

                Nothing ->
                    Control.missing
    in
    case Model.refusal p.addr m of
        Just why ->
            Control.refused why r

        Nothing ->
            r



-- THE SCREENS


{-| What a screen on `page` draws from: that page's own addresses and the
selectors its variants hang on (FILTER.KIND for FILTER's), in `describe`
order, each with the unit's stored value. Nothing else, so a value elsewhere
never repaints it.
-}
screenVals : Model -> PageNo -> List ( Addr, Float )
screenVals m n =
    let
        own : List Addr
        own =
            Schema.pageAddresses n m.schema

        selectors : List Addr
        selectors =
            Schema.pages m.schema
                |> List.filter (\pg -> pg.n == n)
                |> List.concatMap .panels
                |> List.filter (\pn -> pn.variant /= "-")
                |> List.filterMap (.name >> UnitPage.variantSelector)

        wanted : Set String
        wanted =
            Set.fromList (List.map Addr.key (own ++ selectors))
    in
    Schema.addresses m.schema
        |> List.filter (\a -> Set.member (Addr.key a) wanted)
        |> List.filterMap (\a -> Model.shown a m |> Maybe.map (\v -> ( a, v.stored )))


{-| A module's screen, as `<chimera-screen>` is told it.
-}
screenSpec : Model -> Engine -> Shared.ScreenAt -> Maybe Screen.Config
screenSpec m e sc =
    UnitPage.resolve sc.page m.schema
        |> Maybe.map
            (\n ->
                Screen.drawn e n sc.shape
                    |> (sc.fallback |> Maybe.map Screen.fallback |> Maybe.withDefault identity)
                    |> (sc.focus |> Maybe.map Screen.focus |> Maybe.withDefault identity)
                    |> (UnitPage.pad sc.page |> Maybe.map Screen.onPad |> Maybe.withDefault identity)
                    |> Screen.values (screenVals m n)
            )


{-| Pad `p`'s icon in the strip: the unit's own for the pad's machine.
-}
padIcon : Model -> Pad -> Screen.Config
padIcon m p =
    case UnitPage.resolve (PadPage p PadMch) m.schema of
        Just n ->
            let
                machine : Addr
                machine =
                    Addr.at (Addr.Pad p Voice) (Addr.field "MACHINE")
            in
            Screen.padIcon n p |> Screen.values (Model.shown machine m |> Maybe.map (\v -> [ ( machine, v.stored ) ]) |> Maybe.withDefault [])

        Nothing ->
            Screen.new Drum (Screen.PadIcon p)



-- THE PAGE


{-| What every module of a page draws from.
-}
type alias Ctx =
    { m : Model
    , s : State
    , engine : Engine
    , items : Dict String Item
    }


view : Model -> State -> Html Msg
view m s =
    case ( Model.notice m, m.described, m.link ) of
        ( Nothing, Just e, Just Transport.Open ) ->
            -- The page stays up while the unit is read again (another
            -- engine's schema): it says so, and takes no edits meanwhile.
            enginePage m s (Model.status m |> Maybe.withDefault "READING THE UNIT") e

        _ ->
            connectPage m s


connectPage : Model -> State -> Html Msg
connectPage m s =
    let
        says : String
        says =
            case Model.notice m of
                Just n ->
                    n

                Nothing ->
                    if m.link == Just Transport.Open then
                        "READING THE UNIT"

                    else
                        "NOT CONNECTED"
    in
    Page.view PageMsg
        s.page
        (Page.new "CHIMERA"
            |> Page.meta says
            |> Page.modules
                (saidModule m
                    ++ [ Module.new Chip.Unit "UNIT"
                            |> Module.note says
                            |> Module.body
                                [ div [ class "connect" ]
                                    [ button [ class "hbtn", onClick (Unit (Model.Connect Model.ViaUnit)) ] [ text "CONNECT UNIT" ]
                                    , button [ class "hbtn", onClick (Unit (Model.Connect Model.ViaSimulator)) ] [ text "CONNECT SIMULATOR" ]
                                    ]
                                ]
                       ]
                )
        )


{-| What the unit refused or the editor couldn't read, in the unit's words,
until the user has seen it; nothing kept, no module.
-}
saidModule : Model -> List (Module.Config Msg)
saidModule m =
    if List.isEmpty (Model.notices m) then
        []

    else
        let
            line : Model.Kept -> Html Msg
            line k =
                div []
                    [ text (Model.noticeText k.notice)
                    , text
                        (if k.times > 1 then
                            " ×" ++ String.fromInt k.times

                         else
                            ""
                        )
                    ]

            lost : List (Html Msg)
            lost =
                if Model.noticesLost m > 0 then
                    [ div [] [ text ("AND " ++ String.fromInt (Model.noticesLost m) ++ " MORE NO LONGER KEPT") ] ]

                else
                    []
        in
        [ Module.new Chip.Unit "THE UNIT SAID"
            |> Module.full
            |> Module.body
                [ div [ class "said" ]
                    (List.map line (Model.notices m) ++ lost ++ [ div [ class "connect" ] [ button [ class "hbtn", onClick (Unit Model.SeenNotices) ] [ text "SEEN" ] ] ])
                ]
        ]


enginePage : Model -> State -> String -> Engine -> Html Msg
enginePage m s status e =
    let
        lay : Layout
        lay =
            layout e s.tab

        items : List Item
        items =
            Shared.active m.schema m.variants

        ctx : Ctx
        ctx =
            { m = m
            , s = s
            , engine = e
            , items = Shared.index items
            }

        rest : List Item
        rest =
            other items lay.every

        title : String
        title =
            (m.part |> Maybe.map (\pt -> "PART " ++ String.fromInt (Unit.partNumber pt) ++ " · ") |> Maybe.withDefault "") ++ Unit.engineName e
    in
    Page.view PageMsg
        s.page
        (Page.new title
            |> Page.meta status
            |> Page.parts (partTabs m)
            |> Page.link (Page.Linked status)
            |> (case e of
                    Drum ->
                        Page.strip (Html.map PadMsg (PadStrip.view (PadStrip.new "DRUM" (padName m) |> PadStrip.icons (padIcon m)) s.tab))

                    Algo ->
                        identity

                    Va ->
                        identity

                    Modal ->
                        identity

                    Tape ->
                        identity
               )
            |> Page.modules
                (saidModule m
                    ++ List.map (moduleOf ctx) lay.shown
                    ++ (if List.isEmpty rest then
                            []

                        else
                            [ otherModule ctx rest ]
                       )
                )
        )


{-| A pad's name in the strip: its machine, as the unit prints it.
-}
padName : Model -> Pad -> String
padName m p =
    Model.shown (Addr.at (Addr.Pad p Voice) (Addr.field "MACHINE")) m |> Maybe.map .text |> Maybe.withDefault "--"


partTabs : Model -> List (Page.Tab Msg)
partTabs m =
    List.map
        (\pt ->
            { kicker = "P" ++ String.fromInt (Unit.partNumber pt)
            , name =
                case ( m.part == Just pt, m.engine ) of
                    ( True, Just e ) ->
                        Unit.engineName e

                    ( True, Nothing ) ->
                        "PART"

                    ( False, _ ) ->
                        "PART"
            , current =
                if m.part == Just pt then
                    Here

                else
                    Elsewhere
            , go = Unit (Model.FollowPart pt)
            }
        )
        Unit.allParts


moduleOf : Ctx -> Module -> Module.Config Msg
moduleOf ctx md =
    let
        groups : List ( String, List ( Item, As ) )
        groups =
            Shared.fill ctx.items md

        headers : List Item
        headers =
            List.filterMap (\a -> Dict.get (Addr.key a) ctx.items) (Shared.header md)

        screen : List (Html Msg)
        screen =
            Shared.screen md
                |> Maybe.andThen (screenSpec ctx.m ctx.engine)
                |> Maybe.map (\sc -> [ Module.group "" [ Screen.view sc ] ])
                |> Maybe.withDefault []

        nothing : List (Html Msg)
        nothing =
            if List.isEmpty groups && List.isEmpty (Shared.extras md) then
                [ p [ class "pnote" ] [ text "NOTHING TO SET FOR THIS TYPE" ] ]

            else
                []
    in
    Module.new (Shared.chip md) (Shared.title md)
        |> Module.note (Shared.note md)
        |> Module.menus (List.map (\i -> menuOf ctx True i) headers)
        |> Module.summary (List.map (summaryOf ctx) (List.take 3 (headers ++ List.concatMap (Tuple.second >> List.map Tuple.first) groups)))
        |> (if Shared.isWhole md then
                Module.full

            else
                identity
           )
        |> Module.body
            ((if List.isEmpty screen && List.isEmpty groups then
                []

              else
                [ Module.panel (screen ++ List.map (\( t, xs ) -> Module.group t (List.map (control ctx) xs)) groups) ]
             )
                ++ nothing
                ++ List.map (extra ctx) (Shared.extras md)
            )


otherModule : Ctx -> List Item -> Module.Config Msg
otherModule ctx items =
    Module.new Chip.Other "OTHER"
        |> Module.note "DESCRIBED BY THE UNIT, NOT YET PLACED"
        |> Module.body [ Module.panel [ Module.group "" (List.map (\i -> control ctx ( i, Auto )) items) ] ]


summaryOf : Ctx -> Item -> ( String, String )
summaryOf ctx i =
    ( i.label
    , if isChoice i.param then
        Control.choiceText i.param.options (chosen ctx.m i.param)

      else
        Control.text (reading ctx.m i.param)
    )


extra : Ctx -> Extra -> Html Msg
extra ctx x =
    case x of
        NextSlice ->
            let
                named : List String
                named =
                    Schema.pages ctx.m.schema
                        |> List.concatMap (.panels >> List.filter (\pn -> pn.variant == "-" || Dict.get pn.name ctx.m.variants == Just pn.variant) >> List.concatMap (.routes >> List.map .label))
                        |> List.foldl
                            (\r acc ->
                                if List.member r acc then
                                    acc

                                else
                                    acc ++ [ r ]
                            )
                            []
            in
            if List.isEmpty named then
                text ""

            else
                p [ class "pnote" ] [ text (String.join " · " named ++ ": MOD MATRIX ROUTES, IN MOD ONCE THE UNIT DESCRIBES THEM") ]

        SourceText ->
            Module.panel
                [ Module.group ""
                    [ Control.cell [ class "ro" ]
                        "SOURCE"
                        [ Control.readout sourceChars
                            (case ctx.m.source of
                                Just t ->
                                    Control.reading 0 t

                                Nothing ->
                                    Control.missing
                            )
                        ]
                    ]
                ]

        Note t ->
            p [ class "pnote" ] [ text t ]

        PadRoutes pad ->
            Html.map (BayMsg (PadBay pad)) (PatchBay.view (bayOf (PadBay pad) ctx.s) (padBay ctx.m pad))

        PartMatrix ->
            div []
                [ Html.map (BayMsg PartBay) (PatchBay.view (bayOf PartBay ctx.s) (Routes.new [] []))
                , p [ class "pnote" ] [ text "THE UNIT DOESN'T DESCRIBE ITS MOD MATRIX YET" ]
                ]


{-| TAPE's sample name: the card's longest that fits the cell.
-}
sourceChars : Ui.Tokens.Chars
sourceChars =
    Ui.Tokens.oneMore (Ui.Tokens.oneMore Ui.Tokens.readoutChars)



-- THE CONTROLS, BY MEANING


control : Ctx -> ( Item, As ) -> Html Msg
control ctx ( item, as_ ) =
    let
        p : Param
        p =
            item.param
    in
    if isChoice p then
        case as_ of
            AsKnob ->
                choiceKnob ctx item False

            AsHero ->
                choiceKnob ctx item True

            AsMenu ->
                menuOf ctx False item

            Auto ->
                autoChoice ctx item

            AsFader ->
                autoChoice ctx item

            AsHeroFader ->
                autoChoice ctx item

    else
        case as_ of
            AsKnob ->
                contKnob ctx item False

            AsHero ->
                contKnob ctx item True

            AsFader ->
                contFader ctx item False

            AsHeroFader ->
                contFader ctx item True

            AsMenu ->
                contKnob ctx item False

            Auto ->
                case p.scale of
                    Octaves ->
                        contKnob ctx item False

                    Linear ->
                        contFader ctx item False


contKnob : Ctx -> Item -> Bool -> Html Msg
contKnob ctx item big =
    Knob.new item.param.addr (Control.polarity item.param)
        |> Knob.label item.label
        |> when big Knob.hero
        |> (\c -> Html.map Hand (Knob.view c (reading ctx.m item.param)))


contFader : Ctx -> Item -> Bool -> Html Msg
contFader ctx item big =
    Fader.new item.param.addr (Control.polarity item.param)
        |> Fader.label item.label
        |> when big Fader.hero
        |> (\c -> Html.map Hand (Fader.view c (reading ctx.m item.param)))


{-| A choice turned like a knob: its options run one way, never either side
of nothing.
-}
choiceKnob : Ctx -> Item -> Bool -> Html Msg
choiceKnob ctx item big =
    Knob.new item.param.addr (Control.polarity { min = 0, max = 1 })
        |> Knob.label item.label
        |> when big Knob.hero
        |> (\c -> Html.map Hand (Knob.view c (choiceReading ctx.m item.param)))


when : Bool -> (a -> a) -> a -> a
when yes f =
    if yes then
        f

    else
        identity


{-| By the option count: two or three a printed toggle, four to eight a
selector, more a menu.
-}
autoChoice : Ctx -> Item -> Html Msg
autoChoice ctx item =
    let
        p : Param
        p =
            item.param

        now : Chosen
        now =
            chosen ctx.m p
    in
    case ( Toggle.fromOptions p.addr p.options, Selector.fromOptions p.addr p.options ) of
        ( Just t, _ ) ->
            Html.map Hand (Toggle.view (Toggle.label item.label t) now)

        ( Nothing, Just sl ) ->
            Html.map Hand (Selector.view (Selector.label item.label sl) now)

        ( Nothing, Nothing ) ->
            menuOf ctx False item


menuOf : Ctx -> Bool -> Item -> Html Msg
menuOf ctx header item =
    case Control.options item.param.options of
        Just opts ->
            Html.map MenuMsg (Menu.view (Menu.new item.param.addr opts |> Menu.label item.label |> when header Menu.inHeader) ctx.s.menu (chosen ctx.m item.param))

        Nothing ->
            -- A choice the unit lists no options for: turned like a knob.
            choiceKnob ctx item False
