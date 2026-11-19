module Layout.Shared exposing
    ( As(..)
    , Extra(..)
    , Group
    , Item
    , Module
    , Route
    , ScreenAt
    , Slot
    , active
    , addresses
    , ampOut
    , auto
    , envelope
    , fader
    , fill
    , filter
    , filterCore
    , filterMod
    , group
    , hero
    , heroFader
    , index
    , knob
    , lfo
    , menu
    , mod
    , module_
    , padRoutes
    , pitch
    , strip
    , tall
    , voiceFx
    )

{-| A page is a table: modules in signal order, each placing the unit's
addresses into groups, with the control each gets by meaning. A layout never
invents a parameter: labels, ranges and option names come from the unit, and
an address the Sound doesn't show now (another variant's) is simply not
drawn. The modules every voice engine shares live here.
-}

import Dict exposing (Dict)
import Protocol exposing (Addr(..), addrKey)
import Schema exposing (Schema)
import Ui.Pad as Pad exposing (Pad)
import Ui.Screen exposing (Glyph, Shape(..))



-- THE TABLE'S WORDS


{-| The control a slot gets. `Auto` follows the schema (a choice of 2–3 is a
toggle, 4–8 a selector, more a menu; a frequency a knob, the rest faders);
the others are overrides by meaning: a ratio or a morph is a knob, the one
to noodle a hero, a long list a menu.
-}
type As
    = Auto
    | AsKnob
    | AsHero
    | AsFader
    | AsHeroFader
    | AsMenu


type alias Slot =
    { addr : Addr, as_ : As }


type alias Group =
    { title : String, slots : List Slot }


{-| What a module holds besides its controls.
-}
type Extra
    = -- TAPE's sample name: shown, never edited here (#1015).
      SourceText
    | Note String
      -- The routes a panel names that the unit doesn't describe yet.
    | NextSlice
      -- The Part's mod matrix: no protocol yet, so an empty bay.
    | PartMatrix
      -- A DRUM pad's four routes, a live patch bay.
    | PadRoutes Pad


{-| A screen: the page (by its `ed describe` label) whose art it draws.
-}
type alias ScreenAt =
    { page : String, shape : Shape, fallback : Maybe Glyph }


type alias Module =
    { title : String
    , note : String
    , chip : String
    , header : List Addr
    , screen : Maybe ScreenAt
    , groups : List Group
    , extras : List Extra
    , full : Bool
    }


module_ : String -> String -> Module
module_ title chip =
    { title = title, note = "", chip = chip, header = [], screen = Nothing, groups = [], extras = [], full = False }


group : String -> List Slot -> Group
group =
    Group


slot : As -> String -> Slot
slot a s =
    { addr = Addr s, as_ = a }


auto : String -> Slot
auto =
    slot Auto


knob : String -> Slot
knob =
    slot AsKnob


hero : String -> Slot
hero =
    slot AsHero


fader : String -> Slot
fader =
    slot AsFader


heroFader : String -> Slot
heroFader =
    slot AsHeroFader


menu : String -> Slot
menu =
    slot AsMenu


strip : String -> Maybe ScreenAt
strip page =
    Just { page = page, shape = Strip, fallback = Nothing }


tall : String -> Maybe ScreenAt
tall page =
    Just { page = page, shape = Tall, fallback = Nothing }


{-| One of a DRUM pad's routes: its three addresses.
-}
type alias Route =
    { source : Addr, dest : Addr, amount : Addr }


padRoutes : Pad -> List Route
padRoutes p =
    List.map
        (\k ->
            let
                at : String -> Addr
                at f =
                    Addr ("PAD" ++ String.fromInt (Pad.number p) ++ "_RT" ++ String.fromInt k ++ "." ++ f)
            in
            { source = at "SOURCE", dest = at "DEST", amount = at "AMOUNT" }
        )
        (List.range 1 4)


{-| Every address the module places.
-}
addresses : Module -> List Addr
addresses m =
    m.header
        ++ List.concatMap (.slots >> List.map .addr) m.groups
        ++ List.concatMap
            (\x ->
                case x of
                    PadRoutes p ->
                        List.concatMap (\r -> [ r.source, r.dest, r.amount ]) (padRoutes p)

                    SourceText ->
                        []

                    Note _ ->
                        []

                    NextSlice ->
                        []

                    PartMatrix ->
                        []
            )
            m.extras



-- WHAT THE SOUND SHOWS NOW


{-| A parameter the Sound has now, with the label its most specific panel
gives it.
-}
type alias Item =
    { param : Schema.Param, label : String }


{-| Every parameter of the panels the Sound shows, once each, in the unit's
order. One address can sit on several pages (an operator's WAVE is on the OSC
page and on the operator's own): the variant panel's description wins.
-}
active : Schema -> Dict String String -> List Item
active sch variants =
    let
        shown : Schema.Panel -> Bool
        shown pn =
            pn.variant == "-" || Dict.get pn.name variants == Just pn.variant

        keep : ( Bool, Item ) -> ( List String, Dict String ( Bool, Item ) ) -> ( List String, Dict String ( Bool, Item ) )
        keep ( generic, item ) ( order, best ) =
            let
                k : String
                k =
                    addrKey item.param.addr
            in
            case Dict.get k best of
                Nothing ->
                    ( order ++ [ k ], Dict.insert k ( generic, item ) best )

                Just ( heldGeneric, _ ) ->
                    if heldGeneric && not generic then
                        ( order, Dict.insert k ( generic, item ) best )

                    else
                        ( order, best )

        ( keys, chosen ) =
            Schema.pages sch
                |> List.concatMap
                    (\pg ->
                        pg.panels
                            |> List.filter shown
                            |> List.concatMap (\pn -> List.map (\p -> ( pn.variant == "-", { param = p, label = labelOf pg pn p } )) pn.params)
                    )
                |> List.foldl keep ( [], Dict.empty )
    in
    List.filterMap (\k -> Dict.get k chosen |> Maybe.map Tuple.second) keys


{-| The Sound's items by address.
-}
index : List Item -> Dict String Item
index items =
    Dict.fromList (List.map (\i -> ( addrKey i.param.addr, i )) items)


{-| A module's groups as the Sound shows them now: each slot whose address
the Sound has, and no empty group.
-}
fill : Dict String Item -> Module -> List ( String, List ( Item, As ) )
fill items m =
    m.groups
        |> List.map (\g -> ( g.title, List.filterMap (\s -> Dict.get (addrKey s.addr) items |> Maybe.map (\i -> ( i, s.as_ ))) g.slots ))
        |> List.filter (Tuple.second >> List.isEmpty >> not)


{-| A page of all six operators labels its cells OP1..OP6; the cell then says
what the page is about.
-}
labelOf : Schema.Page -> Schema.Panel -> Schema.Param -> String
labelOf pg pn p =
    case ( pn.variant, String.toList p.label ) of
        ( "-", [ 'O', 'P', d ] ) ->
            if d >= '1' && d <= '6' then
                pg.label

            else
                p.label

        _ ->
            p.label



-- THE MODULES EVERY VOICE SHARES


pitch : Module
pitch =
    let
        m : Module
        m =
            module_ "PITCH" "PITCH"
    in
    { m
        | screen = strip "PIT"
        , groups =
            [ group "" [ hero "PITCH.PITCH", knob "PITCH.FINE" ]
            , group "" [ fader "PITCH.GLIDE_TIME" ]
            , group "" [ auto "PITCH.STEAL", auto "PITCH.SNAP" ]
            ]
    }


{-| A filter whose addresses start `prefix` (FILTER, or a DRUM pad's
PADn\_FLT), its art on page `page`.
-}
filterCore : String -> String -> Module
filterCore prefix page =
    let
        m : Module
        m =
            module_ "FILTER" "FILTER"

        a : String -> String
        a f =
            prefix ++ "." ++ f
    in
    { m
        | header = [ Addr (a "KIND") ]
        , screen = tall page
        , groups =
            [ group "" [ hero (a "CUTOFF"), knob (a "RESO") ]
            , group "" [ auto (a "MODE"), auto (a "VOICE"), knob (a "MORPH") ]
            ]
    }


filter : Module
filter =
    let
        m : Module
        m =
            filterCore "FILTER" "FLT"
    in
    { m | groups = m.groups ++ [ group "" [ fader "FILTER.STAB" ] ] }


{-| The filter's own modulation: feedback, the input, and the two jacks.
-}
filterMod : Module
filterMod =
    let
        m : Module
        m =
            module_ "FILTER MOD" "FILTER"
    in
    { m
        | groups =
            [ group "" [ fader "FILTER.FB", fader "FILTER.LEVEL" ]
            , group "JACKS" [ menu "FILTER.FM3IN", menu "FILTER.QMIN" ]
            ]
        , extras = [ NextSlice ]
    }


voiceFx : Module
voiceFx =
    let
        m : Module
        m =
            module_ "VOICE FX" "FX"
    in
    { m
        | header = [ Addr "VOICEFX.TYPE" ]
        , screen = strip "FX"
        , groups =
            [ group "" [ auto "VOICEFX.PLACE" ]
            , group "" [ hero "VOICEFX.P1", knob "VOICEFX.P2" ]
            , group "" [ fader "VOICEFX.P3", fader "VOICEFX.MIX" ]
            ]
    }


ampOut : Module
ampOut =
    let
        m : Module
        m =
            module_ "AMP · OUT" "AMP"
    in
    { m
        | note = "LOW-PASS GATE · SENDS"
        , screen = strip "SND"
        , groups =
            [ group "GATE" [ heroFader "OUT.LPG", fader "OUT.DECAY", fader "OUT.STRIKE", fader "OUT.VEL" ]
            , group "OUT" [ heroFader "OUT.LEVEL", fader "OUT.SEND1", fader "OUT.SEND2", fader "OUT.SEND3" ]
            ]
    }


{-| Envelope `n`: type A's stages or type B's function, by the unit's TYPE.
-}
envelope : Int -> Module
envelope n =
    let
        m : Module
        m =
            module_ ("ENVELOPE " ++ String.fromInt n) "ENVELOPES"

        a : String -> String
        a f =
            "ENV" ++ String.fromInt n ++ "." ++ f
    in
    { m
        | header = [ Addr (a "TYPE") ]
        , screen = tall ("E" ++ String.fromInt n)
        , groups =
            [ group "" [ fader (a "ATK"), fader (a "H"), fader (a "DEC"), fader (a "SUS"), fader (a "REL") ]
            , group "" [ fader (a "RISE"), fader (a "FALL"), knob (a "SHAPE") ]
            , group "" [ auto (a "MODE"), auto (a "FORM"), auto (a "SPEED"), auto (a "HOLD") ]
            ]
    }


lfo : Int -> Module
lfo n =
    let
        m : Module
        m =
            module_ ("LFO " ++ String.fromInt n) "LFOS"

        a : String -> String
        a f =
            "LFO" ++ String.fromInt n ++ "." ++ f
    in
    { m
        | header = [ Addr (a "TYPE") ]
        , screen = strip ("L" ++ String.fromInt n)
        , groups =
            [ group "" [ hero (a "RATE"), hero (a "RISE"), auto (a "SHAPE") ]
            , group "" [ fader (a "DEPTH"), auto (a "SYNC"), knob (a "PHASE"), knob (a "FALL"), knob (a "SHAPE_B"), auto (a "FORM") ]
            ]
    }


{-| The Part's mod matrix, at full width.
-}
mod : Module
mod =
    let
        m : Module
        m =
            module_ "MOD" "MOD"
    in
    { m | note = "PATCH BAY", full = True, extras = [ PartMatrix ] }
