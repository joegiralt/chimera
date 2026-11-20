module Layout.Shared exposing
    ( As(..)
    , Extra(..)
    , Group
    , Item
    , Module
    , Placement(..)
    , Route
    , ScreenAt
    , Slot
    , active
    , addGroups
    , addresses
    , ampOut
    , auto
    , chip
    , envelope
    , extras
    , fader
    , fill
    , filter
    , filterCore
    , filterMod
    , group
    , header
    , hero
    , heroFader
    , index
    , isWhole
    , knob
    , lfo
    , menu
    , mod
    , module_
    , note
    , padRoutes
    , pitch
    , place
    , screen
    , strip
    , tall
    , title
    , voiceFx
    , withChip
    , withExtras
    , withFallback
    , withFocus
    , withGroups
    , withHeader
    , withNote
    , withScreen
    )

{-| A page is a table: modules in signal order, each placing the unit's
addresses into groups, with the control each gets by meaning. A layout never
invents a parameter: labels, ranges and option names come from the unit, and
an address the Sound doesn't show now (another variant's) is simply not
drawn. The modules every voice engine shares live here.

A `Module` is opaque: a layout builds one with `module_` and the `with`
setters, and every address in it comes from `Addr`'s typed builders.

-}

import Addr exposing (Addr, Block(..), Env, Lfo, PadBlock(..))
import Dict exposing (Dict)
import Layout.Page exposing (UnitPage(..))
import Protocol
import Schema exposing (Schema)
import Ui.Chip as Chip exposing (Chip)
import Ui.Pad exposing (Pad)
import Ui.Screen exposing (Focus, Glyph, Shape(..))



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


{-| A screen: the unit's page whose art it draws, its shape, the crop under
it while the WASM loads, and the cell the unit's focus band is on.
-}
type alias ScreenAt =
    { page : UnitPage, shape : Shape, fallback : Maybe Glyph, focus : Maybe Focus }


type Span
    = Half
    | Whole


type Module
    = Module
        { title : String
        , note : String
        , chip : Chip
        , header : List Addr
        , screen : Maybe ScreenAt
        , groups : List Group
        , extras : List Extra
        , span : Span
        }


module_ : Chip -> String -> Module
module_ c t =
    Module { title = t, note = "", chip = c, header = [], screen = Nothing, groups = [], extras = [], span = Half }


withChip : Chip -> Module -> Module
withChip c (Module m) =
    Module { m | chip = c }


withNote : String -> Module -> Module
withNote n (Module m) =
    Module { m | note = n }


{-| The title bar's type menus: the selectors that swap the module's body.
-}
withHeader : List Addr -> Module -> Module
withHeader h (Module m) =
    Module { m | header = h }


withScreen : ScreenAt -> Module -> Module
withScreen s (Module m) =
    Module { m | screen = Just s }


withGroups : List Group -> Module -> Module
withGroups gs (Module m) =
    Module { m | groups = gs }


addGroups : List Group -> Module -> Module
addGroups gs (Module m) =
    Module { m | groups = m.groups ++ gs }


withExtras : List Extra -> Module -> Module
withExtras xs (Module m) =
    Module { m | extras = xs }


{-| A module that takes a whole row (MOD).
-}
whole : Module -> Module
whole (Module m) =
    Module { m | span = Whole }


title : Module -> String
title (Module m) =
    m.title


note : Module -> String
note (Module m) =
    m.note


chip : Module -> Chip
chip (Module m) =
    m.chip


header : Module -> List Addr
header (Module m) =
    m.header


screen : Module -> Maybe ScreenAt
screen (Module m) =
    m.screen


extras : Module -> List Extra
extras (Module m) =
    m.extras


isWhole : Module -> Bool
isWhole (Module m) =
    m.span == Whole


group : String -> List Slot -> Group
group =
    Group


auto : Addr -> Slot
auto a =
    { addr = a, as_ = Auto }


knob : Addr -> Slot
knob a =
    { addr = a, as_ = AsKnob }


hero : Addr -> Slot
hero a =
    { addr = a, as_ = AsHero }


fader : Addr -> Slot
fader a =
    { addr = a, as_ = AsFader }


heroFader : Addr -> Slot
heroFader a =
    { addr = a, as_ = AsHeroFader }


menu : Addr -> Slot
menu a =
    { addr = a, as_ = AsMenu }


strip : UnitPage -> ScreenAt
strip page =
    { page = page, shape = Strip, fallback = Nothing, focus = Nothing }


tall : UnitPage -> ScreenAt
tall page =
    { page = page, shape = Tall, fallback = Nothing, focus = Nothing }


withFallback : Glyph -> ScreenAt -> ScreenAt
withFallback g s =
    { s | fallback = Just g }


withFocus : Focus -> ScreenAt -> ScreenAt
withFocus f s =
    { s | focus = Just f }


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
                    Addr.at (Pad p (PadRoute k)) (Addr.field f)
            in
            { source = at "SOURCE", dest = at "DEST", amount = at "AMOUNT" }
        )
        Addr.routeSlots


{-| Every address the module places.
-}
addresses : Module -> List Addr
addresses (Module m) =
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


{-| Where an item sits on a page: in a module, or in no module (OTHER's, so
nothing the unit describes is ever hidden).
-}
type Placement
    = Placed Module
    | Unplaced


place : List Module -> Item -> Placement
place every =
    let
        -- Built once per layout: the first module that places each address.
        holder : Dict String Module
        holder =
            List.foldr (\m acc -> List.foldl (\a -> Dict.insert (Addr.key a) m) acc (addresses m)) Dict.empty every
    in
    \item ->
        case Dict.get (Addr.key item.param.addr) holder of
            Just m ->
                Placed m

            Nothing ->
                Unplaced


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
                    Addr.key item.param.addr
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
    Dict.fromList (List.map (\i -> ( Addr.key i.param.addr, i )) items)


{-| A module's groups as the Sound shows them now: each slot whose address
the Sound has, and no empty group.
-}
fill : Dict String Item -> Module -> List ( String, List ( Item, As ) )
fill items (Module m) =
    m.groups
        |> List.map (\g -> ( g.title, List.filterMap (\s -> Dict.get (Addr.key s.addr) items |> Maybe.map (\i -> ( i, s.as_ ))) g.slots ))
        |> List.filter (Tuple.second >> List.isEmpty >> not)


{-| A page of all six operators labels its cells OP1..OP6; the cell then says
what the page is about.
-}
labelOf : Schema.Page -> Schema.Panel -> Schema.Param -> String
labelOf pg pn p =
    case ( pn.variant, String.toList p.label ) of
        ( "-", [ 'O', 'P', d ] ) ->
            if d >= '1' && d <= '6' then
                Protocol.pageLabel pg.label

            else
                p.label

        _ ->
            p.label



-- THE MODULES EVERY VOICE SHARES


word : String -> Addr.Field
word =
    Addr.field


pitch : Module
pitch =
    let
        a : String -> Addr
        a =
            word >> Addr.at Pitch
    in
    module_ Chip.Pitch "PITCH"
        |> withScreen (strip Pit)
        |> withGroups
            [ group "" [ hero (a "PITCH"), knob (a "FINE") ]
            , group "" [ fader (a "GLIDE_TIME") ]
            , group "" [ auto (a "STEAL"), auto (a "SNAP") ]
            ]


{-| A filter in `block` (FILTER, or a DRUM pad's own), its art on `page`.
-}
filterCore : Block -> UnitPage -> Module
filterCore block page =
    let
        a : String -> Addr
        a =
            word >> Addr.at block
    in
    module_ Chip.Filter "FILTER"
        |> withHeader [ a "KIND" ]
        |> withScreen (tall page)
        |> withGroups
            [ group "" [ hero (a "CUTOFF"), knob (a "RESO") ]
            , group "" [ auto (a "MODE"), auto (a "VOICE"), knob (a "MORPH") ]
            ]


filter : Module
filter =
    filterCore Filter Flt |> addGroups [ group "" [ fader (Addr.at Filter (word "STAB")) ] ]


{-| The filter's own modulation: feedback, the input, and the two jacks.
-}
filterMod : Module
filterMod =
    let
        a : String -> Addr
        a =
            word >> Addr.at Filter
    in
    module_ Chip.Filter "FILTER MOD"
        |> withGroups
            [ group "" [ fader (a "FB"), fader (a "LEVEL") ]
            , group "JACKS" [ menu (a "FM3IN"), menu (a "QMIN") ]
            ]
        |> withExtras [ NextSlice ]


{-| The voice's effect, its art on `page` (FX, or a DRUM pad's).
-}
voiceFx : UnitPage -> Module
voiceFx page =
    let
        a : String -> Addr
        a =
            word >> Addr.at VoiceFx
    in
    module_ Chip.Fx "VOICE FX"
        |> withHeader [ a "TYPE" ]
        |> withScreen (strip page)
        |> withGroups
            [ group "" [ auto (a "PLACE") ]
            , group "" [ hero (a "P1"), knob (a "P2") ]
            , group "" [ fader (a "P3"), fader (a "MIX") ]
            ]


ampOut : Module
ampOut =
    let
        a : String -> Addr
        a =
            word >> Addr.at Out
    in
    module_ Chip.Amp "AMP · OUT"
        |> withNote "LOW-PASS GATE · STEREO · SENDS"
        |> withScreen (strip Snd)
        |> withGroups
            [ group "GATE" [ heroFader (a "LPG"), fader (a "DECAY"), fader (a "STRIKE"), fader (a "VEL"), fader (a "VCA") ]
            , group "STEREO" [ knob (a "PAN"), fader (a "WIDTH") ]
            , group "OUT" [ heroFader (a "LEVEL"), fader (a "SEND1"), fader (a "SEND2"), fader (a "SEND3") ]
            ]


{-| An envelope: type A's stages or type B's function, by the unit's TYPE.
-}
envelope : Env -> Module
envelope e =
    let
        a : String -> Addr
        a =
            word >> Addr.at (EnvBlock e)
    in
    module_ Chip.Envelopes ("ENVELOPE " ++ String.fromInt (Addr.envNumber e))
        |> withHeader [ a "TYPE" ]
        |> withScreen (tall (EnvPage e))
        |> withGroups
            [ group "" [ fader (a "ATK"), fader (a "H"), fader (a "DEC"), fader (a "SUS"), fader (a "REL") ]
            , group "" [ fader (a "RISE"), fader (a "FALL"), knob (a "SHAPE") ]
            , group "" [ auto (a "MODE"), auto (a "FORM"), auto (a "SPEED"), auto (a "HOLD") ]
            ]


lfo : Lfo -> Module
lfo l =
    let
        a : String -> Addr
        a =
            word >> Addr.at (LfoBlock l)
    in
    module_ Chip.Lfos ("LFO " ++ String.fromInt (Addr.lfoNumber l))
        |> withHeader [ a "TYPE" ]
        |> withScreen (strip (LfoPage l))
        |> withGroups
            [ group "" [ hero (a "RATE"), hero (a "RISE"), auto (a "SHAPE") ]
            , group "" [ fader (a "DEPTH"), auto (a "SYNC"), knob (a "PHASE"), knob (a "FALL"), knob (a "SHAPE_B"), auto (a "FORM") ]
            ]


{-| The Part's mod matrix, at full width.
-}
mod : Module
mod =
    module_ Chip.Mod "MOD"
        |> withNote "PATCH BAY"
        |> whole
        |> withExtras [ PartMatrix ]
