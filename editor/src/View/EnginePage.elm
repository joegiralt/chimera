module View.EnginePage exposing (BayId(..), Held, Layout, Msg(..), State, init, layout, other, subscriptions, update, view)

{-| An engine's page: the unit's schema, placed by the engine's layout,
read from `Model`, drawn with the Ui components. What the unit describes and
the layout doesn't place is shown in OTHER, never hidden.

A hand never changes a value here: it asks for edits, at most one `ed set`
a frame for each control, and holds the control where the hand put it until
the unit's report (or the pending mark's end) takes over.

-}

import Browser.Events
import Dict exposing (Dict)
import Html exposing (Html, button, div, p, text)
import Html.Attributes exposing (class)
import Html.Events exposing (onClick)
import Layout.Algo
import Layout.Drum
import Layout.Modal
import Layout.Shared as Shared exposing (As(..), Extra(..), Item, Module)
import Layout.Tape
import Layout.Va
import Model exposing (Model)
import Protocol exposing (Addr(..), Kind(..), OptIndex, Scale(..), Value(..), addrKey)
import Schema exposing (Param)
import Set exposing (Set)
import Transport
import Ui.Control as Control exposing (Chosen, Intent(..), Reading, Target(..), Want(..))
import Ui.Fader as Fader
import Ui.Knob as Knob
import Ui.Menu as Menu
import Ui.Module as Module
import Ui.Pad as Pad exposing (Pad)
import Ui.PadStrip as PadStrip exposing (Tab(..))
import Ui.Page as Page exposing (Current(..), Effect(..))
import Ui.PatchBay as PatchBay
import Ui.Routes as Routes exposing (Dest(..), Source(..))
import Ui.Screen as Screen
import Ui.Selector as Selector
import Ui.Toggle as Toggle
import Ui.Tokens exposing (Theme)
import Unit exposing (Engine(..))



-- STATE


{-| Where a hand put a control, shown until the unit's own value takes over.
-}
type Held
    = HeldNorm Float
    | HeldOption OptIndex


{-| A patch bay on the page: the Part's mod matrix, or a DRUM pad's routes.
-}
type BayId
    = PartBay
    | PadBay Pad


type alias State =
    { page : Page.State
    , drag : Control.Drag
    , grabbed : Maybe Addr
    , wanted : Control.Wanted
    , held : Dict String Held
    , menu : Menu.State
    , bays : Dict String ( BayId, PatchBay.Bay )
    , tab : Tab
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
    , grabbed = Nothing
    , wanted = Control.empty
    , held = Dict.empty
    , menu = Menu.shut
    , bays = Dict.empty
    , tab = Kit
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
        placed : Set String
        placed =
            Set.fromList (List.concatMap (Shared.addresses >> List.map addrKey) every)
    in
    List.filter (\i -> not (Set.member (addrKey i.param.addr) placed)) items



-- WHAT A HAND DOES


update : Msg -> Model -> State -> ( State, List Model.Msg, Maybe Theme )
update msg m s =
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

        ( PadBay p, PatchBay.Patched (Source from) (Dest to) ) ->
            let
                routes : List RouteNow
                routes =
                    routesNow m s p
            in
            if List.any (\r -> r.on && r.from == Just from && r.to == Just to) routes then
                s

            else
                case List.filter (.on >> not) routes |> List.head of
                    Just r ->
                        s
                            |> pickNamed m r.route.source from
                            |> pickNamed m r.route.dest to
                            |> (\st -> paramOf m r.route.amount |> Maybe.map (\ap -> want ap (Number (Control.atNorm ap newCable)) (HeldNorm newCable) st) |> Maybe.withDefault st)

                    Nothing ->
                        s

        ( PadBay p, PatchBay.Unpatched (Source from) (Dest to) ) ->
            routesNow m s p
                |> List.filter (\r -> r.on && r.from == Just from && r.to == Just to)
                |> List.foldl (\r st -> paramOf m r.route.amount |> Maybe.map (\ap -> want ap (Number 0) (HeldNorm (normOf ap 0)) st) |> Maybe.withDefault st) s


{-| Where a new cable's attenuverter starts: half way up.
-}
newCable : Float
newCable =
    0.75


pickNamed : Model -> Addr -> String -> State -> State
pickNamed m a name s =
    paramOf m a
        |> Maybe.andThen (\p -> p.options |> List.filter (\( _, n ) -> n == name) |> List.head |> Maybe.map (\( i, _ ) -> want p (Option i) (HeldOption i) s))
        |> Maybe.withDefault s


{-| A pad route as the unit (or a hand) has it now: patched while its amount
is not nothing.
-}
type alias RouteNow =
    { route : Shared.Route, on : Bool, from : Maybe String, to : Maybe String, amount : Maybe Param }


routesNow : Model -> State -> Pad -> List RouteNow
routesNow m s p =
    List.map
        (\r ->
            let
                amount : Maybe Param
                amount =
                    paramOf m r.amount

                stored : Maybe Float
                stored =
                    amount
                        |> Maybe.andThen
                            (\ap ->
                                case holding m s ap.addr of
                                    Just (HeldNorm t) ->
                                        Just (Control.atNorm ap t)

                                    Just (HeldOption _) ->
                                        Nothing

                                    Nothing ->
                                        Model.shown ap.addr m |> Maybe.map .stored
                            )

                name : Addr -> Maybe String
                name a =
                    paramOf m a |> Maybe.map (\q -> optionName q (optionNow m s q))
            in
            { route = r
            , on = Maybe.map2 (\v ap -> abs v >= ap.step / 2) stored amount |> Maybe.withDefault False
            , from = name r.source
            , to = name r.dest
            , amount = amount
            }
        )
        (Shared.padRoutes p)


padBay : Model -> State -> Pad -> Routes.Routes
padBay m s p =
    let
        routes : List RouteNow
        routes =
            routesNow m s p

        names : (Shared.Route -> Addr) -> List String
        names f =
            routes |> List.head |> Maybe.andThen (\r -> paramOf m (f r.route)) |> Maybe.map (.options >> List.map Tuple.second) |> Maybe.withDefault []
    in
    routes
        |> List.filterMap
            (\r ->
                case ( r.on, ( r.from, r.to ), r.amount ) of
                    ( True, ( Just from, Just to ), Just ap ) ->
                        Just { from = Source from, to = Dest to, amount = Routes.amountFromNorm (normNow m s ap), addr = ap.addr }

                    _ ->
                        Nothing
            )
        |> List.foldl Routes.add (Routes.new (List.map Source (names .source)) (List.map Dest (names .dest)))


hand : Model -> Control.Msg -> State -> State
hand m h s =
    let
        ( drag, intents ) =
            Control.update (target m s) h s.drag

        grabbed : Maybe Addr
        grabbed =
            case h of
                Control.Grab a _ _ ->
                    Just a

                Control.Released ->
                    Nothing

                Control.Moved _ ->
                    s.grabbed

                Control.Step _ _ ->
                    s.grabbed

                Control.Pick _ _ ->
                    s.grabbed

                Control.Reset _ ->
                    s.grabbed
    in
    List.foldl (intend m) { s | drag = drag, grabbed = grabbed } intents


paramOf : Model -> Addr -> Maybe Param
paramOf m a =
    Schema.paramIn m.variants a m.schema


isChoice : Param -> Bool
isChoice p =
    case p.kind of
        Cont ->
            False

        Choice ->
            True

        Toggle ->
            True


target : Model -> State -> Addr -> Maybe Target
target m s a =
    paramOf m a
        |> Maybe.map
            (\p ->
                if isChoice p then
                    optionNow m s p |> Maybe.map (\i -> Discrete { options = List.map Tuple.first p.options, current = i })

                else
                    Just (Continuous { norm = normNow m s p, wheel = Control.wheelNorm p })
            )
        |> Maybe.andThen identity


intend : Model -> Intent -> State -> State
intend m intent s =
    let
        on : Addr -> (Param -> State) -> State
        on a f =
            paramOf m a |> Maybe.map f |> Maybe.withDefault s
    in
    case intent of
        SetNorm a t ->
            on a (\p -> want p (Number (Control.atNorm p t)) (HeldNorm (clamp 0 1 t)) s)

        SetOption a i ->
            on a (\p -> want p (Option i) (HeldOption i) s)

        Restore a ->
            on a
                (\p ->
                    if isChoice p then
                        Protocol.optIndex (round p.default)
                            |> Maybe.map (\i -> want p (Option i) (HeldOption i) s)
                            |> Maybe.withDefault s

                    else
                        want p (Number p.default) (HeldNorm (normOf p p.default)) s
                )


{-| The last want of a control this frame, and where its hand now is.
-}
want : Param -> Want -> Held -> State -> State
want p w h s =
    { s | wanted = Control.want p.addr w s.wanted, held = Dict.insert (addrKey p.addr) h s.held }


{-| Where a stored value sits on the travel: `Control.atNorm` backwards.
-}
normOf : Param -> Float -> Float
normOf p v =
    case p.scale of
        Linear ->
            if p.max == p.min then
                0

            else
                clamp 0 1 ((v - p.min) / (p.max - p.min))

        Octaves ->
            if p.min <= 0 || v <= 0 || p.max <= p.min then
                0

            else
                clamp 0 1 (logBase 2 (v / p.min) / logBase 2 (p.max / p.min))



-- WHERE A CONTROL IS NOW


{-| A hand holds the control while it drags it, while its edit waits to
leave, and while the unit has not confirmed it.
-}
holding : Model -> State -> Addr -> Maybe Held
holding m s a =
    if s.grabbed == Just a || Model.isPending a m || Control.wants a s.wanted then
        Dict.get (addrKey a) s.held

    else
        Nothing


normNow : Model -> State -> Param -> Float
normNow m s p =
    case holding m s p.addr of
        Just (HeldNorm t) ->
            t

        Just (HeldOption _) ->
            Model.shown p.addr m |> Maybe.map .norm |> Maybe.withDefault 0

        Nothing ->
            Model.shown p.addr m |> Maybe.map .norm |> Maybe.withDefault 0


optionNow : Model -> State -> Param -> Maybe OptIndex
optionNow m s p =
    let
        unit : Maybe OptIndex
        unit =
            Model.shown p.addr m |> Maybe.andThen (.stored >> round >> Protocol.optIndex)

        held : Maybe OptIndex
        held =
            case holding m s p.addr of
                Just (HeldOption i) ->
                    Just i

                Just (HeldNorm _) ->
                    Nothing

                Nothing ->
                    Nothing

        first : Maybe OptIndex
        first =
            p.options |> List.head |> Maybe.map Tuple.first
    in
    [ held, unit, Protocol.optIndex (round p.default), first ] |> List.filterMap identity |> List.head


isHeldOrPending : Model -> State -> Addr -> Bool
isHeldOrPending m s a =
    Model.isPending a m || holding m s a /= Nothing


reading : Model -> State -> Param -> Reading
reading m s p =
    let
        r : Reading
        r =
            Control.reading (normNow m s p) (Model.shown p.addr m |> Maybe.map .text |> Maybe.withDefault "")
    in
    if isHeldOrPending m s p.addr then
        Control.pending r

    else
        r


chosen : Model -> State -> Param -> Maybe Chosen
chosen m s p =
    optionNow m s p
        |> Maybe.map
            (\i ->
                if isHeldOrPending m s p.addr then
                    Control.pendingChoice (Control.chosen i)

                else
                    Control.chosen i
            )


optionName : Param -> Maybe OptIndex -> String
optionName p i =
    p.options |> List.filter (\( k, _ ) -> Just k == i) |> List.head |> Maybe.map Tuple.second |> Maybe.withDefault "--"


{-| A choice turned like a knob (an operator's ratio): its option's place
along the list, and the option's name.
-}
choiceReading : Model -> State -> Param -> Reading
choiceReading m s p =
    let
        i : Maybe OptIndex
        i =
            optionNow m s p

        at : Int
        at =
            p.options |> List.indexedMap (\k ( o, _ ) -> ( k, o )) |> List.filter (\( _, o ) -> Just o == i) |> List.head |> Maybe.map Tuple.first |> Maybe.withDefault 0

        r : Reading
        r =
            Control.reading (toFloat at / toFloat (max 1 (List.length p.options - 1))) (optionName p i)
    in
    if isHeldOrPending m s p.addr then
        Control.pending r

    else
        r



-- THE PAGE


{-| What every module of a page draws from.
-}
type alias Ctx =
    { m : Model
    , s : State
    , engine : Engine
    , items : Dict String Item
    , pages : Dict String Int
    , vals : List ( Addr, Float )
    }


view : Model -> State -> Html Msg
view m s =
    case ( Model.status m, m.engine ) of
        ( Just status, Just e ) ->
            enginePage m s status e

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
                [ Module.new "UNIT"
                    |> Module.note says
                    |> Module.body
                        [ div [ class "connect" ]
                            [ button [ class "hbtn", onClick (Unit Model.ConnectSerial) ] [ text "CONNECT UNIT" ]
                            , button [ class "hbtn", onClick (Unit Model.ConnectSocket) ] [ text "CONNECT SIMULATOR" ]
                            ]
                        ]
                ]
        )


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
            , pages = Dict.fromList (List.map (\pg -> ( pg.label, pg.n )) (Schema.pages m.schema))
            , vals = Dict.toList m.values |> List.map (\( k, r ) -> ( Addr k, r.stored ))
            }

        rest : List Item
        rest =
            other items lay.every

        title : String
        title =
            (m.part |> Maybe.map (\p -> "PART " ++ String.fromInt (Unit.partNumber p) ++ " · ") |> Maybe.withDefault "") ++ Unit.engineName e
    in
    Page.view PageMsg
        s.page
        (Page.new title
            |> Page.meta status
            |> Page.parts (partTabs m)
            |> Page.link (Page.Linked status)
            |> (case e of
                    Drum ->
                        Page.strip (Html.map PadMsg (PadStrip.view (PadStrip.new "DRUM" (padName m)) s.tab))

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
                (List.map (moduleOf ctx) lay.shown
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
    Model.shown (Addr ("PAD" ++ String.fromInt (Pad.number p) ++ ".MACHINE")) m |> Maybe.map .text |> Maybe.withDefault ""


partTabs : Model -> List (Page.Tab Msg)
partTabs m =
    List.map
        (\p ->
            { kicker = "P" ++ String.fromInt (Unit.partNumber p)
            , name =
                if m.part == Just p then
                    m.engine |> Maybe.map Unit.engineName |> Maybe.withDefault ""

                else
                    "PART"
            , current =
                if m.part == Just p then
                    Here

                else
                    Elsewhere
            , go = Unit (Model.FollowPart p)
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
            List.filterMap (\a -> Dict.get (addrKey a) ctx.items) md.header

        screen : List (Html Msg)
        screen =
            md.screen
                |> Maybe.andThen
                    (\sc ->
                        Dict.get sc.page ctx.pages
                            |> Maybe.map
                                (\n ->
                                    Screen.drawn ctx.engine n sc.shape
                                        |> (sc.fallback |> Maybe.map Screen.fallback |> Maybe.withDefault identity)
                                        |> Screen.values ctx.vals
                                        |> Screen.view
                                )
                    )
                |> Maybe.map (\h -> [ Module.group "" [ h ] ])
                |> Maybe.withDefault []

        extras : List (Html Msg)
        extras =
            List.map (extra ctx) md.extras

        nothing : List (Html Msg)
        nothing =
            if List.isEmpty groups && List.isEmpty md.extras then
                [ p [ class "pnote" ] [ text "NOTHING TO SET FOR THIS TYPE" ] ]

            else
                []
    in
    Module.new md.title
        |> Module.note md.note
        |> Module.chip md.chip
        |> Module.menus (List.map (\i -> menuOf ctx True i) headers)
        |> Module.summary (List.map (summaryOf ctx) (List.take 3 (headers ++ List.concatMap (Tuple.second >> List.map Tuple.first) groups)))
        |> (if md.full then
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
                ++ extras
            )


otherModule : Ctx -> List Item -> Module.Config Msg
otherModule ctx items =
    Module.new "OTHER"
        |> Module.note "DESCRIBED BY THE UNIT, NOT YET PLACED"
        |> Module.chip "OTHER"
        |> Module.body [ Module.panel [ Module.group "" (List.map (\i -> control ctx ( i, Auto )) items) ] ]


summaryOf : Ctx -> Item -> ( String, String )
summaryOf ctx i =
    ( i.label
    , if isChoice i.param then
        optionName i.param (optionNow ctx.m ctx.s i.param)

      else
        Model.shown i.param.addr ctx.m |> Maybe.map .text |> Maybe.withDefault "--"
    )


extra : Ctx -> Extra -> Html Msg
extra ctx x =
    case x of
        NextSlice ->
            let
                named : List String
                named =
                    Schema.pages ctx.m.schema
                        |> List.concatMap (.panels >> List.filter (\pn -> pn.variant == "-" || Dict.get pn.name ctx.m.variants == Just pn.variant) >> List.concatMap .routes)
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
            Module.panel [ Module.group "" [ Control.cell [ class "ro" ] "SOURCE" [ Control.readout 16 (Control.reading 0 (Maybe.withDefault "--" ctx.m.source)) ] ] ]

        Note t ->
            p [ class "pnote" ] [ text t ]

        PadRoutes pad ->
            Html.map (BayMsg (PadBay pad)) (PatchBay.view (bayOf (PadBay pad) ctx.s) (padBay ctx.m ctx.s pad))

        PartMatrix ->
            div []
                [ Html.map (BayMsg PartBay) (PatchBay.view (bayOf PartBay ctx.s) (Routes.new [] []))
                , p [ class "pnote" ] [ text "THE UNIT DOESN'T DESCRIBE ITS MOD MATRIX YET" ]
                ]



-- THE CONTROLS, BY MEANING


bipolar : Param -> Bool
bipolar p =
    p.min < 0 && p.max > 0


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
    Knob.new item.param.addr
        |> Knob.label item.label
        |> when big Knob.hero
        |> when (bipolar item.param) Knob.bipolar
        |> (\c -> Html.map Hand (Knob.view c (reading ctx.m ctx.s item.param)))


contFader : Ctx -> Item -> Bool -> Html Msg
contFader ctx item big =
    Fader.new item.param.addr
        |> Fader.label item.label
        |> when big Fader.hero
        |> when (bipolar item.param) Fader.bipolar
        |> (\c -> Html.map Hand (Fader.view c (reading ctx.m ctx.s item.param)))


choiceKnob : Ctx -> Item -> Bool -> Html Msg
choiceKnob ctx item big =
    Knob.new item.param.addr
        |> Knob.label item.label
        |> when big Knob.hero
        |> (\c -> Html.map Hand (Knob.view c (choiceReading ctx.m ctx.s item.param)))


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

        n : Int
        n =
            List.length p.options

        c : Maybe Chosen
        c =
            chosen ctx.m ctx.s p
    in
    if n >= 2 && n <= 3 then
        Maybe.map2 (\t now -> Html.map Hand (Toggle.view (Toggle.label item.label t) now)) (Toggle.fromOptions p.addr p.options) c
            |> Maybe.withDefault (menuOf ctx False item)

    else if n >= 4 && n <= 8 then
        Maybe.map2 (\sl now -> Html.map Hand (Selector.view (Selector.label item.label sl) now)) (Selector.fromOptions p.addr p.options) c
            |> Maybe.withDefault (menuOf ctx False item)

    else
        menuOf ctx False item


menuOf : Ctx -> Bool -> Item -> Html Msg
menuOf ctx header item =
    case ( Menu.fromOptions item.param.addr item.param.options, chosen ctx.m ctx.s item.param ) of
        ( Just cfg, Just now ) ->
            Html.map MenuMsg (Menu.view (cfg |> Menu.label item.label |> when header Menu.inHeader) ctx.s.menu now)

        _ ->
            -- A choice the unit lists no options for: turned like a knob.
            choiceKnob ctx item False
